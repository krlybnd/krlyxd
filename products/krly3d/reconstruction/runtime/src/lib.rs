//! A bounded, single-process reconstruction host. Domains own geometry; this
//! layer owns revisions, dependencies, admission and publication. Workers use
//! immutable handles and never borrow or lock the mutable host.

use krly2d_api::Region;
use krly3d_brep::planar::{PlanarQuality, MAX_HEIGHT};
use krly3d_brep_engine::{planar::build_region_retained_controlled, Snapshot};
use krly3d_mesh::{MeshCall, MeshInput, MeshResult, RequestId};
use krly3d_mesh_engine::MeshEngine;
use krlyvis_api::DrawingCall;
use krlyvis_engine::{read_shared_controlled, SharedDrawing};
use krlyxd::{CallControl, CallStop, Diagnostic, Id};

mod coarse;
mod frame;
mod review;
pub mod sheet;
pub use coarse::{CoarsePreview, CoarseReady};
pub use frame::{centered_top_view, ConstructionFrame};
pub use review::Review;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub const MAX_IN_FLIGHT: usize = 4;
pub const EVENT_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub height: f64,
    pub tolerance: f64,
}

impl Settings {
    pub fn validate(self) -> Result<(), Diagnostic> {
        if !self.height.is_finite() || self.height <= 0.0 || self.height > MAX_HEIGHT {
            return Err(error("input", "height must be positive, finite and at most 1e9 model units"));
        }
        let mut input = MeshInput::new("part@1");
        input.tolerance.value = self.tolerance;
        input.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Drawing,
    Height,
    MeshTolerance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// Accepted command; dependent results cease to be current immediately.
    Invalidated(Change),
    /// All required results published atomically for this revision.
    Committed,
    /// Explicit bounding-prism approximation, before contour construction.
    CoarsePreview,
}

/// A compact notification, not a durable command log or an AI callback. Sequence
/// is local to this host and follows accepted commands/publication, not wall time.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub sequence: u64,
    pub revision: u64,
    pub kind: EventKind,
    pub settings: Settings,
}

/// Retained domain owner plus its immutable snapshot. No native-document round trip.
pub struct Model {
    engine: MeshEngine,
    snapshot: Snapshot,
    region: Arc<Region>,
    frame: ConstructionFrame,
    height: f64,
    quality: PlanarQuality,
}

impl Model {
    /// Shared input profile. Its coordinates carry no inferred physical scale.
    pub fn region(&self) -> &Arc<Region> {
        &self.region
    }

    pub fn frame(&self) -> ConstructionFrame {
        self.frame
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    pub fn quality(&self) -> &PlanarQuality {
        &self.quality
    }

    /// Explicit output boundary. Internal consumers never need this serialization.
    pub fn to_k3d(&self) -> Result<String, Diagnostic> {
        self.engine.brep().to_k3d()
    }
}

pub struct Reconstruction {
    revision: u64,
    settings: Settings,
    evidence: Arc<SharedDrawing>,
    model: Arc<Model>,
    mesh: Arc<MeshResult>,
    review: Review,
}

impl Reconstruction {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn settings(&self) -> Settings {
        self.settings
    }
    pub fn evidence(&self) -> &Arc<SharedDrawing> {
        &self.evidence
    }
    pub fn model(&self) -> &Arc<Model> {
        &self.model
    }
    pub fn review(&self) -> &Review {
        &self.review
    }
    pub fn mesh(&self) -> &Arc<MeshResult> {
        &self.mesh
    }
}

/// Each owned job/result occupies one slot until published or dropped. Dropping
/// the host does not destroy data pinned by a worker or reader.
struct Permit(Arc<AtomicUsize>);

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct Job {
    host: Arc<()>,
    permit: Permit,
    revision: u64,
    settings: Settings,
    evidence: Arc<SharedDrawing>,
    reusable: Option<Arc<Model>>,
    control: Arc<CallControl>,
}

pub struct Completed {
    host: Arc<()>,
    _permit: Permit,
    result: Reconstruction,
    control: Arc<CallControl>,
}

impl Job {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn evidence(&self) -> &Arc<SharedDrawing> {
        &self.evidence
    }

