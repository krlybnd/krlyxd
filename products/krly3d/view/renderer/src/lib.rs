//! Local view API 1.0 execution over immutable B-rep snapshots.
//!
//! Cameras, rendering and PNG encoding use the existing algorithms. There is no
//! wire adapter or independent projection implementation in this crate.

pub mod likeness;
pub mod runner;

use krly3d_brep_engine::{BrepEngine, Snapshot};
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::{CallControl, Error};
use krly3d_kernel::ports::interpret::body_of;
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};
use krly3d_view::ports::render::{Frame, Image, RenderRequest, View, CHANNELS};
use krly3d_view_api::{
    diagnostic, Backend, Camera, Diagnostic, Exactness, FaceCoverage, Id, Provenance, Quality, RenderCall,
    RenderResult, Timing, ENGINE_REVISION,
};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Owns a B-rep session and a distinct renderer identity.
pub struct ViewEngine {
    brep: BrepEngine,
    owner: Arc<()>,
}

/// A validated, normalized call and its pinned source. Callers cannot forge or
/// replace either. Executing consumes the handle; preparing another call is cheap.
#[derive(Debug)]
pub struct PreparedRender {
    snapshot: Snapshot,
    call: RenderCall,
    owner: Arc<()>,
}

/// Render a retained snapshot using its originating services, even after its owner
/// is dropped. The requested node must be its canonical node or unversioned name.
pub fn render_snapshot(snapshot: Snapshot, call: RenderCall) -> Result<RenderResult, Diagnostic> {
    let context = snapshot.legacy_context().clone();
    render_snapshot_in(snapshot, call, &context)
}

/// Render a retained snapshot with cooperative cancellation, deadlines and guarded
/// final publication. Control is attached only to this call's source context clone.
pub fn render_snapshot_controlled(
    snapshot: Snapshot,
    call: RenderCall,
    control: Arc<CallControl>,
) -> Result<RenderResult, Diagnostic> {
    let context = snapshot.legacy_context().with_call_control(control);
    render_snapshot_in(snapshot, call, &context)
}

fn render_snapshot_in(
    snapshot: Snapshot,
    mut call: RenderCall,
    context: &Context,
) -> Result<RenderResult, Diagnostic> {
    call.validate()?;
    let node = &snapshot.identity().node;
    let name = node.rsplit_once('@').map_or(node.as_str(), |(name, _)| name);
    if call.input.node != *node && call.input.node != name {
        return Err(diagnostic("input", "render node does not match the retained snapshot"));
    }
    validate_mesh_tolerance(&snapshot, &call)?;
    call.input.node = node.clone();
    render_prepared(snapshot, call, context).map_err(legacy_diagnostic)
}

fn validate_mesh_tolerance(snapshot: &Snapshot, call: &RenderCall) -> Result<(), Diagnostic> {
    if call.input.backend == Backend::Mesh
        && call.input.tolerance.value <= snapshot.legacy_context().accuracy().length()
    {
        return Err(legacy_diagnostic(Error::input(
            "a mesh preview tolerance must be finite and above the length tolerance",
        )));
    }
    Ok(())
}

impl ViewEngine {
    pub fn new(brep: BrepEngine) -> Self {
        Self { brep, owner: Arc::new(()) }
    }

    pub fn from_k3d(text: &str) -> Result<Self, Diagnostic> {
        Ok(Self::new(BrepEngine::from_k3d(text)?))
    }

    pub fn brep(&self) -> &BrepEngine {
        &self.brep
    }

    /// Edits retain prepared snapshots. Replacing this owner with another session
    /// invalidates previously prepared work on this renderer.
    pub fn brep_mut(&mut self) -> &mut BrepEngine {
        &mut self.brep
    }

    /// Resolve the requested node/version exactly once, without rendering.
    pub fn prepare(&self, mut call: RenderCall) -> Result<PreparedRender, Diagnostic> {
        call.validate()?;
        let snapshot = self.brep.snapshot(&call.input.node)?;
        validate_mesh_tolerance(&snapshot, &call)?;
        call.input.node = snapshot.identity().node.clone();
        Ok(PreparedRender { snapshot, call, owner: Arc::clone(&self.owner) })
    }

    /// Render only the prepared source, regardless of subsequent edits or rollback.
    pub fn execute(&self, prepared: PreparedRender) -> Result<RenderResult, Diagnostic> {
        let context = prepared.snapshot.legacy_context().clone();
        self.execute_in(prepared, &context)
    }

    /// Cooperative cancellation and injected-clock deadlines apply to conversion,
    /// rendering, face counting, PNG encoding and final publication.
    pub fn execute_controlled(
        &self,
        prepared: PreparedRender,
        control: Arc<CallControl>,
    ) -> Result<RenderResult, Diagnostic> {
        let context = prepared.snapshot.legacy_context().with_call_control(control);
        self.execute_in(prepared, &context)
    }

