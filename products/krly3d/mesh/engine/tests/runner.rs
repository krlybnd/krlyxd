#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::{Accuracy, Attempt, Error, Exactness, Id};
use krly3d_kernel::model::Statement;
use krly3d_kernel::ports::interpret::{MeshRequest, ToMesh};
use krly3d_kernel::schema::Params;
use krly3d_kernel::{Context, KernelBuilder, Plugin};
use krly3d_mesh::operation::{encode_event, MeshEvent, MeshEventBody, OperationCall};
use krly3d_mesh::{MeshInput, MeshResult, RequestId};
use krly3d_mesh_engine::runner::{MeshRunner, RunnerLimits};
use krly3d_mesh_engine::MeshEngine;
use krlyxd::{Clock, FakeClock, OperationId};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::time::{Duration, Instant};

const WATCHDOG: Duration = Duration::from_secs(10);

fn block(width: f64) -> Statement {
    Statement::new("part", "box", Params::new().with("w", width).with("d", 4.0).with("h", 2.0), &[]).unwrap()
}

fn engine() -> MeshEngine {
    let mut builder = krly3d_std::standard();
    builder.accuracy(Accuracy::for_scale(100.0).unwrap());
    let mut kernel = builder.build();
    kernel.apply(block(6.0)).unwrap();
    MeshEngine::new(kernel).unwrap()
}

fn call(request: &str) -> OperationCall {
    OperationCall::new(RequestId::new(request), MeshInput::new("part"))
}

fn runner(limits: RunnerLimits) -> MeshRunner {
    MeshRunner::new(limits, Arc::new(FakeClock::default())).unwrap()
}

fn pair(runner: &MeshRunner, id: &OperationId) -> [MeshEvent; 2] {
    let started = runner.wait_next(id).unwrap().unwrap();
    let terminal = runner.wait_next(id).unwrap().unwrap();
    assert_eq!((started.seq, terminal.seq), (0, 1));
    assert_eq!((&started.operation_id, &terminal.operation_id), (id, id));
    assert_eq!(started.request_id, terminal.request_id);
    assert!(matches!(started.body, MeshEventBody::Started(_)));
    assert!(!matches!(terminal.body, MeshEventBody::Started(_)));
    assert_eq!(runner.next(id).unwrap(), None);
    assert_eq!(runner.wait_next(id).unwrap(), None);
    [started, terminal]
}

fn result(event: &MeshEvent) -> &MeshResult {
    let MeshEventBody::Completed(result) = &event.body else { panic!("expected completion: {event:?}") };
    result
}

fn failed(event: &MeshEvent, code: &str) {
    let MeshEventBody::Failed(error) = &event.body else { panic!("expected failure: {event:?}") };
    assert_eq!(error.code.as_str(), code);
}

// Terminal publication precedes the final few instructions of worker cleanup. Wait
// only for that lifecycle transition; all computation ordering uses channels/gates.
fn release(runner: &MeshRunner, id: &OperationId) {
    let until = Instant::now() + WATCHDOG;
    loop {
        match runner.release(id) {
            Ok(()) => return,
            Err(error) => assert_eq!(error.code.as_str(), "input"),
        }
        assert!(Instant::now() < until, "worker did not return its permit");
        std::thread::yield_now();
    }
}

struct Gate {
    entered: Sender<()>,
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn wait(&self) {
        let _ = self.entered.send(());
        let open = self.open.lock().unwrap();
        let (open, _) = self.changed.wait_timeout_while(open, WATCHDOG, |open| !*open).unwrap();
        assert!(*open, "test did not release the plugin gate");
    }

    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

#[derive(Clone, Copy)]
enum Behavior {
    Mesh,
    Panic,
    HugeError,
    Decline,
}

struct ControlledMesh {
    gate: Arc<Gate>,
    behavior: Behavior,
}

impl Plugin<ToMesh> for ControlledMesh {
    fn id(&self) -> Id {
        Id::new("brep")
    }

