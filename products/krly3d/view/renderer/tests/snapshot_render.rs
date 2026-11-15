#![allow(clippy::unwrap_used)]

use krly3d_brep_engine::{BrepEngine, Snapshot};
use krly3d_core::{CallControl, Clock, FakeClock};
use krly3d_kernel::model::Statement;
use krly3d_kernel::schema::Params;
use krly3d_view_api::{
    ApiVersion, Backend, Camera, Diagnostic, LengthUnit, NamedView, RenderCall, RenderInput, RenderResult,
    RequestId,
};
use krly3d_view_renderer::{render_snapshot, render_snapshot_controlled, ViewEngine};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

// Four source-clock reads bracket rendering and PNG encoding. Separate deadline
// clocks can stop real work at these stages without wall-clock races or sleeps.
#[derive(Default)]
struct StageClock {
    reads: AtomicUsize,
    cancel_at_end: Mutex<Weak<CallControl>>,
}

impl StageClock {
    fn reset(&self) {
        self.reads.store(0, Ordering::SeqCst);
        *self.cancel_at_end.lock().unwrap() = Weak::new();
    }
}

impl Clock for StageClock {
    fn now(&self) -> Duration {
        let index = self.reads.fetch_add(1, Ordering::SeqCst);
        if index == 3 {
            if let Some(control) = self.cancel_at_end.lock().unwrap().upgrade() {
                // The ending timing read precedes the final publication gate.
                control.cancel();
            }
        }
        Duration::from_millis([10, 17, 100, 113][index.min(3)])
    }
}

fn block(width: f64) -> Statement {
    Statement::new("part", "box", Params::new().with("w", width).with("d", 4.0).with("h", 2.0), &[]).unwrap()
}

fn engine(stage: &Arc<StageClock>) -> ViewEngine {
    let mut builder = krly3d_std::standard();
    builder.clock(stage.clone());
    let mut brep = BrepEngine::new(builder.build()).unwrap();
    brep.apply(block(6.0)).unwrap();
    stage.reset();
    ViewEngine::new(brep)
}

fn call(backend: Backend) -> RenderCall {
    let mut input = RenderInput::new("part");
    input.width = 80;
    input.height = 60;
    input.backend = backend;
    RenderCall::new(RequestId::new("snapshot-render"), input)
}

fn render(snapshot: Snapshot, call: RenderCall, controlled: bool) -> Result<RenderResult, Diagnostic> {
    if controlled {
        render_snapshot_controlled(snapshot, call, Arc::new(CallControl::new()))
    } else {
        render_snapshot(snapshot, call)
    }
}

fn cameras() -> Vec<Camera> {
    let mut cameras: Vec<_> = [
        NamedView::Iso,
        NamedView::Front,
        NamedView::Back,
        NamedView::Left,
        NamedView::Right,
        NamedView::Top,
        NamedView::Bottom,
    ]
    .into_iter()
    .map(Camera::Named)
    .collect();
    cameras.push(Camera::Orthographic { toward: [2.0, -3.0, -1.0], up: [0.0, 0.0, 1.0] });
    for centre in [None, Some([35.25, 20.75])] {
        cameras.push(Camera::Pinhole {
            eye: [10.0, -14.0, 9.0],
            toward: [-10.0, 14.0, -8.0],
            up: [0.0, 0.0, 1.0],
            focal: 70.0,
            centre,
        });
    }
    cameras
}

#[test]
fn both_snapshot_entry_points_match_existing_png_and_all_metadata_for_every_camera() {
    let stage = Arc::new(StageClock::default());
    let engine = engine(&stage);
    let snapshot = engine.brep().snapshot("part").unwrap();
    for backend in [Backend::Raycast, Backend::Mesh] {
        for camera in cameras() {
            let mut request = call(backend);
            request.input.camera = camera;
            stage.reset();
            let expected = engine.call(request.clone()).unwrap();
            assert!(!expected.png.is_empty());
            assert!(!expected.faces.is_empty());
            for controlled in [false, true] {
                for node in ["part", snapshot.identity().node.as_str()] {
                    stage.reset();
                    request.input.node = node.into();
                    let actual = render(snapshot.clone(), request.clone(), controlled).unwrap();
                    // Injected source time makes even timing metadata comparable.
                    assert_eq!(actual, expected);
                    assert_eq!(stage.reads.load(Ordering::SeqCst), 4);
                }
            }
        }
    }
}

