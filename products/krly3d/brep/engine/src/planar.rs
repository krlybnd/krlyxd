//! Composition of existing native profile, extrusion, topology and document providers.

use crate::{diagnostic, legacy_diagnostic, BrepEngine, Snapshot};
use krly2d_api::Region;
use krly3d_brep::planar::{PlanarCall, PlanarInput, PlanarQuality, PlanarResult, PROTOCOL};
use krly3d_brep::{Diagnostic, Id};
use krly3d_core::math::{Point3, Vec3};
use krly3d_kernel::model::{Document, Statement, GEOMETRY};
use krly3d_kernel::ports::meet::{meet, MeetRequest, Meeting};
use krly3d_kernel::ports::sweep::{sweep, Motion, SweepRequest};
use krly3d_kernel::ports::tool::run;
use krly3d_kernel::ports::trim::{place, Place, TrimRequest};
use krly3d_kernel::schema::Params;
use krly3d_kernel::topo::{check, Body, Edge};
use krly3d_kernel::{Context, Kernel};
use krlyxd::{CallControl, CallStop};
use std::fmt::Write as _;
use std::sync::Arc;

/// A planar extrusion retained in memory with its originating engine and snapshot.
pub struct RetainedPlanar {
    pub engine: BrepEngine,
    pub snapshot: Snapshot,
    /// The same immutable input region supplied to native construction.
    pub region: Arc<Region>,
    /// Caller-supplied provenance claim, copied without verification.
    pub source: String,
    pub request_id: String,
    pub quality: PlanarQuality,
}

/// Build one bounded planar extrusion and explicitly export its native wire artifact.
pub fn build_planar(call: PlanarCall) -> Result<PlanarResult, Diagnostic> {
    let retained = build_planar_retained(call)?;
    let model = retained.engine.to_k3d()?;
    Ok(PlanarResult {
        protocol: PROTOCOL.into(),
        request_id: retained.request_id,
        model,
        node: retained.snapshot.identity().node.clone(),
        source: retained.source,
        provenance: retained.snapshot.identity().clone(),
        quality: retained.quality,
    })
}

/// Build exactly one bounded planar extrusion without native export or tessellation.
/// The snapshot pins the constructed shape and services independently of the engine.
pub fn build_planar_retained(call: PlanarCall) -> Result<RetainedPlanar, Diagnostic> {
    build_planar_retained_controlled(call, Arc::new(CallControl::new()))
}

/// Controlled counterpart of [`build_planar_retained`], including the coarse path.
/// The token controls this computation only; retained services do not retain it.
/// See [`build_region_retained_controlled`] for interruption and cache limitations.
pub fn build_planar_retained_controlled(
    call: PlanarCall,
    control: Arc<CallControl>,
) -> Result<RetainedPlanar, Diagnostic> {
    control.check().map_err(stop_diagnostic)?;
    call.validate()?;
    control.check().map_err(stop_diagnostic)?;
    let region = Arc::new(Region::from_polygons(call.input.polygons, krlyxd::LengthUnit::Model)?);
    build_region_retained_controlled(region, call.input.height, call.request_id, call.input.source, control)
}

/// Extrude a shared krly2d region using the existing native providers. This first
/// bridge accepts straight boundaries in model units; it never silently samples
/// curves or converts units. Structural region validation is not a topology proof:
/// native meet/trim providers still reject crossing, touching or misplaced holes.
pub fn build_region_retained(
    region: Arc<Region>,
    height: f64,
    request_id: String,
    source: String,
) -> Result<RetainedPlanar, Diagnostic> {
    build_region_retained_controlled(region, height, request_id, source, Arc::new(CallControl::new()))
}

/// Cooperatively construct a native extrusion without binding the retained owner
/// or snapshot to the caller's token. Legacy wrappers use a token with no deadline.
///
/// Checks bound our path, topology, edge-pair, containment and finite-solid loops;
/// a controlled context also checks native node/edit and nested provider boundaries.
/// Individual profile parsing/building, sweep/meet/trim calls, naming, topology
/// checks, document validation/delta/commit and region conversion remain synchronous:
/// they cannot be interrupted internally except at checks their providers implement.
/// Extrusion edge/vertex copying and cap/side assembly between native sweeps
/// likewise finish their current operation before the next provider checkpoint.
/// Allocation, path joining and destructors are likewise not interruptible.
///
/// Construction and destruction run outside the publication gate. A final short
/// guard accepts the completed result; the caller still owns host publication.
/// The completed document/model move through a checkpoint into an uncontrolled
/// kernel sharing the original services and derived caches. Kernel checkpoints do
/// not carry the document-build cache, so later edits may rebuild existing nodes.
pub fn build_region_retained_controlled(
    region: Arc<Region>,
    height: f64,
    request_id: String,
    source: String,
    control: Arc<CallControl>,
) -> Result<RetainedPlanar, Diagnostic> {
    control.check().map_err(stop_diagnostic)?;
    build_region_in_context(region, height, request_id, source, &control, krly3d_std::standard().context())
}

