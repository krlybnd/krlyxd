#![allow(clippy::unwrap_used, clippy::panic)]
use krly3d_view_api::{
    operation::{OperationCall, RenderEventBody},
    Backend, RenderCall, RenderInput, RequestId,
};
use krly3d_view_renderer::{
    runner::{RunnerLimits, ViewRunner},
    ViewEngine,
};
use krlyxd::{Clock, FakeClock, OperationId};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
const MODEL: &str = "k3d 1\nlayer geo\npart = box w=6 d=4 h=2\n";
fn call(id: &str) -> OperationCall {
    let mut input = RenderInput::new("part");
    input.width = 32;
    input.height = 24;
    OperationCall::new(RequestId::new(id), input)
}
fn release(r: &ViewRunner, id: &OperationId) {
    let end = Instant::now() + Duration::from_secs(10);
    while r.release(id).is_err() {
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
}
#[test]
fn detached_snapshot_results_match_sync_for_both_backends_and_release_reserves_ids() {
    for backend in [Backend::Raycast, Backend::Mesh] {
        let engine = ViewEngine::from_k3d(MODEL).unwrap();
        let mut call = call("parity");
        call.input.backend = backend;
        let expected = engine.call(RenderCall::new(call.request_id.clone(), call.input.clone())).unwrap();
        let runner = ViewRunner::standard();
        let id = runner.start(&engine, call.clone()).unwrap();
        drop(engine);
        let started = runner.wait_next(&id).unwrap().unwrap();
        assert_eq!(started.seq, 0);
        let RenderEventBody::Started(provenance) = started.body else { panic!() };
        assert_eq!(provenance.source.node, "part@1");
        let terminal = runner.wait_next(&id).unwrap().unwrap();
        assert_eq!(terminal.seq, 1);
        let RenderEventBody::Completed(result) = terminal.body else { panic!("{:?}", terminal) };
        assert_eq!(result.png, expected.png);
        assert_eq!(result.faces, expected.faces);
        assert_eq!(result.quality, expected.quality);
        assert_eq!(result.provenance, provenance);
        assert!(runner.next(&id).unwrap().is_none());
        runner.cancel(&id).unwrap(); // Completed remains the sole terminal.
        let engine = ViewEngine::from_k3d(MODEL).unwrap();
        assert!(runner.start(&engine, call.clone()).is_err());
        release(&runner, &id);
        assert!(runner.next(&id).is_err());
        assert!(runner.start(&engine, call).is_ok());
    }
}
#[test]
fn zero_deadline_and_retained_byte_limit_produce_terminal_failure_and_allow_retry() {
    let engine = ViewEngine::from_k3d(MODEL).unwrap();
    let runner = ViewRunner::standard();
    let mut timed = call("deadline");
    timed.timeout_ms = Some(0);
    let id = runner.start(&engine, timed).unwrap();
    runner.wait_next(&id).unwrap();
    let RenderEventBody::Failed(error) = runner.wait_next(&id).unwrap().unwrap().body else { panic!() };
    assert_eq!(error.code.as_str(), "deadline_exceeded");
    release(&runner, &id);
    let runner = ViewRunner::new(
        RunnerLimits { max_queued_bytes: 2048, ..RunnerLimits::default() },
        Arc::new(FakeClock::default()),
    )
    .unwrap();
    let id = runner.start(&engine, call("large")).unwrap();
    runner.wait_next(&id).unwrap();
    let RenderEventBody::Failed(error) = runner.wait_next(&id).unwrap().unwrap().body else { panic!() };
    assert_eq!(error.code.as_str(), "resource_limit");
    release(&runner, &id);
    let mut small = call("large");
    small.input.width = 1;
    small.input.height = 1;
    let id = runner.start(&engine, small).unwrap();
    runner.wait_next(&id).unwrap();
    assert!(matches!(runner.wait_next(&id).unwrap().unwrap().body, RenderEventBody::Completed(_)));
}
struct GateClock {
    calls: AtomicUsize,
    entered: mpsc::Sender<()>,
    gate: (Mutex<bool>, Condvar),
}
impl Clock for GateClock {
    fn now(&self) -> Duration {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
            self.entered.send(()).unwrap();
            let (lock, cv) = &self.gate;
            let mut open = lock.lock().unwrap();
            while !*open {
                open = cv.wait(open).unwrap();
            }
        }
        Duration::ZERO
    }
}
impl GateClock {
    fn open(&self) {
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }
}
#[test]
fn cancellation_keeps_worker_permit_until_exit_and_publishes_only_one_terminal() {
    let (tx, rx) = mpsc::channel();
    let clock = Arc::new(GateClock {
        calls: AtomicUsize::new(0),
        entered: tx,
        gate: (Mutex::new(false), Condvar::new()),
    });
    let runner =
        ViewRunner::new(RunnerLimits { max_live_workers: 1, ..RunnerLimits::default() }, clock.clone())
            .unwrap();
    let engine = ViewEngine::from_k3d(MODEL).unwrap();
    let mut request = call("cancel");
    request.timeout_ms = Some(1000);
    let id = runner.start(&engine, request).unwrap();
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    runner.cancel(&id).unwrap();
    runner.cancel(&id).unwrap();
    assert!(matches!(runner.wait_next(&id).unwrap().unwrap().body, RenderEventBody::Started(_)));
    assert!(matches!(runner.wait_next(&id).unwrap().unwrap().body, RenderEventBody::Cancelled));
    assert!(runner.next(&id).unwrap().is_none());
    assert!(runner.release(&id).is_err());
    assert_eq!(runner.start(&engine, call("other")).unwrap_err().code.as_str(), "resource_limit");
    clock.open();
    release(&runner, &id);
    let id = runner.start(&engine, call("other")).unwrap();
    runner.wait_next(&id).unwrap();
    assert!(matches!(runner.wait_next(&id).unwrap().unwrap().body, RenderEventBody::Completed(_)));
}
#[test]
fn admission_rejects_invalid_input_and_retention_overflow_without_consuming_ids() {
    let engine = ViewEngine::from_k3d(MODEL).unwrap();
    let runner = ViewRunner::new(
        RunnerLimits { max_retained_operations: 1, ..RunnerLimits::default() },
        Arc::new(FakeClock::default()),
    )
    .unwrap();
    let mut invalid = call("same");
    invalid.input.node = "missing".into();
    assert!(runner.start(&engine, invalid).is_err());
    let id = runner.start(&engine, call("same")).unwrap();
    runner.wait_next(&id).unwrap();
    runner.wait_next(&id).unwrap();
    assert!(runner.start(&engine, call("second")).is_err());
    release(&runner, &id);
    assert!(runner.start(&engine, call("second")).is_ok());
    assert!(ViewRunner::new(
        RunnerLimits { max_queued_events: 1, ..RunnerLimits::default() },
        Arc::new(FakeClock::default())
    )
    .is_err());
}