#[test]
fn retained_versions_survive_edits_rollback_and_owner_drop_on_another_thread() {
    for backend in [Backend::Raycast, Backend::Mesh] {
        let stage = Arc::new(StageClock::default());
        let mut engine = engine(&stage);
        let first = engine.brep().snapshot("part").unwrap();
        let expected_first = engine.call(call(backend)).unwrap();
        let checkpoint = engine.brep().checkpoint();
        engine.brep_mut().apply(block(12.0)).unwrap();
        let second = engine.brep().snapshot("part").unwrap();
        stage.reset();
        let expected_second = engine.call(call(backend)).unwrap();
        assert_ne!(expected_first.png, expected_second.png);
        engine.brep_mut().rollback(checkpoint);
        engine.brep_mut().apply(block(3.0)).unwrap();
        let branch = engine.brep().snapshot("part").unwrap();
        assert_eq!(branch.identity().node, second.identity().node);
        assert_ne!(branch.identity().source, second.identity().source);
        drop(branch);
        drop(engine);
        std::thread::spawn(move || {
            for (snapshot, expected) in [(first, expected_first), (second, expected_second)] {
                for controlled in [false, true] {
                    stage.reset();
                    let actual = render(snapshot.clone(), call(backend), controlled).unwrap();
                    assert_eq!(actual, expected);
                }
            }
        })
        .join()
        .unwrap();
    }
}

#[test]
fn different_names_versions_and_noncanonical_references_are_rejected_before_rendering() {
    let stage = Arc::new(StageClock::default());
    let mut engine = engine(&stage);
    let first = engine.brep().snapshot("part").unwrap();
    engine.brep_mut().apply(block(12.0)).unwrap();
    let second = engine.brep().snapshot("part").unwrap();
    for (snapshot, wrong_version) in [(first, "part@2"), (second, "part@1")] {
        for backend in [Backend::Raycast, Backend::Mesh] {
            for controlled in [false, true] {
                for node in ["other", "other@1", wrong_version, "part@0", "part@0001", "part@two", "part#top"]
                {
                    stage.reset();
                    let mut request = call(backend);
                    request.input.node = node.into();
                    let error = render(snapshot.clone(), request, controlled).unwrap_err();
                    assert_eq!(error.code.as_str(), "input");
                    assert_eq!(stage.reads.load(Ordering::SeqCst), 0);
                }
                stage.reset();
                let mut request = call(backend);
                request.input.node = snapshot.identity().node.clone();
                let result = render(snapshot.clone(), request, controlled).unwrap();
                assert_eq!(&result.provenance.source, snapshot.identity());
                assert_eq!(result.provenance.input.node, snapshot.identity().node);
            }
        }
    }
}

#[test]
fn snapshot_calls_keep_typed_validation_and_source_mesh_tolerance_checks() {
    let stage = Arc::new(StageClock::default());
    let engine = engine(&stage);
    let snapshot = engine.brep().snapshot("part").unwrap();
    for backend in [Backend::Raycast, Backend::Mesh] {
        let valid = call(backend);
        let mut invalid = Vec::new();
        let mut request = valid.clone();
        request.api = ApiVersion { major: 1, minor: 1 };
        invalid.push(request);
        let mut request = valid.clone();
        request.request_id = RequestId::new("");
        invalid.push(request);
        let mut request = valid.clone();
        request.input.node.clear();
        invalid.push(request);
        for dimensions in [(0, 60), (80, 0), (2049, 60), (80, 2049), (u32::MAX, u32::MAX)] {
            let mut request = valid.clone();
            (request.input.width, request.input.height) = dimensions;
            invalid.push(request);
        }
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut request = valid.clone();
            request.input.tolerance.value = value;
            invalid.push(request);
        }
        let mut request = valid.clone();
        request.input.tolerance.unit = LengthUnit::Metre;
        invalid.push(request);
        let mut request = valid.clone();
        request.input.camera = Camera::Orthographic { toward: [0.0; 3], up: [0.0, 0.0, 1.0] };
        invalid.push(request);
        for request in invalid {
            let expected = request.validate().unwrap_err();
            for controlled in [false, true] {
                stage.reset();
                assert_eq!(render(snapshot.clone(), request.clone(), controlled).unwrap_err(), expected);
                assert_eq!(stage.reads.load(Ordering::SeqCst), 0);
            }
        }
        for factor in [0.5, 1.0] {
            let mut request = valid.clone();
            request.input.tolerance.value = snapshot.legacy_context().accuracy().length() * factor;
            for controlled in [false, true] {
                stage.reset();
                let actual = render(snapshot.clone(), request.clone(), controlled);
                if backend == Backend::Mesh {
                    assert_eq!(actual.unwrap_err(), engine.prepare(request.clone()).unwrap_err());
                    assert_eq!(stage.reads.load(Ordering::SeqCst), 0);
                } else {
                    assert_eq!(actual.unwrap().quality.requested_tolerance, None);
                }
            }
        }
    }
}

