//! The mesh reading of a B-rep body, for any surface and any curve: what every kernel needs
//! to show, print or export a body, written once for all of them.
//!
//! Every edge is cut into points once, within half the tolerance, and both faces beside it
//! use exactly those points, so the mesh closes without gaps or T-junctions. Each face is
//! laid out in its parameters (`layout`), cut into triangles and refined until the surface
//! stays within the tolerance (`triangulate`); its triangles are turned to face outward.
//!
//! Deterministic budget declines use exact stored-content equality for built-in
//! Line/Circle/Ellipse/NURBS curves and Plane/Sphere/Cylinder/Cone/Torus/Ellipsoid/NURBS
//! surfaces, including effective NURBS projection data. This is a bounded, context-local
//! memo, not a canonical geometry hash or portable serialization: alternate parameterizations,
//! topology order, names, signed-zero bits and transformed numerical data can cause safe
//! misses. Unknown shape/geometry types, nonfinite/oversized identities, oversized reasons,
//! and layouts consulting trim plugins opt out. Failures/cancellation are never cached.
//! `BrepMesh::decline_stats` reports hits/misses/retained tolerances for default settings.

#[cfg(test)]
mod cache_tests;
#[cfg(test)]
mod control_tests;
mod edges;
mod identity;
mod layout;
mod memo;
mod triangulate;

use krly3d_core::math::Point3;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::mesh::Mesh;
use krly3d_kernel::ports::interpret::{MeshDeclineStats, MeshDeclines, MeshRequest, ToMesh};
use krly3d_kernel::ports::shape::Leaf;
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

/// A cooperative failure is never a geometric decline. Only the explicit budget
/// variant is deterministic enough to enter the decline memo.
#[derive(Debug)]
enum Abort {
    Failed(Error),
    Declined(String),
    Budget(String),
}

impl From<Error> for Abort {
    fn from(error: Error) -> Self {
        Self::Failed(error)
    }
}

impl From<String> for Abort {
    fn from(reason: String) -> Self {
        Self::Declined(reason)
    }
}

impl From<&str> for Abort {
    fn from(reason: &str) -> Self {
        Self::Declined(reason.into())
    }
}

impl Abort {
    fn at_face(self, name: &str) -> Self {
        match self {
            Self::Failed(error) => Self::Failed(error),
            Self::Declined(reason) => Self::Declined(format!("face `{name}`: {reason}")),
            Self::Budget(reason) => Self::Budget(format!("face `{name}`: {reason}")),
        }
    }
}

/// Bound the work between checks in loops and recursion without reading the clock
/// for every arithmetic operation. A step is one point/side/candidate, not a round.
struct Work<'a> {
    cx: &'a Context,
    remaining: usize,
    quality: triangulate::Quality,
    cacheable: bool,
}

impl<'a> Work<'a> {
    const CHUNK: usize = 64;

    fn new(cx: &'a Context) -> Self {
        Self { cx, remaining: 0, quality: triangulate::Quality::default(), cacheable: true }
    }

    fn check(&self) -> Result<(), Abort> {
        self.cx.check_control().map_err(Abort::from)
    }

    fn step(&mut self) -> Result<(), Abort> {
        if self.remaining == 0 {
            self.check()?;
            self.remaining = Self::CHUNK;
        }
        self.remaining -= 1;
        Ok(())
    }
}

pub struct BrepMesh;

// Keep this solver's refusals apart from other solvers using the same context.
#[derive(Default)]
struct Declines(MeshDeclines);

/// Geometry-affecting implementation changes require a solver revision bump.
/// Budgets are read by the real triangulator and serialized independently. There
/// are currently no other public quality knobs; triangle-output selection affects
/// packaging only. Context isolation additionally separates plugin registries.
#[derive(Clone, Copy)]
struct Config {
    solver: &'static str,
    api: krly3d_mesh::ApiVersion,
    engine: &'static str,
    artifact: &'static str,
    quality: triangulate::Quality,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            solver: "krly3d.std.brep-mesh/2",
            api: krly3d_mesh::API_VERSION,
            engine: krly3d_mesh::ENGINE_REVISION,
            artifact: krly3d_mesh::ARTIFACT_FORMAT,
            quality: triangulate::Quality::default(),
        }
    }
}