    /// Run on the caller's thread or a caller-owned worker. No host mutex is held.
    /// Failure never publishes a partial result or restores an invalidated one.
    pub fn run(self) -> Result<Completed, Diagnostic> {
        check(&self.control)?;
        let model = match self.reusable {
            Some(model) if model.height == self.settings.height => model,
            previous => {
                // Height edits retain the same centered profile and source frame.
                let (region, frame) = match previous {
                    Some(model) => (Arc::clone(&model.region), model.frame),
                    None => frame::centered_top_view(self.evidence.region())?,
                };
                let retained = build_region_retained_controlled(
                    Arc::clone(&region),
                    self.settings.height,
                    format!("reconstruction:{}", self.revision),
                    format!(
                        "sha256:{}:drawing-revision:{}",
                        self.evidence.provenance().image_sha256,
                        self.revision
                    ),
                    Arc::clone(&self.control),
                )?;
                Arc::new(Model {
                    engine: MeshEngine::from_brep(retained.engine),
                    snapshot: retained.snapshot,
                    region: retained.region,
                    frame,
                    height: self.settings.height,
                    quality: retained.quality,
                })
            }
        };
        let mut input = MeshInput::new(model.snapshot.identity().node.clone());
        input.triangles = true;
        input.tolerance.value = self.settings.tolerance;
        check(&self.control)?;
        let prepared = model.engine.prepare(MeshCall::new(RequestId::new("reconstruction"), input))?;
        let mesh = model.engine.execute_controlled(prepared, Arc::clone(&self.control))?;
        let expected_volume = self.evidence.material_area() * self.settings.height;
        if mesh.open != 0
            || !mesh.volume.is_finite()
            || (mesh.volume - expected_volume).abs() > 1e-8_f64.max(expected_volume.abs() * 1e-6)
        {
            return Err(error("geometry", "mesh volume/edge balance disagrees with planar evidence"));
        }
        Ok(Completed {
            host: self.host,
            _permit: self.permit,
            control: self.control,
            result: Reconstruction {
                revision: self.revision,
                settings: self.settings,
                evidence: self.evidence,
                model,
                mesh: Arc::new(mesh),
                review: Review::default(),
            },
        })
    }
}

/// One drawing document, fixed evidence → B-rep → mesh dependency chain. This is
/// an application host, not the CTM domain or a universal heterogeneous store.
pub struct Host {
    identity: Arc<()>,
    in_flight: Arc<AtomicUsize>,
    revision: u64,
    sequence: u64,
    evidence: Option<Arc<SharedDrawing>>,
    settings: Option<Settings>,
    reusable: Option<Arc<Model>>,
    current: Option<Arc<Reconstruction>>,
    coarse: Option<Arc<CoarsePreview>>,
    events: VecDeque<Event>,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            identity: Arc::new(()),
            in_flight: Arc::new(AtomicUsize::new(0)),
            revision: 0,
            sequence: 0,
            evidence: None,
            settings: None,
            reusable: None,
            current: None,
            coarse: None,
            events: VecDeque::new(),
        }
    }
}

impl Host {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn current(&self) -> Option<&Arc<Reconstruction>> {
        self.current.as_ref()
    }
    /// Latest explicit approximation for the pending revision, if published.
    pub fn coarse_preview(&self) -> Option<&Arc<CoarsePreview>> {
        self.coarse.as_ref()
    }
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    /// Image decoding is synchronous in this first host. Validation/decoding
    /// failure leaves the previous accepted command and result untouched.
    pub fn replace_drawing(&mut self, call: DrawingCall, settings: Settings) -> Result<Job, Diagnostic> {
        self.replace_drawing_controlled(call, settings, Arc::new(CallControl::new()))
    }

    /// One shared cancellation/deadline token for all native stages, including
    /// contour and planar topology loops. Individual codec/provider calls may
    /// still be synchronous between their cooperative checkpoints.
    pub fn replace_drawing_controlled(
        &mut self,
        call: DrawingCall,
        settings: Settings,
        control: Arc<CallControl>,
    ) -> Result<Job, Diagnostic> {
        check(&control)?;
        settings.validate()?;
        let permit = self.admit()?;
        let evidence = Arc::new(read_shared_controlled(call, Arc::clone(&control))?);
        control
            .publish(|| self.begin(evidence, settings, None, Change::Drawing, permit, Arc::clone(&control)))
            .map_err(stop)?
    }

    /// Sheet composition has already selected validated native evidence from its
    /// shared image owner. No encoded-image copy or second decode is needed here.
    fn replace_evidence_controlled(
        &mut self,
        evidence: Arc<SharedDrawing>,
        settings: Settings,
        control: Arc<CallControl>,
    ) -> Result<Job, Diagnostic> {
        check(&control)?;
        settings.validate()?;
        let permit = self.admit()?;
        control
            .publish(|| self.begin(evidence, settings, None, Change::Drawing, permit, Arc::clone(&control)))
            .map_err(stop)?
    }

    /// Height changes keep evidence; tolerance-only changes also keep a completed
    /// B-rep. A job can outlive later edits, but its result then fails publication.
    pub fn revise(&mut self, settings: Settings) -> Result<Job, Diagnostic> {
        self.revise_controlled(settings, Arc::new(CallControl::new()))
    }

