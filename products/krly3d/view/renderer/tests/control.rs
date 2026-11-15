//! Stop real work at known stages, without sleeps or large timing-dependent fixtures.
#![allow(clippy::unwrap_used)]

use krly3d_brep_engine::BrepEngine;
use krly3d_core::{CallControl, Clock};
use krly3d_kernel::model::Statement;
use krly3d_kernel::schema::Params;
use krly3d_view_api::{Backend, RenderCall, RenderInput, RequestId};
use krly3d_view_renderer::ViewEngine;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

// Only timing uses the source context clock. The four reads delimit rendering
// and encoding, so a separate deadline clock can observe the stage directly.
struct StageClock {
    reads: AtomicUsize,
    millis: [u64; 4],
}

impl StageClock {
    fn new(millis: [u64; 4]) -> Arc<Self> {
        Arc::new(Self { reads: AtomicUsize::new(0), millis })
    }

    fn reset(&self) {
        self.reads.store(0, Ordering::SeqCst);
    }
}

impl Clock for StageClock {
    fn now(&self) -> Duration {
        let index = self.reads.fetch_add(1, Ordering::SeqCst);
        Duration::from_millis(self.millis[index.min(3)])
    }
}

#[derive(Clone, Copy)]
enum Stop {
    Cancel,
    Deadline,
}

struct StopClock {
    stage: Arc<StageClock>,
    target: usize,
    after: usize,
    checks: AtomicUsize,
    fired: AtomicBool,
    stop: Stop,
    control: Mutex<Weak<CallControl>>,
}

impl StopClock {
    fn control(
        stage: &Arc<StageClock>,
        target: usize,
        after: usize,
        stop: Stop,
    ) -> (Arc<Self>, Arc<CallControl>) {
        let clock = Arc::new(Self {
            stage: Arc::clone(stage),
            target,
            after,
            checks: AtomicUsize::new(0),
            fired: AtomicBool::new(false),
            stop,
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
        {
            if let Stop::Cancel = self.stop {
                // Cancellation is used only inside PNG work (stage 3), where no
                // publication mutex is held. The next checkpoint observes it.
                self.control.lock().unwrap().upgrade().unwrap().cancel();
            }
        }
        if self.fired.load(Ordering::SeqCst) && matches!(self.stop, Stop::Deadline) {
            Duration::from_secs(1)
        } else {
            Duration::ZERO
        }
    }
}

fn engine(clock: &Arc<StageClock>) -> ViewEngine {
    let mut builder = krly3d_std::standard();
    builder.clock(clock.clone());
    let mut brep = BrepEngine::new(builder.build()).unwrap();
    brep.apply(
        Statement::new("part", "box", Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0), &[])
            .unwrap(),
    )
    .unwrap();
    clock.reset();
    ViewEngine::new(brep)
}

fn call(backend: Backend) -> RenderCall {
    let mut input = RenderInput::new("part");
    input.width = 80;
    input.height = 60;
    input.backend = backend;
    RenderCall::new(RequestId::new("controlled"), input)
}

#[test]
fn cancellation_and_deadline_interrupt_png_copying_and_checksums_then_retry_cleanly() {
    for backend in [Backend::Raycast, Backend::Mesh] {
        let stage = StageClock::new([10, 17, 100, 113]);
        let engine = engine(&stage);
        let (observer, control) = StopClock::control(&stage, 3, usize::MAX, Stop::Deadline);
        let before = engine.execute_controlled(engine.prepare(call(backend)).unwrap(), control).unwrap();
        let encoding_checks = observer.checks.load(Ordering::SeqCst);
        assert!(encoding_checks > 120, "fixture must exercise copying and checksums");
        assert_eq!(stage.reads.load(Ordering::SeqCst), 4);
        for after in [8, encoding_checks - 3, encoding_checks - 1] {
            for stop in [Stop::Cancel, Stop::Deadline] {
                stage.reset();
                let (clock, control) = StopClock::control(&stage, 3, after, stop);
                let error =
                    engine.execute_controlled(engine.prepare(call(backend)).unwrap(), control).unwrap_err();
                assert_eq!(
                    error.code.as_str(),
                    match stop {
                        Stop::Cancel => "cancelled",
                        Stop::Deadline => "deadline_exceeded",
                    }
                );
                assert!(clock.fired.load(Ordering::SeqCst));
                // The encoder never reached its ending timing read: no complete
                // file or face list escaped through a partial result.
                assert_eq!(stage.reads.load(Ordering::SeqCst), 3);
                stage.reset();
                let retry = engine
                    .execute_controlled(engine.prepare(call(backend)).unwrap(), Arc::new(CallControl::new()))
                    .unwrap();
                assert_eq!(retry.png, before.png);
                assert_eq!(retry.faces, before.faces);
                assert_eq!(retry.provenance, before.provenance);
            }
        }
    }
}

#[test]
fn deadlines_stop_real_render_work_face_tally_and_final_publication() {
    for backend in [Backend::Raycast, Backend::Mesh] {
        let stage = StageClock::new([10, 17, 100, 113]);
        let engine = engine(&stage);
        for (target, after) in [(1, 50), (2, 3), (4, 1)] {
            stage.reset();
            let (clock, control) = StopClock::control(&stage, target, after, Stop::Deadline);
            let error =
                engine.execute_controlled(engine.prepare(call(backend)).unwrap(), control).unwrap_err();
            assert_eq!(error.code.as_str(), "deadline_exceeded");
            assert!(clock.fired.load(Ordering::SeqCst));
            assert_eq!(stage.reads.load(Ordering::SeqCst), target);
            if target == 4 {
                // Both render and PNG have completed. Only guarded publication
                // can reject a deadline first observed at this boundary.
                assert_eq!(clock.checks.load(Ordering::SeqCst), 1);
            }
            stage.reset();
            let retry = engine.call(call(backend)).unwrap();
            assert!(!retry.png.is_empty());
            assert!(!retry.faces.is_empty());
        }
    }
}

#[test]
fn pre_cancelled_and_expired_controls_return_no_result_and_do_not_poison_the_owner() {
    for backend in [Backend::Raycast, Backend::Mesh] {
        let stage = StageClock::new([10, 17, 100, 113]);
        let engine = engine(&stage);
        let cancelled = Arc::new(CallControl::new());
        cancelled.cancel();
        let error = engine.execute_controlled(engine.prepare(call(backend)).unwrap(), cancelled).unwrap_err();
        assert_eq!(error.code.as_str(), "cancelled");
        assert_eq!(stage.reads.load(Ordering::SeqCst), 0);
        let expired =
            Arc::new(CallControl::with_deadline(Arc::new(krly3d_core::FakeClock::default()), Duration::ZERO));
        let error = engine.execute_controlled(engine.prepare(call(backend)).unwrap(), expired).unwrap_err();
        assert_eq!(error.code.as_str(), "deadline_exceeded");
        assert_eq!(stage.reads.load(Ordering::SeqCst), 0);
        assert!(engine.call(call(backend)).is_ok());
    }
}

#[test]
fn source_clock_times_rendering_and_encoding_separately_and_saturates_backwards_deltas() {
    for backend in [Backend::Raycast, Backend::Mesh] {
        for (times, expected) in [([10, 17, 100, 113], (7.0, 13.0)), ([20, 10, 40, 30], (0.0, 0.0))] {
            let stage = StageClock::new(times);
            let engine = engine(&stage);
            let result = engine.call(call(backend)).unwrap();
            assert_eq!((result.timing.render_ms, result.timing.encoding_ms), expected);
            assert_eq!(stage.reads.load(Ordering::SeqCst), 4);
        }
    }
}