impl BrepMesh {
    /// Decline cache counters for this content/configuration and context. Successes remain in `mesh_of`'s
    /// existing memo. Layout/plugin declines are retried because their cause can depend
    /// on another plugin; only deterministic triangulation budget declines are retained.
    /// Future triangulation failures must remain uncached unless proven deterministic;
    /// timeout/cancellation must never be recorded here. Equal built-in content shares
    /// counters across allocations. Unsupported/oversized bodies opt out of negative
    /// caching. Counters reset on bounded eviction; this is not a persistent cache.
    pub fn decline_stats(cx: &Context, body: &Arc<Body>) -> Attempt<MeshDeclineStats> {
        memo::memo(cx, body, Config::default()).map(|memo| memo.declines.0.stats()).into()
    }
}

impl Plugin<ToMesh> for BrepMesh {
    fn id(&self) -> Id {
        Id::new("brep")
    }

    fn call(&self, cx: &Context, request: &MeshRequest) -> Attempt<Arc<Mesh>> {
        read_mesh(cx, request, triangulate::triangles)
    }
}

// A private seam lets tests count triangulator calls without building 400,000 triangles.
fn read_mesh(
    cx: &Context,
    request: &MeshRequest,
    triangulate: impl FnMut(
        &mut Work<'_>,
        &mut layout::Layout,
        &dyn Surface,
        &mut Vec<Point3>,
        f64,
    ) -> Result<Vec<[usize; 3]>, Abort>,
) -> Attempt<Arc<Mesh>> {
    read_mesh_config(cx, request, Config::default(), triangulate)
}

fn read_mesh_config(
    cx: &Context,
    request: &MeshRequest,
    config: Config,
    triangulate: impl FnMut(
        &mut Work<'_>,
        &mut layout::Layout,
        &dyn Surface,
        &mut Vec<Point3>,
        f64,
    ) -> Result<Vec<[usize; 3]>, Abort>,
) -> Attempt<Arc<Mesh>> {
    if let Err(error) = cx.check_control() {
        return Attempt::Failed(error);
    }
    let Some(leaf) = request.shape.as_any().downcast_ref::<Leaf>() else {
        return Attempt::declined("the brep mesh rule reads B-rep leaves");
    };
    if !(request.tolerance.is_finite() && request.tolerance > cx.accuracy().length()) {
        return Attempt::Failed(Error::input(format!(
            "a mesh tolerance must be above the length tolerance, got {}",
            request.tolerance
        )));
    }
    let memo = match memo::memo(cx, &leaf.body, config) {
        Ok(memo) => memo,
        Err(error) => return Attempt::Failed(error),
    };
    if let Some(reason) = memo.declines.0.get(request.tolerance) {
        return match cx.publish_control(|| reason) {
            Ok(reason) => Attempt::Declined(reason),
            Err(error) => Attempt::Failed(error),
        };
    }
    // Layout can consult other plugins, whose untyped declines/failures may be
    // transient. Only triangulate's fixed triangle-budget refusal is memoized.
    match meshed(cx, &leaf.body, request.tolerance, config.quality, triangulate) {
        Ok(mesh) => cx.publish_control(|| Arc::new(mesh)).into(),
        Err(Abort::Failed(error)) => Attempt::Failed(error),
        Err(Abort::Declined(reason)) => match cx.check_control() {
            Ok(()) => Attempt::Declined(reason),
            Err(error) => Attempt::Failed(error),
        },
        Err(Abort::Budget(reason)) => match cx.publish_control(|| {
            if memo.cacheable {
                memo.declines.0.remember(request.tolerance, reason.clone());
            }
            reason
        }) {
            Ok(reason) => Attempt::Declined(reason),
            Err(error) => Attempt::Failed(error),
        },
    }
}