fn stop_diagnostic(stop: CallStop) -> Diagnostic {
    legacy_diagnostic(krly3d_core::Error::from(stop))
}

fn checkpoint(cx: &Context) -> Result<(), Diagnostic> {
    cx.check_control().map_err(legacy_diagnostic)
}

fn build_region_in_context(
    region: Arc<Region>,
    height: f64,
    request_id: String,
    source: String,
    control: &Arc<CallControl>,
    services: Context,
) -> Result<RetainedPlanar, Diagnostic> {
    let cx = services.with_call_control(Arc::clone(control));
    checkpoint(&cx)?;
    if region.length_unit() != krlyxd::LengthUnit::Model {
        return Err(diagnostic("unsupported", "planar construction requires explicit model units"));
    }
    let call = PlanarCall {
        protocol: PROTOCOL.into(),
        request_id,
        input: PlanarInput { polygons: region.polygons()?, height, length_unit: "model".into(), source },
    };
    checkpoint(&cx)?;
    // Preserve the bounded B-rep contract for direct native callers too. The
    // temporary polygon/path values adapt to the existing profile provider.
    call.validate()?;
    checkpoint(&cx)?;
    let paths =
        call.input.polygons.iter().map(|polygon| path(&cx, polygon)).collect::<Result<Vec<_>, _>>()?;
    validate_topology(&cx, &paths)?;
    checkpoint(&cx)?;
    let profile = Statement::new(
        "outline_profile",
        "profile",
        Params::new().with("path", paths.join(" ")).with("plane", "xy"),
        &[],
    )
    .map_err(legacy_diagnostic)?;
    let part =
        Statement::new("part", "extrude", Params::new().with("h", call.input.height), &["outline_profile"])
            .map_err(legacy_diagnostic)?;
    let document = Document::default().with_in(GEOMETRY, profile).with_in(GEOMETRY, part);
    checkpoint(&cx)?;
    let mut construction = Kernel::new(cx.clone());
    let delta = construction.open(document).map_err(legacy_diagnostic)?;
    checkpoint(&cx)?;
    if delta.refused() {
        return Err(diagnostic("input", delta.text()));
    }
    let body = construction.body("part@1").map_err(legacy_diagnostic)?;
    check(&body, cx.accuracy()).map_err(legacy_diagnostic)?;
    checkpoint(&cx)?;
    if !body.solid {
        return Err(diagnostic("geometry", "native extrusion did not produce a finite solid"));
    }
    for point in &body.vertices {
        checkpoint(&cx)?;
        if !point.coords.iter().all(|value| value.is_finite()) {
            return Err(diagnostic("geometry", "native extrusion did not produce a finite solid"));
        }
    }
    for edge in &body.edges {
        for fraction in [0.0, 0.5, 1.0] {
            checkpoint(&cx)?;
            if !edge.point(fraction).coords.iter().all(|value| value.is_finite()) {
                return Err(diagnostic("geometry", "native extrusion did not produce a finite solid"));
            }
        }
    }
    checkpoint(&cx)?;
    let mut kernel = Kernel::new(services);
    kernel.rollback(construction.checkpoint());
    // The temporary kernel owns plugin/cache values whose destructors may call
    // back into control. Never destroy them under the publication mutex.
    drop(construction);
    let engine = BrepEngine::new(kernel)?;
    let snapshot = engine.snapshot("part@1")?;
    let retained = RetainedPlanar {
        engine,
        snapshot,
        region,
        request_id: call.request_id,
        source: call.input.source,
        quality: PlanarQuality::default(),
    };
    publish_retained(retained, control)
}

fn publish_retained(retained: RetainedPlanar, control: &CallControl) -> Result<RetainedPlanar, Diagnostic> {
    // Capture no candidate in this closure: rejection must drop it after the
    // publication lock has been released, even when a destructor calls cancel.
    control.publish(|| ()).map_err(stop_diagnostic)?;
    Ok(retained)
}

fn path(cx: &Context, polygon: &[[f64; 2]]) -> Result<String, Diagnostic> {
    let mut path = String::new();
    for (index, [x, y]) in polygon.iter().enumerate() {
        checkpoint(cx)?;
        let command = if index == 0 { "M" } else { "L" };
        // Display preserves round-trip f64 precision, including nonzero origins.
        let _ = write!(path, "{command} {x},{y} ");
    }
    path.push('Z');
    Ok(path)
}

fn unsupported() -> Diagnostic {
    diagnostic("unsupported", "native planar providers require simple, non-touching loops: first outer, remaining disjoint interior holes")
}