    /// Apply an edit with caller-owned cancellation/deadline control. A stopped
    /// command cannot invalidate the currently published revision. After admission,
    /// the same token guards computation, review and final publication.
    pub fn revise_controlled(
        &mut self,
        settings: Settings,
        control: Arc<CallControl>,
    ) -> Result<Job, Diagnostic> {
        check(&control)?;
        settings.validate()?;
        let evidence = self.evidence.as_ref().ok_or_else(|| error("input", "no drawing loaded"))?;
        let evidence = Arc::clone(evidence);
        let unchanged_height = self.settings.is_some_and(|old| old.height == settings.height);
        let reusable = self.reusable.clone();
        let change = if unchanged_height { Change::MeshTolerance } else { Change::Height };
        let permit = self.admit()?;
        control
            .publish(|| self.begin(evidence, settings, reusable, change, permit, Arc::clone(&control)))
            .map_err(stop)?
    }

    /// For proposals based on an observed revision. A delayed agent must refresh
    /// its inputs before it can change newer user state.
    pub fn revise_if_current(&mut self, base_revision: u64, settings: Settings) -> Result<Job, Diagnostic> {
        self.revise_if_current_controlled(base_revision, settings, Arc::new(CallControl::new()))
    }

    /// Revision-guarded proposals use the same control semantics as direct edits.
    /// A stale base is rejected before admission even when its control is stopped.
    pub fn revise_if_current_controlled(
        &mut self,
        base_revision: u64,
        settings: Settings,
        control: Arc<CallControl>,
    ) -> Result<Job, Diagnostic> {
        if base_revision != self.revision {
            return Err(error("stale_revision", "proposal was based on an older host revision"));
        }
        self.revise_controlled(settings, control)
    }

    fn admit(&self) -> Result<Permit, Diagnostic> {
        self.in_flight
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_IN_FLIGHT).then_some(count + 1)
            })
            .map_err(|_| error("resource_limit", "host has four outstanding jobs/results"))?;
        Ok(Permit(Arc::clone(&self.in_flight)))
    }

    fn begin(
        &mut self,
        evidence: Arc<SharedDrawing>,
        settings: Settings,
        reusable: Option<Arc<Model>>,
        change: Change,
        permit: Permit,
        control: Arc<CallControl>,
    ) -> Result<Job, Diagnostic> {
        let revision =
            self.revision.checked_add(1).ok_or_else(|| error("resource_limit", "revision exhausted"))?;
        // Reserve room for this command and its eventual publication before mutation.
        self.sequence.checked_add(3).ok_or_else(|| error("resource_limit", "event sequence exhausted"))?;
        self.revision = revision;
        self.evidence = Some(Arc::clone(&evidence));
        self.settings = Some(settings);
        self.reusable = reusable.clone();
        self.current = None;
        self.coarse = None;
        self.emit(EventKind::Invalidated(change), settings);
        Ok(Job { host: Arc::clone(&self.identity), permit, revision, settings, evidence, reusable, control })
    }

    pub fn publish(&mut self, completed: Completed) -> Result<Arc<Reconstruction>, Diagnostic> {
        if !Arc::ptr_eq(&self.identity, &completed.host) {
            return Err(error("foreign_host", "result belongs to another host"));
        }
        if completed.result.revision != self.revision || self.current.is_some() {
            return Err(error("stale_revision", "result no longer matches the requested revision"));
        }
        if completed.result.review().preview().is_some() != completed.result.review().likeness().is_some() {
            return Err(error("stage", "a started review must finish before publication"));
        }
        completed
            .control
            .publish(|| {
                let result = Arc::new(completed.result);
                self.coarse = None;
                self.reusable = Some(Arc::clone(&result.model));
                self.current = Some(Arc::clone(&result));
                self.emit(EventKind::Committed, result.settings);
                result
            })
            .map_err(stop)
    }

    fn emit(&mut self, kind: EventKind, settings: Settings) {
        self.sequence += 1; // reserved at admission: invalidation + optional coarse + final
        if self.events.len() == EVENT_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(Event { sequence: self.sequence, revision: self.revision, kind, settings });
    }

    /// Readers must resnapshot after lag, never silently miss a dependency change.
    /// Cursor 0 starts at the first event. No retained payloads or subscriber queues.
    pub fn events_after(&self, cursor: u64) -> Result<Vec<Event>, Diagnostic> {
        if cursor > self.sequence {
            return Err(error("input", "event cursor is ahead of this host"));
        }
        if self.events.front().is_some_and(|event| cursor < event.sequence - 1) {
            return Err(error("event_gap", "event history expired; read the current snapshot"));
        }
        Ok(self.events.iter().filter(|event| event.sequence > cursor).cloned().collect())
    }
}

fn error(code: &str, message: &str) -> Diagnostic {
    Diagnostic { code: Id::new(code), message: message.into() }
}

fn check(control: &CallControl) -> Result<(), Diagnostic> {
    control.check().map_err(stop)
}
fn stop(reason: CallStop) -> Diagnostic {
    match reason {
        CallStop::Cancelled => error("cancelled", "reconstruction cancelled"),
        CallStop::DeadlineExceeded => error("deadline_exceeded", "reconstruction deadline exceeded"),
    }
}