struct StopClock {
    stage: Arc<StageClock>,
    target: usize,
    after: usize,
    checks: AtomicUsize,
    fired: AtomicBool,
    cancel: bool,
    control: Mutex<Weak<CallControl>>,
}

impl StopClock {
    fn control(
        stage: &Arc<StageClock>,
        target: usize,
        after: usize,
        cancel: bool,
    ) -> (Arc<Self>, Arc<CallControl>) {
        let clock = Arc::new(Self {
            stage: Arc::clone(stage),
            target,
            after,
            checks: AtomicUsize::new(0),
            fired: AtomicBool::new(false),
            cancel,
            control: Mutex::new(Weak::new()),
        });
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
        *clock.control.lock().unwrap() = Arc::downgrade(&control);
        (clock, control)
    }
}

impl Clock for StopClock {
    fn now(&self) -> Duration {
        if self.stage.reads.load(Ordering::SeqCst) == self.target
            && self.checks.fetch_add(1, Ordering::SeqCst) + 1 >= self.after
            && !self.fired.swap(true, Ordering::SeqCst)
            && self.cancel
        {
            // Used only in PNG work, outside any publication gate.
            self.control.lock().unwrap().upgrade().unwrap().cancel();
        }
        if self.fired.load(Ordering::SeqCst) && !self.cancel {
            Duration::from_secs(1)
        } else {
            Duration::ZERO
        }
    }
}

#[test]
fn pre_cancelled_and_expired_snapshot_calls_do_no_work_and_controls_stay_local() {
    for backend in [Backend::Raycast, Backend::Mesh] {
        let stage = Arc::new(StageClock::default());
        let engine = engine(&stage);
        let snapshot = engine.brep().snapshot("part").unwrap();
        let cancelled = Arc::new(CallControl::new());
        cancelled.cancel();
        let expired = Arc::new(CallControl::with_deadline(Arc::new(FakeClock::default()), Duration::ZERO));
        for (control, code) in [(cancelled, "cancelled"), (expired, "deadline_exceeded")] {
            let error = render_snapshot_controlled(snapshot.clone(), call(backend), control).unwrap_err();
            assert_eq!(error.code.as_str(), code);
            assert_eq!(stage.reads.load(Ordering::SeqCst), 0);
        }
        let expected = engine.call(call(backend)).unwrap();
        stage.reset();
        assert_eq!(render_snapshot(snapshot, call(backend)).unwrap(), expected);
    }
}

#[test]
fn retained_snapshot_controls_interrupt_work_encoding_and_final_publication_then_retry() {
    for backend in [Backend::Raycast, Backend::Mesh] {
        let stage = Arc::new(StageClock::default());
        let engine = engine(&stage);
        let snapshot = engine.brep().snapshot("part").unwrap();
        let expected = engine.call(call(backend)).unwrap();
        drop(engine);
        // Deadline stops cover rendering, face tally, PNG and final publication.
        // Cancellation in PNG exercises a stop requested after work has begun.
        for (target, after, cancel) in
            [(1, 50, false), (2, 3, false), (3, 8, false), (3, 8, true), (4, 1, false)]
        {
            stage.reset();
            let (clock, control) = StopClock::control(&stage, target, after, cancel);
            let error = render_snapshot_controlled(snapshot.clone(), call(backend), control).unwrap_err();
            assert_eq!(error.code.as_str(), if cancel { "cancelled" } else { "deadline_exceeded" });
            assert!(clock.fired.load(Ordering::SeqCst));
            assert_eq!(stage.reads.load(Ordering::SeqCst), target);
            if target == 4 {
                // All image work and encoding finished; the publication check
                // itself must observe the deadline and refuse a complete result.
                assert_eq!(clock.checks.load(Ordering::SeqCst), 1);
            }
            stage.reset();
            let retry =
                render_snapshot_controlled(snapshot.clone(), call(backend), Arc::new(CallControl::new()));
            assert_eq!(retry.unwrap(), expected);
        }
        stage.reset();
        let control = Arc::new(CallControl::new());
        *stage.cancel_at_end.lock().unwrap() = Arc::downgrade(&control);
        let error = render_snapshot_controlled(snapshot.clone(), call(backend), control).unwrap_err();
        assert_eq!(error.code.as_str(), "cancelled");
        assert_eq!(stage.reads.load(Ordering::SeqCst), 4);
        stage.reset();
        assert_eq!(render_snapshot(snapshot, call(backend)).unwrap(), expected);
    }
}