fn validate_topology(cx: &Context, paths: &[String]) -> Result<(), Diagnostic> {
    let loops = paths
        .iter()
        .map(|path| {
            checkpoint(cx)?;
            let body = run(
                cx,
                &Id::new("profile"),
                &Params::new().with("path", path.as_str()).with("plane", "xy"),
                &[],
            )
            .into_result()
            .map_err(legacy_diagnostic)?;
            checkpoint(cx)?;
            check(&body, cx.accuracy()).map_err(legacy_diagnostic)?;
            checkpoint(cx)?;
            if body.faces.len() != 1 || body.faces[0].loops.len() != 1 || body.edges.len() < 3 {
                return Err(unsupported());
            }
            Ok(Arc::new(body))
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    // Intersections are answered by the native line/plane meet and sweep providers,
    // never by a second polygon intersection or winding implementation.
    validate_boundaries(cx, &loops)?;
    let outer = loops.first().ok_or_else(unsupported)?;
    for (index, hole) in loops.iter().enumerate().skip(1) {
        checkpoint(cx)?;
        let point = *hole.vertices.first().ok_or_else(unsupported)?;
        if location(cx, outer, point)? != Place::Inside {
            return Err(unsupported());
        }
        for other in loops.iter().take(index).skip(1) {
            checkpoint(cx)?;
            let other_point = *other.vertices.first().ok_or_else(unsupported)?;
            if location(cx, other, point)? != Place::Outside
                || location(cx, hole, other_point)? != Place::Outside
            {
                return Err(unsupported());
            }
        }
    }
    Ok(())
}

fn location(cx: &Context, body: &Arc<Body>, point: Point3) -> Result<Place, Diagnostic> {
    checkpoint(cx)?;
    let location = place(cx, &TrimRequest { body: Arc::clone(body), face: 0, point })
        .into_result()
        .map_err(legacy_diagnostic)?;
    checkpoint(cx)?;
    Ok(location)
}

fn validate_boundaries(cx: &Context, loops: &[Arc<Body>]) -> Result<(), Diagnostic> {
    let mut edges = Vec::new();
    for (loop_index, body) in loops.iter().enumerate() {
        checkpoint(cx)?;
        for (index, edge) in body.edges.iter().enumerate() {
            checkpoint(cx)?;
            edges.push((loop_index, index, edge));
        }
    }
    let motion = Motion::shift(Vec3::z()).map_err(legacy_diagnostic)?;
    let walls = edges
        .iter()
        .map(|(_, _, edge)| {
            checkpoint(cx)?;
            let wall = sweep(cx, &SweepRequest { curve: Arc::clone(&edge.curve), motion })
                .into_result()
                .map_err(legacy_diagnostic)?;
            checkpoint(cx)?;
            Ok(wall)
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    for (index, (loop_a, index_a, a)) in edges.iter().enumerate() {
        checkpoint(cx)?;
        for (loop_b, index_b, b) in edges.iter().take(index) {
            checkpoint(cx)?;
            let count = loops[*loop_a].edges.len();
            let adjacent = loop_a == loop_b
                && (index_a.abs_diff(*index_b) == 1 || index_a.abs_diff(*index_b) == count - 1);
            let meeting =
                meet(cx, &MeetRequest { curve: Arc::clone(&b.curve), surface: Arc::clone(&walls[index]) })
                    .into_result()
                    .map_err(legacy_diagnostic)?;
            checkpoint(cx)?;
            validate_meeting(cx, a, b, adjacent, meeting)?;
        }
    }
    checkpoint(cx)?;
    Ok(())
}

fn on_edge(cx: &Context, edge: &Edge, point: &Point3) -> bool {
    cx.accuracy().same_point(&edge.curve.point(edge.nearest(point)), point)
}

fn endpoint(cx: &Context, edge: &Edge, point: &Point3) -> bool {
    [edge.point(0.0), edge.point(1.0)].iter().any(|end| cx.accuracy().same_point(end, point))
}

fn validate_meeting(
    cx: &Context,
    a: &Edge,
    b: &Edge,
    adjacent: bool,
    meeting: Meeting,
) -> Result<(), Diagnostic> {
    checkpoint(cx)?;
    let candidates = match meeting {
        Meeting::At { parameters, .. } => parameters
            .into_iter()
            .map(|t| {
                checkpoint(cx)?;
                Ok(b.curve.point(t))
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?,
        Meeting::Within => vec![a.point(0.0), a.point(1.0), b.point(0.0), b.point(1.0)],
    };
    let mut contacts = 0;
    for point in candidates {
        checkpoint(cx)?;
        if !point.coords.iter().all(|value| value.is_finite()) {
            return Err(unsupported());
        }
        if on_edge(cx, a, &point) && on_edge(cx, b, &point) {
            contacts += 1;
            if !adjacent || !endpoint(cx, a, &point) || !endpoint(cx, b, &point) {
                return Err(unsupported());
            }
        }
    }
    checkpoint(cx)?;
    // Collinear adjacent segments can share one endpoint, counted twice above.
    if contacts > 2 {
        return Err(unsupported());
    }
    Ok(())
}

#[cfg(test)]
mod control_tests;