    fn call(&self, cx: &Context, request: &MeshRequest) -> Attempt<Arc<krly3d_kernel::mesh::Mesh>> {
        self.gate.wait();
        match self.behavior {
            Behavior::Mesh => krly3d_std::interpret::mesh::BrepMesh.call(cx, request),
            Behavior::Panic => panic!("injected plugin panic"),
            Behavior::HugeError => Attempt::Failed(Error::geometry("x".repeat(64 * 1024))),
            Behavior::Decline => Attempt::declined("fixture cannot mesh this shape"),
        }
    }
}

struct Fixture {
    engine: MeshEngine,
    gate: Arc<Gate>,
    entered: Receiver<()>,
}

impl Fixture {
    fn new(behavior: Behavior) -> Self {
        let (entered, receiver) = mpsc::channel();
        let gate = Arc::new(Gate { entered, open: Mutex::new(false), changed: Condvar::new() });
        let mut builder = KernelBuilder::new();
        builder.accuracy(Accuracy::for_scale(100.0).unwrap());
        builder.plugin::<ToMesh>(ControlledMesh { gate: Arc::clone(&gate), behavior });
        krly3d_std::sweep::register(&mut builder);
        krly3d_std::tool::register(&mut builder);
        krly3d_std::stage::register(&mut builder);
        let mut kernel = builder.build();
        kernel.apply(block(6.0)).unwrap();
        Self { engine: MeshEngine::new(kernel).unwrap(), gate, entered: receiver }
    }