    pub(crate) fn task(
        &self,
        prepared: PreparedRender,
        control: Arc<CallControl>,
    ) -> Result<RenderTask, Diagnostic> {
        if !Arc::ptr_eq(&self.owner, &prepared.owner) || !self.brep.owns(&prepared.snapshot) {
            return Err(diagnostic("input", "a prepared render belongs to a different owner or session"));
        }
        let context = prepared.snapshot.legacy_context().with_call_control(control);
        Ok(RenderTask { prepared, context })
    }

    fn execute_in(&self, prepared: PreparedRender, context: &Context) -> Result<RenderResult, Diagnostic> {
        if !Arc::ptr_eq(&self.owner, &prepared.owner) || !self.brep.owns(&prepared.snapshot) {
            return Err(diagnostic("input", "a prepared render belongs to a different owner or session"));
        }
        render_prepared(prepared.snapshot, prepared.call, context).map_err(legacy_diagnostic)
    }

    pub fn call(&self, call: RenderCall) -> Result<RenderResult, Diagnostic> {
        self.execute(self.prepare(call)?)
    }
}

fn render_prepared(snapshot: Snapshot, call: RenderCall, cx: &Context) -> Result<RenderResult, Error> {
    cx.check_control()?;
    let input = &call.input;
    // Preparation validates dimensions before any image allocation. The call and
    // its dimensions are private and immutable from that point onward.
    let started = cx.clock().now();
    let body = body_of(cx, snapshot.legacy_shape()).into_result()?;
    cx.check_control()?;
    let request = RenderRequest {
        bodies: vec![Arc::clone(&body)],
        view: camera(&input.camera)?,
        size: (input.width as usize, input.height as usize),
        paint: Vec::new(),
    };
    let image = match input.backend {
        Backend::Raycast => krly3d_view::raycast::Raycast.call(cx, &request),
        Backend::Mesh => krly3d_view_mesh::render_mesh(cx, &request, input.tolerance.value),
    }
    .into_result()?;
    let render_ms = cx.clock().now().saturating_sub(started).as_secs_f64() * 1000.0;
    let faces = tally(cx, &image, &body)?;
    let encoding_started = cx.clock().now();
    let png =
        krly3d_view::png::encode_checked(image.width(), image.height(), CHANNELS, &image.pixels, &|| {
            cx.check_control()
        })?;
    let encoding_ms = cx.clock().now().saturating_sub(encoding_started).as_secs_f64() * 1000.0;
    let result = RenderResult {
        request_id: call.request_id,
        width: input.width,
        height: input.height,
        png,
        faces,
        quality: Quality {
            exactness: Exactness::Sampled { samples: input.width * input.height },
            requested_tolerance: (input.backend == Backend::Mesh).then_some(input.tolerance),
            positional_uncertainty: None,
            confidence: None,
        },
        provenance: Provenance {
            source: snapshot.identity().clone(),
            input: call.input,
            engine_revision: Id::new(ENGINE_REVISION),
        },
        timing: Timing { render_ms, encoding_ms },
    };
    cx.publish_control(|| result)
}

fn camera(camera: &Camera) -> Result<View, Error> {
    match camera {
        Camera::Named(name) => krly3d_view::named_view(name.as_str()),
        Camera::Orthographic { toward, up } => {
            Ok(View { toward: Vec3::from(*toward), up: Vec3::from(*up), frame: Frame::Fit })
        }
        Camera::Pinhole { eye, toward, up, focal, centre } => Ok(View {
            toward: Vec3::from(*toward),
            up: Vec3::from(*up),
            frame: Frame::Camera {
                eye: Point3::from(*eye),
                focal: *focal,
                centre: centre.map(|[x, y]| (x, y)),
            },
        }),
    }
}

fn tally(cx: &Context, image: &Image, body: &Body) -> Result<Vec<FaceCoverage>, Error> {
    let mut counts = BTreeMap::<&str, u64>::new();
    for batch in image.seen.chunks(1024) {
        cx.check_control()?;
        for seen in batch.iter().flatten() {
            let face = body.faces.get(seen.face).filter(|_| seen.body == 0).ok_or_else(|| {
                Error::geometry("render visibility does not identify a face of the source body")
            })?;
            *counts.entry(&face.name).or_default() += 1;
        }
    }
    let mut faces = Vec::with_capacity(counts.len());
    for (index, (name, pixels)) in counts.into_iter().enumerate() {
        if index % 64 == 0 {
            cx.check_control()?;
        }
        faces.push(FaceCoverage { name: name.to_string(), pixels });
    }
    cx.check_control()?;
    Ok(faces)
}

fn legacy_diagnostic(error: Error) -> Diagnostic {
    diagnostic(error.kind.label(), error.message)
}

/// Detached pinned computation; no mutable owner crosses the worker boundary.
pub(crate) struct RenderTask {
    prepared: PreparedRender,
    context: Context,
}

impl RenderTask {
    pub(crate) fn provenance(&self) -> Provenance {
        Provenance {
            source: self.prepared.snapshot.identity().clone(),
            input: self.prepared.call.input.clone(),
            engine_revision: Id::new(ENGINE_REVISION),
        }
    }
    pub(crate) fn execute(self) -> Result<RenderResult, Diagnostic> {
        render_prepared(self.prepared.snapshot, self.prepared.call, &self.context).map_err(legacy_diagnostic)
    }
}