fn meshed(
    cx: &Context,
    body: &Arc<Body>,
    tolerance: f64,
    quality: triangulate::Quality,
    mut triangulate: impl FnMut(
        &mut Work<'_>,
        &mut layout::Layout,
        &dyn Surface,
        &mut Vec<Point3>,
        f64,
    ) -> Result<Vec<[usize; 3]>, Abort>,
) -> Result<Mesh, Abort> {
    let mut work = Work::new(cx);
    work.quality = quality;
    let mut points = Vec::with_capacity(body.vertices.len());
    for point in &body.vertices {
        work.step()?;
        points.push(*point);
    }
    let edges = edges::sample(&mut work, body, 0.5 * tolerance, &mut points)?;
    let mut triangles = Vec::new();
    let mut source_faces = Vec::new();
    for (index, face) in body.faces.iter().enumerate() {
        work.check()?;
        let mut flat = layout::lay_out(&mut work, body, index, &edges, &mut points, 0.5 * tolerance)
            .map_err(|abort| abort.at_face(&face.name))?;
        let found = triangulate(&mut work, &mut flat, face.surface.as_ref(), &mut points, tolerance)
            .map_err(|abort| match abort {
                Abort::Budget(reason) if !work.cacheable => Abort::Declined(reason),
                other => other,
            })
            .map_err(|abort| abort.at_face(&face.name))?;
        let mut kept = Vec::new();
        for corners in found {
            work.step()?;
            let ids = corners.map(|corner| flat.ids[corner]);
            if ids[0] != ids[1] && ids[1] != ids[2] && ids[2] != ids[0] {
                kept.push(ids);
            }
        }
        // The layout's triangles all turn the same way in the plane, so one sense holds for
        // the whole face. It is voted by area: a sliver's normal is too noisy to trust alone.
        let mut vote = -0.0;
        for ids in &kept {
            work.step()?;
            let [a, b, c] = ids.map(|id| points[id]);
            let normal = (b - a).cross(&(c - a));
            let centre = Point3::from((a.coords + b.coords + c.coords) / 3.0);
            vote += normal.dot(&face.outward_normal(&centre));
        }
        for ids in kept {
            work.step()?;
            triangles.push(if vote >= 0.0 { ids } else { [ids[0], ids[2], ids[1]] });
            source_faces.push(index);
        }
    }
    work.check()?;
    Ok(Mesh { points, triangles, source_faces: Some(source_faces) })
}

#[cfg(test)]
mod tests {
    use super::{read_mesh, Abort, BrepMesh};
    use crate::tool::testing::{area, context, made, volume};
    use krly3d_core::{Attempt, Error, Id};
    use krly3d_kernel::mesh::Mesh;
    use krly3d_kernel::ports::interpret::contract::check_mesh;
    use krly3d_kernel::ports::interpret::{mesh_of, MeshDeclineStats, MeshRequest, ToMesh};
    use krly3d_kernel::ports::shape::Leaf;
    use krly3d_kernel::schema::{Params, Value};
    use krly3d_kernel::topo::Body;
    use krly3d_kernel::{Context, KernelBuilder, Plugin};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct BudgetMesh(Arc<AtomicUsize>);

    impl Plugin<ToMesh> for BudgetMesh {
        fn id(&self) -> Id {
            Id::new("brep")
        }

        fn call(&self, cx: &Context, request: &MeshRequest) -> Attempt<Arc<Mesh>> {
            read_mesh(cx, request, |_, _, _, _, tolerance| {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(Abort::Budget(format!(
                    "the face needs more than 400000 triangles for a tolerance of {tolerance}"
                )))
            })
        }
    }

    #[test]
    fn a_second_budget_decline_skips_tessellation_and_preserves_diagnostics() {
        let geometry = context();
        let body = made(&geometry, "box", Params::new().with("w", 2.0).with("d", 3.0).with("h", 4.0), &[]);
        let equal = made(&geometry, "box", Params::new().with("w", 2.0).with("d", 3.0).with("h", 4.0), &[]);
        assert!(!Arc::ptr_eq(&body, &equal));
        assert!(!Arc::ptr_eq(&body.edges[0].curve, &equal.edges[0].curve));
        assert!(!Arc::ptr_eq(&body.faces[0].surface, &equal.faces[0].surface));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = KernelBuilder::new();
        builder.plugin::<ToMesh>(BudgetMesh(Arc::clone(&calls)));
        let cx = builder.context();
        let shape = Leaf::of(Arc::clone(&body));
        let first = mesh_of(&cx, &shape, 0.22);
        let again = mesh_of(&cx, &Leaf::of(Arc::clone(&equal)), 0.22);
        let reason = format!(
            "face `{}`: the face needs more than 400000 triangles for a tolerance of 0.22",
            body.faces[0].name
        );
        assert_eq!(first, Attempt::declined(format!("mesh reading `brep` declined: {reason}")));
        assert_eq!(again, first);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            BrepMesh::decline_stats(&cx, &body),
            Attempt::Done(MeshDeclineStats { hits: 1, misses: 1, entries: 1 })
        );
        assert_eq!(BrepMesh::decline_stats(&cx, &equal), BrepMesh::decline_stats(&cx, &body));