    fn entered(&self) {
        self.entered.recv_timeout(WATCHDOG).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.gate.release();
    }
}

#[test]
fn real_box_events_preserve_geometry_artifact_provenance_and_canonical_encoding() {
    let engine = engine();
    let runner = runner(RunnerLimits::default());
    let mut request = call("box");
    request.input.triangles = true;
    let id = runner.start(&engine, request).unwrap();
    let [started, completed] = pair(&runner, &id);
    let result = result(&completed);
    assert_eq!((result.points, result.triangles, result.open), (8, 12, 0));
    assert_eq!((result.volume, result.area), (48.0, 88.0));
    assert_eq!(result.quality.exactness, Exactness::Sampled { samples: 12 });
    let mesh = result.mesh.as_ref().unwrap();
    assert_eq!((mesh.revision, mesh.data.points.len(), mesh.data.triangles.len()), (1, 8, 12));
    assert_eq!(started.body, MeshEventBody::Started(result.provenance.clone()));
    assert_eq!(result.provenance.input.node, "part@1");
    for event in [&started, &completed] {
        let encoded = encode_event(event).unwrap();
        assert!(encoded.contains("\"major\":2"));
        assert!(!encoded.contains('\n'));
    }
    runner.cancel(&id).unwrap();
    runner.cancel(&id).unwrap();
    assert_eq!(runner.next(&id).unwrap(), None);
}

#[test]
fn snapshot_remains_immutable_while_session_is_edited_and_rolled_back() {
    let mut fixture = Fixture::new(Behavior::Mesh);
    let runner = runner(RunnerLimits::default());
    let mark = fixture.engine.checkpoint();
    let id = runner.start(&fixture.engine, call("old")).unwrap();
    fixture.entered();
    let started = runner.next(&id).unwrap().unwrap();
    assert_eq!(runner.next(&id).unwrap(), None, "next must not wait on the plugin");
    fixture.engine.apply(block(8.0)).unwrap();
    fixture.engine.rollback(mark);
    fixture.engine.apply(block(10.0)).unwrap();
    fixture.gate.release();
    let completed = runner.wait_next(&id).unwrap().unwrap();
    let old = result(&completed);
    assert_eq!(old.volume, 48.0);
    assert_eq!(started.body, MeshEventBody::Started(old.provenance.clone()));
    let new_id = runner.start(&fixture.engine, call("new")).unwrap();
    let [_, completed] = pair(&runner, &new_id);
    let new = result(&completed);
    assert_eq!(new.volume, 80.0);
    assert_eq!(new.provenance.input.node, "part@2");
    assert_ne!(new.provenance.source, old.provenance.source);
    assert_eq!(runner.wait_next(&id).unwrap(), None);
}

#[test]
fn duplicate_request_ids_remain_reserved_until_explicit_release() {
    let engine = engine();
    let other = self::engine();
    let runner = runner(RunnerLimits::default());
    let id = runner.start(&engine, call("same")).unwrap();
    assert_eq!(runner.start(&other, call("same")).unwrap_err().code.as_str(), "input");
    pair(&runner, &id);
    assert_eq!(runner.start(&other, call("same")).unwrap_err().code.as_str(), "input");
    release(&runner, &id);
    for error in [
        runner.next(&id).unwrap_err(),
        runner.wait_next(&id).unwrap_err(),
        runner.cancel(&id).unwrap_err(),
        runner.release(&id).unwrap_err(),
    ] {
        assert_eq!(error.code.as_str(), "not_found");
    }
    let replacement = runner.start(&other, call("same")).unwrap();
    assert_ne!(id, replacement);
    pair(&runner, &replacement);
}

#[test]
fn retention_and_live_worker_limits_are_independent_and_rejections_do_not_leak() {
    let engine = engine();
    let runner =
        runner(RunnerLimits { max_live_workers: 1, max_retained_operations: 1, ..RunnerLimits::default() });
    let id = runner.start(&engine, call("one")).unwrap();
    pair(&runner, &id);
    assert_eq!(runner.start(&engine, call("two")).unwrap_err().code.as_str(), "resource_limit");
    release(&runner, &id);
    let second = runner.start(&engine, call("two")).unwrap();
    pair(&runner, &second);
}

#[test]
fn cancelled_but_computing_worker_keeps_its_permit_and_record() {
    let fixture = Fixture::new(Behavior::Mesh);
    let runner =
        runner(RunnerLimits { max_live_workers: 1, max_retained_operations: 3, ..RunnerLimits::default() });
    let id = runner.start(&fixture.engine, call("one")).unwrap();
    fixture.entered();
    assert_eq!(runner.release(&id).unwrap_err().code.as_str(), "input");
    runner.cancel(&id).unwrap();
    runner.cancel(&id).unwrap();
    let [_, cancelled] = pair(&runner, &id);
    assert_eq!(cancelled.body, MeshEventBody::Cancelled);
    assert_eq!(runner.release(&id).unwrap_err().code.as_str(), "input");
    assert_eq!(runner.start(&fixture.engine, call("two")).unwrap_err().code.as_str(), "resource_limit");
    fixture.gate.release();
    release(&runner, &id);
    let second = runner.start(&fixture.engine, call("two")).unwrap();
    let [_, completed] = pair(&runner, &second);
    assert_eq!(result(&completed).volume, 48.0);
}

#[test]
fn timeout_zero_is_accepted_then_fails_and_injected_deadline_stops_inflight_work() {
    let clock = Arc::new(FakeClock::default());
    let runner = MeshRunner::new(RunnerLimits::default(), clock.clone()).unwrap();
    let engine = engine();
    let mut request = call("zero");
    request.timeout_ms = Some(0);
    let id = runner.start(&engine, request).unwrap();
    let [_, terminal] = pair(&runner, &id);
    failed(&terminal, "deadline_exceeded");
    runner.cancel(&id).unwrap();
    assert_eq!(runner.next(&id).unwrap(), None);

    let fixture = Fixture::new(Behavior::Mesh);
    let mut request = call("later");
    request.timeout_ms = Some(5);
    let id = runner.start(&fixture.engine, request).unwrap();
    fixture.entered();
    clock.advance(Duration::from_millis(5));
    fixture.gate.release();
    let [_, terminal] = pair(&runner, &id);
    failed(&terminal, "deadline_exceeded");
}

#[test]
fn cancellation_wins_before_deadline_publication_and_wakes_waiters() {
    let fixture = Fixture::new(Behavior::Mesh);
    let clock = Arc::new(FakeClock::default());
    let runner = Arc::new(MeshRunner::new(RunnerLimits::default(), clock.clone()).unwrap());
    let mut request = call("cancel");
    request.timeout_ms = Some(1);
    let id = runner.start(&fixture.engine, request).unwrap();
    fixture.entered();
    runner.next(&id).unwrap().unwrap();
    let (send, recv) = mpsc::channel();
    let waiter = {
        let runner = Arc::clone(&runner);
        let id = id.clone();
        std::thread::spawn(move || send.send(runner.wait_next(&id).unwrap()).unwrap())
    };
    clock.advance(Duration::from_millis(1));
    runner.cancel(&id).unwrap();
    let terminal = recv.recv_timeout(WATCHDOG).unwrap().unwrap();
    assert_eq!(terminal.body, MeshEventBody::Cancelled);
    waiter.join().unwrap();
    fixture.gate.release();
    release(&runner, &id);
}

#[test]
fn oversized_real_artifact_and_plugin_error_become_small_resource_failures() {
    let mut engine = engine();
    engine.apply(Statement::new("ball", "sphere", Params::new().with("r", 5.0), &[]).unwrap()).unwrap();
    let limits = RunnerLimits { max_queued_bytes: 2048, ..RunnerLimits::default() };
    let runner = runner(limits);
    let mut request = call("artifact");
    request.input.node = "ball".to_owned();
    request.input.triangles = true;
    let id = runner.start(&engine, request).unwrap();
    let [started, terminal] = pair(&runner, &id);
    failed(&terminal, "resource_limit");
    assert!(
        encode_event(&started).unwrap().len() + encode_event(&terminal).unwrap().len()
            <= limits.max_queued_bytes
    );
    let fixture = Fixture::new(Behavior::HugeError);
    let id = runner.start(&fixture.engine, call("error")).unwrap();
    fixture.entered();
    fixture.gate.release();
    let [_, terminal] = pair(&runner, &id);
    failed(&terminal, "resource_limit");
    assert!(encode_event(&terminal).unwrap().len() < 512);
}

#[test]
fn panicking_worker_publishes_internal_once_and_returns_capacity() {
    let fixture = Fixture::new(Behavior::Panic);
    let runner = runner(RunnerLimits { max_live_workers: 1, ..RunnerLimits::default() });
    let id = runner.start(&fixture.engine, call("panic")).unwrap();
    fixture.entered();
    fixture.gate.release();
    let [_, terminal] = pair(&runner, &id);
    failed(&terminal, "internal");
    release(&runner, &id);
    let id = runner.start(&engine(), call("healthy")).unwrap();
    let [_, terminal] = pair(&runner, &id);
    assert_eq!(result(&terminal).volume, 48.0);
}

#[test]
fn expired_deadline_takes_precedence_over_a_plugin_panic_and_returns_capacity() {
    let fixture = Fixture::new(Behavior::Panic);
    let clock = Arc::new(FakeClock::default());
    let runner =
        MeshRunner::new(RunnerLimits { max_live_workers: 1, ..RunnerLimits::default() }, clock.clone())
            .unwrap();
    let mut request = call("panic");
    request.timeout_ms = Some(5);
    let id = runner.start(&fixture.engine, request).unwrap();
    fixture.entered();
    clock.advance(Duration::from_millis(5));
    fixture.gate.release();
    let [_, terminal] = pair(&runner, &id);
    failed(&terminal, "deadline_exceeded");
    release(&runner, &id);
    let id = runner.start(&engine(), call("healthy")).unwrap();
    let [_, terminal] = pair(&runner, &id);
    assert_eq!(result(&terminal).volume, 48.0);
}

#[test]
fn plugin_decline_remains_a_structured_unsupported_failure() {
    let fixture = Fixture::new(Behavior::Decline);
    let runner = runner(RunnerLimits::default());
    let id = runner.start(&fixture.engine, call("decline")).unwrap();
    fixture.entered();
    fixture.gate.release();
    let [_, terminal] = pair(&runner, &id);
    failed(&terminal, "unsupported");
}

#[test]
fn repeated_cancel_completion_races_never_publish_after_the_terminal() {
    for _ in 0..40 {
        let fixture = Fixture::new(Behavior::Mesh);
        let runner = runner(RunnerLimits::default());
        let id = runner.start(&fixture.engine, call("race")).unwrap();
        fixture.entered();
        let barrier = Arc::new(Barrier::new(2));
        let release_gate = {
            let barrier = Arc::clone(&barrier);
            let gate = Arc::clone(&fixture.gate);
            std::thread::spawn(move || {
                barrier.wait();
                gate.release();
            })
        };
        barrier.wait();
        runner.cancel(&id).unwrap();
        runner.cancel(&id).unwrap();
        release_gate.join().unwrap();
        let [_, terminal] = pair(&runner, &id);
        assert!(matches!(terminal.body, MeshEventBody::Completed(_) | MeshEventBody::Cancelled));
        // Releasing proves the worker has passed its publication boundary as well.
        release(&runner, &id);
        assert_eq!(runner.next(&id).unwrap_err().code.as_str(), "not_found");
    }
}

#[test]
fn drop_requests_cancel_without_waiting_for_an_uncooperative_plugin() {
    let fixture = Fixture::new(Behavior::Mesh);
    let runner = runner(RunnerLimits::default());
    runner.start(&fixture.engine, call("drop")).unwrap();
    fixture.entered();
    let (send, recv) = mpsc::channel();
    let dropping = std::thread::spawn(move || {
        drop(runner);
        send.send(()).unwrap();
    });
    recv.recv_timeout(WATCHDOG).unwrap();
    dropping.join().unwrap();
    fixture.gate.release();
}

#[test]
fn runners_generate_distinct_handles_even_for_different_engines() {
    let first = runner(RunnerLimits::default());
    let second = runner(RunnerLimits::default());
    let one = first.start(&engine(), call("same")).unwrap();
    let two = second.start(&engine(), call("same")).unwrap();
    assert_ne!(one, two);
    assert_eq!(first.cancel(&two).unwrap_err().code.as_str(), "not_found");
    pair(&first, &one);
    pair(&second, &two);
}

#[test]
fn validation_string_caps_and_started_reservation_reject_before_acceptance() {
    let engine = engine();
    let limits = RunnerLimits { max_request_id_bytes: 4, max_input_bytes: 4, ..RunnerLimits::default() };
    let runner = runner(limits);
    let mut bad_version = call("v1");
    bad_version.protocol.major = 1;
    assert_eq!(runner.start(&engine, bad_version).unwrap_err().code.as_str(), "unsupported_version");
    assert_eq!(runner.start(&engine, call("ééé")).unwrap_err().code.as_str(), "resource_limit");
    let mut long_input = call("node");
    long_input.input.node = "x".repeat(5);
    assert_eq!(runner.start(&engine, long_input).unwrap_err().code.as_str(), "resource_limit");
    let mut invalid = call("bad");
    invalid.input.tolerance.value = f64::NAN;
    assert_eq!(runner.start(&engine, invalid).unwrap_err().code.as_str(), "input");
    let id = runner.start(&engine, call("good")).unwrap();
    pair(&runner, &id);

    for limits in [
        RunnerLimits { max_queued_bytes: 1, ..RunnerLimits::default() },
        RunnerLimits { max_operation_id_bytes: 1, ..RunnerLimits::default() },
    ] {
        let runner = self::runner(limits);
        for _ in 0..2 {
            assert_eq!(runner.start(&engine, call("same")).unwrap_err().code.as_str(), "resource_limit");
        }
    }
}

#[test]
fn invalid_limits_are_rejected_and_two_event_capacity_is_mandatory() {
    for limits in [
        RunnerLimits { max_live_workers: 0, ..RunnerLimits::default() },
        RunnerLimits { max_retained_operations: 0, ..RunnerLimits::default() },
        RunnerLimits { max_request_id_bytes: 0, ..RunnerLimits::default() },
        RunnerLimits { max_operation_id_bytes: 0, ..RunnerLimits::default() },
        RunnerLimits { max_input_bytes: 0, ..RunnerLimits::default() },
        RunnerLimits { max_queued_events: 1, ..RunnerLimits::default() },
        RunnerLimits { max_queued_bytes: 0, ..RunnerLimits::default() },
    ] {
        let error = MeshRunner::new(limits, Arc::new(FakeClock::default())).err().unwrap();
        assert_eq!(error.code.as_str(), "input");
    }
}

struct OverflowClock;
impl Clock for OverflowClock {
    fn now(&self) -> Duration {
        Duration::MAX
    }
}

#[test]
fn overflowing_deadline_rejects_without_retaining_a_request() {
    let runner = MeshRunner::new(RunnerLimits::default(), Arc::new(OverflowClock)).unwrap();
    let engine = engine();
    let mut request = call("overflow");
    request.timeout_ms = Some(1);
    assert_eq!(runner.start(&engine, request).unwrap_err().code.as_str(), "resource_limit");
    let id = runner.start(&engine, call("overflow")).unwrap();
    pair(&runner, &id);
}

struct PanickingClock(AtomicUsize);

impl Clock for PanickingClock {
    fn now(&self) -> Duration {
        assert_eq!(self.0.fetch_add(1, Ordering::Relaxed), 0, "injected clock panic");
        Duration::ZERO
    }
}

#[test]
fn clock_panics_during_execution_and_cleanup_do_not_strand_capacity() {
    let clock = Arc::new(PanickingClock(AtomicUsize::new(0)));
    let runner =
        MeshRunner::new(RunnerLimits { max_live_workers: 1, ..RunnerLimits::default() }, clock).unwrap();
    let engine = engine();
    let mut request = call("clock");
    request.timeout_ms = Some(1);
    let id = runner.start(&engine, request).unwrap();
    let [_, terminal] = pair(&runner, &id);
    failed(&terminal, "internal");
    release(&runner, &id);
    let id = runner.start(&engine, call("healthy")).unwrap();
    let [_, terminal] = pair(&runner, &id);
    assert_eq!(result(&terminal).volume, 48.0);
}