        let _ = mesh_of(&cx, &shape, 0.23);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(mesh_of(&cx, &shape, 0.22), first);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            BrepMesh::decline_stats(&cx, &body),
            Attempt::Done(MeshDeclineStats { hits: 2, misses: 2, entries: 2 })
        );

        let mut changed = body.as_ref().clone();
        changed.faces[0].name = "changed-face".into();
        let changed = Arc::new(changed);
        assert!(
            matches!(mesh_of(&cx, &Leaf::of(Arc::clone(&changed)), 0.22), Attempt::Declined(reason) if reason.contains("changed-face"))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        let mut builder = KernelBuilder::new();
        builder.accuracy(krly3d_core::Accuracy::for_scale(10.0).unwrap());
        builder.plugin::<ToMesh>(BudgetMesh(Arc::clone(&calls)));
        let _ = mesh_of(&builder.context(), &shape, 0.22);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn layout_declines_and_invalid_requests_are_not_memoized() {
        let cx = context();
        let mut body = made(&cx, "box", Params::new().with("w", 2.0).with("d", 3.0).with("h", 4.0), &[])
            .as_ref()
            .clone();
        body.faces[0].loops.clear(); // An unbounded plane has no mesh layout.
        let body = Arc::new(body);
        let shape = Leaf::of(Arc::clone(&body));
        let first = mesh_of(&cx, &shape, 0.22);
        assert!(matches!(&first, Attempt::Declined(reason) if reason.contains("a face without boundary")));
        assert_eq!(mesh_of(&cx, &shape, 0.22), first);
        assert_eq!(
            BrepMesh::decline_stats(&cx, &body),
            Attempt::Done(MeshDeclineStats { hits: 0, misses: 2, entries: 0 })
        );
        for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            for _ in 0..2 {
                assert!(matches!(mesh_of(&cx, &shape, tolerance), Attempt::Failed(Error { .. })));
            }
        }
        assert_eq!(
            BrepMesh::decline_stats(&cx, &body),
            Attempt::Done(MeshDeclineStats { hits: 0, misses: 2, entries: 0 })
        );
    }

    fn closed_with_its_volume(cx: &Context, body: &Arc<Body>, tolerance: f64) {
        let shape = Leaf::of(Arc::clone(body));
        let kinds: Vec<&str> = body.faces.iter().map(|face| face.surface.classes()[0]).collect();
        if let Err(problem) = check_mesh(cx, &shape, tolerance, volume(cx, body), area(cx, body)) {
            panic!("a body of {kinds:?}: {problem}");
        }
    }

    fn numbers(values: &[f64]) -> Value {
        Value::List(values.iter().map(|value| (*value).into()).collect())
    }

    #[test]
    fn primitives_mesh_into_closed_meshes_holding_their_volume() {
        let cx = context();
        for (tool, params) in [
            ("box", Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0)),
            ("cylinder", Params::new().with("r", 3.0).with("h", 5.0)),
            ("cone", Params::new().with("r", 3.0).with("h", 5.0)),
            ("cone", Params::new().with("r", 3.0).with("h", 5.0).with("top", 1.0)),
            ("sphere", Params::new().with("r", 3.0)),
            ("torus", Params::new().with("ring", 4.0).with("tube", 1.0)),
            ("ellipsoid", Params::new().with("rx", 4.0).with("ry", 2.0).with("rz", 3.0)),
        ] {
            let body = made(&cx, tool, params, &[]);
            closed_with_its_volume(&cx, &body, 0.01);
        }
    }

    #[test]
    fn faces_with_holes_seams_and_cuts_mesh_closed() {
        let cx = context();
        let outline = Params::new().with("path", "M -30,-20 L 30,-20 L 30,20 L -30,20 Z O 20,15 4");
        let plate =
            made(&cx, "extrude", Params::new().with("h", 10.0), &[made(&cx, "profile", outline, &[])]);
        let rod = made(
            &cx,
            "cylinder",
            Params::new().with("r", 4.0).with("h", 30.0).with("at", [-10.0, 0.0, -10.0]),
            &[],
        );
        let drilled = made(&cx, "subtract", Params::new(), &[Arc::clone(&plate), rod]);
        let rounded = made(
            &cx,
            "fillet",
            Params::new().with("edges", Value::texts(["front|top"])).with("r", 2.0),
            &[made(&cx, "box", Params::new().with("w", 20.0).with("d", 10.0).with("h", 6.0), &[])],
        );
        let bar = made(
            &cx,
            "pipe",
            Params::new()
                .with("r", 1.0)
                .with("bend", 3.0)
                .with("points", numbers(&[0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0])),
            &[],
        );
        for body in [plate, drilled, rounded, bar] {
            closed_with_its_volume(&cx, &body, 0.02);
        }
    }

    #[test]
    fn a_loft_through_sections_meshes_closed() {
        let cx = context();
        let section = |path: &str, z: f64| {
            made(&cx, "profile", Params::new().with("path", path).with("at", [0.0, 0.0, z]), &[])
        };

        let vase = made(
            &cx,
            "loft",
            Params::new(),
            &[section("O 0,0 4", 0.0), section("O 0,0 6", 4.0), section("O 0,0 2", 9.0)],
        );

        closed_with_its_volume(&cx, &vase, 0.02);
    }

    #[test]
    fn a_finer_tolerance_gives_a_closer_volume() {
        let cx = context();
        let ball = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);
        let shape = Leaf::of(Arc::clone(&ball));
        let exact = volume(&cx, &ball);

        let coarse = mesh_of(&cx, &shape, 0.2).into_result().unwrap();
        let fine = mesh_of(&cx, &shape, 0.005).into_result().unwrap();

        assert!((fine.volume() - exact).abs() < (coarse.volume() - exact).abs());
        assert!(fine.triangles.len() > coarse.triangles.len());
    }

    #[test]
    fn a_mesh_asked_for_again_at_the_same_tolerance_is_the_one_made_before() {
        let cx = context();
        let ball = Leaf::of(made(&cx, "sphere", Params::new().with("r", 5.0), &[]));

        let first = mesh_of(&cx, &ball, 0.2).into_result().unwrap();
        let again = mesh_of(&cx, &ball, 0.2).into_result().unwrap();
        let finer = mesh_of(&cx, &ball, 0.1).into_result().unwrap();

        assert!(Arc::ptr_eq(&first, &again));
        assert!(!Arc::ptr_eq(&first, &finer));
        let body = &ball.as_any().downcast_ref::<Leaf>().unwrap().body;
        assert_eq!(
            BrepMesh::decline_stats(&cx, body),
            Attempt::Done(MeshDeclineStats { hits: 0, misses: 2, entries: 0 })
        );
    }

    /// A section of two circular arcs meeting at a corner at each end, as an airfoil is
    /// drawn, in the xz plane at `y`.
    fn lens(cx: &Context, front: f64, back: f64, thickness: f64, y: f64) -> Arc<Body> {
        let (half, middle) = (0.5 * thickness, 0.5 * (front + back));
        let drop = ((0.5 * (back - front)).powi(2) + half * half) / thickness - half;
        let path = format!("M {front},0 A {middle},{} {back},0 cw A {middle},{drop} {front},0 cw Z", -drop);
        made(
            cx,
            "profile",
            Params::new().with("path", path.as_str()).with("plane", "xz").with("at", [0.0, y, 0.0]),
            &[],
        )
    }

    #[test]
    fn a_loft_of_sharp_edged_sections_swept_hard_meshes_closed() {
        let cx = context();
        // The chine of an aircraft: its leading edge runs nearly along the span.
        let chine = made(
            &cx,
            "loft",
            Params::new(),
            &[lens(&cx, 4.6, 11.4, 0.12, -1.2), lens(&cx, 8.0, 11.4, 0.1, -1.86)],
        );

        closed_with_its_volume(&cx, &chine, 0.022);
    }
}
