//! Bounded background execution over detached, immutable mesh tasks.
//!
//! Limits cover retained canonical event bytes, not solver peak memory or the temporary
//! encoding of a candidate event. This pilot emits exactly Started and one terminal.

use crate::{MeshEngine, MeshTask};
use krly3d_mesh::operation::{encode_event, MeshEvent, MeshEventBody, OperationCall};
use krly3d_mesh::{diagnostic, Diagnostic, MeshCall, RequestId};
use krlyxd::{CallControl, CallStop, Clock, OperationId};
use std::collections::{BTreeMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

static NEXT_OPERATION: AtomicU64 = AtomicU64::new(1);

pub use krlyxd::runner_limits::RunnerLimits;

/// A transport-independent runner. Dropping it requests cooperative cancellation and
/// detaches workers; it never waits indefinitely for an uncooperative plugin.
pub struct MeshRunner {
    shared: Arc<Shared>,
    clock: Arc<dyn Clock>,
}

struct Shared {
    limits: RunnerLimits,
    registry: Mutex<Registry>,
}

#[derive(Default)]
struct Registry {
    operations: BTreeMap<OperationId, Arc<Record>>,
    requests: BTreeMap<RequestId, OperationId>,
    live_workers: usize,
}

struct Record {
    request_id: RequestId,
    operation_id: OperationId,
    control: Arc<CallControl>,
    state: Mutex<RecordState>,
    changed: Condvar,
    fallback: Fallbacks,
}

struct RecordState {
    events: VecDeque<QueuedEvent>,
    queued_bytes: usize,
    terminal: bool,
    computing: bool,
}

#[derive(Clone)]
struct QueuedEvent {
    event: MeshEvent,
    bytes: usize,
}

struct Fallbacks {
    resource: QueuedEvent,
    internal: QueuedEvent,
    deadline: QueuedEvent,
    cancelled: QueuedEvent,
}

impl MeshRunner {
    pub fn new(limits: RunnerLimits, clock: Arc<dyn Clock>) -> Result<Self, Diagnostic> {
        if !limits.is_valid() {
            return Err(diagnostic("input", "runner limits must be positive and reserve two events"));
        }
        Ok(Self { shared: Arc::new(Shared { limits, registry: Mutex::default() }), clock })
    }

    /// Validate and pin a snapshot before accepting work. Request IDs stay reserved
    /// after completion and draining, until explicit release. A rejected call has no
    /// operation or events. IDs are process-unique across runner and engine instances.
    pub fn start(&self, engine: &MeshEngine, call: OperationCall) -> Result<OperationId, Diagnostic> {
        let limits = self.shared.limits;
        if call.request_id.as_str().len() > limits.max_request_id_bytes
            || call.input.node.len() > limits.max_input_bytes
        {
            return Err(resource_limit());
        }
        call.validate()?;
        let control = Arc::new(match call.timeout_ms {
            Some(timeout) => {
                let deadline = self
                    .clock
                    .now()
                    .checked_add(Duration::from_millis(timeout))
                    .ok_or_else(resource_limit)?;
                CallControl::with_deadline(Arc::clone(&self.clock), deadline)
            }
            None => CallControl::new(),
        });
        let prepared = engine.prepare(MeshCall::new(call.request_id.clone(), call.input))?;
        let task = engine.task(prepared, Arc::clone(&control))?;
        let serial = NEXT_OPERATION
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| resource_limit())?;
        let operation_id = OperationId::new(&format!("mesh-op:{serial:016x}"));
        if operation_id.as_str().len() > limits.max_operation_id_bytes {
            return Err(resource_limit());
        }
        let event = |seq, body| MeshEvent {
            request_id: call.request_id.clone(),
            operation_id: operation_id.clone(),
            seq,
            body,
        };
        let started = queued(event(0, MeshEventBody::Started(task.provenance())))?;
        let fallback = Fallbacks {
            resource: queued(event(1, MeshEventBody::Failed(resource_limit())))?,
            internal: queued(event(
                1,
                MeshEventBody::Failed(diagnostic("internal", "mesh worker panicked")),
            ))?,
            deadline: queued(event(
                1,
                MeshEventBody::Failed(diagnostic("deadline_exceeded", "mesh deadline exceeded")),
            ))?,
            cancelled: queued(event(1, MeshEventBody::Cancelled))?,
        };
        let reserve = [&fallback.resource, &fallback.internal, &fallback.deadline, &fallback.cancelled]
            .into_iter()
            .map(|event| event.bytes)
            .max()
            .unwrap_or(0);
        if started.bytes.checked_add(reserve).is_none_or(|bytes| bytes > limits.max_queued_bytes) {
            return Err(resource_limit());
        }
        let record = Arc::new(Record {
            request_id: call.request_id,
            operation_id: operation_id.clone(),
            control,
            changed: Condvar::new(),
            state: Mutex::new(RecordState {
                queued_bytes: started.bytes,
                events: VecDeque::from([started]),
                terminal: false,
                computing: true,
            }),
            fallback,
        });
        let mut registry = lock(&self.shared.registry);
        if registry.requests.contains_key(&record.request_id) {
            return Err(diagnostic("input", "request_id is already retained by this runner"));
        }
        if registry.live_workers >= limits.max_live_workers
            || registry.operations.len() >= limits.max_retained_operations
        {
            return Err(resource_limit());
        }
        registry.requests.insert(record.request_id.clone(), operation_id.clone());
        registry.operations.insert(operation_id.clone(), Arc::clone(&record));
        registry.live_workers += 1;
        let shared = Arc::clone(&self.shared);
        // Construct the lease inside the thread: a failed spawn drops its closure
        // while admission holds the registry lock and must not run lease cleanup.
        if std::thread::Builder::new()
            .name(operation_id.as_str().to_owned())
            .spawn(move || work(task, WorkerLease { shared, record }))
            .is_err()
        {
            registry.operations.remove(&operation_id);
            registry.requests.retain(|_, id| id != &operation_id);
            registry.live_workers -= 1;
            return Err(diagnostic("resource_limit", "could not spawn mesh worker"));
        }
        Ok(operation_id)
    }

    /// Drain one available event without waiting for computation. None means the queue
    /// is currently empty, including an already-drained terminal operation.
    pub fn next(&self, id: &OperationId) -> Result<Option<MeshEvent>, Diagnostic> {
        let record = self.record(id)?;
        let mut state = lock(&record.state);
        Ok(state.events.pop_front().map(|queued| {
            state.queued_bytes -= queued.bytes;
            queued.event
        }))
    }

    /// Wait for one event using the same queue as `next`. Returns None once the
    /// terminal event has been drained. This does not wait for a cancelled plugin
    /// to exit; its live-worker permit is retained separately until cleanup.
    pub fn wait_next(&self, id: &OperationId) -> Result<Option<MeshEvent>, Diagnostic> {
        let record = self.record(id)?;
        let mut state = lock(&record.state);
        while state.events.is_empty() && !state.terminal {
            state = record.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        Ok(state.events.pop_front().map(|queued| {
            state.queued_bytes -= queued.bytes;
            queued.event
        }))
    }

    /// Cancellation and terminal publication share the record lock and the control's
    /// publication gate. The first terminal decision wins; repeats are harmless.
    pub fn cancel(&self, id: &OperationId) -> Result<(), Diagnostic> {
        let record = self.record(id)?;
        cancel(&record);
        Ok(())
    }

    /// Forget a terminal operation only after computation has actually exited. This
    /// discards unread events and permits explicit reuse of its request ID. Draining
    /// events alone never frees a record. Unknown or released IDs return not_found.
    pub fn release(&self, id: &OperationId) -> Result<(), Diagnostic> {
        let mut registry = lock(&self.shared.registry);
        let record = registry.operations.get(id).ok_or_else(not_found)?;
        let state = lock(&record.state);
        if !state.terminal || state.computing {
            return Err(diagnostic("input", "mesh operation is still computing"));
        }
        let request_id = record.request_id.clone();
        drop(state);
        let removed = registry.operations.remove(id);
        registry.requests.remove(&request_id);
        drop(registry);
        // Unread completed artifacts can be large; destroy them outside the
        // registry lock so release cannot stall other operations' admission/drains.
        drop(removed);
        Ok(())
    }

    fn record(&self, id: &OperationId) -> Result<Arc<Record>, Diagnostic> {
        lock(&self.shared.registry).operations.get(id).cloned().ok_or_else(not_found)
    }
}

impl Drop for MeshRunner {
    fn drop(&mut self) {
        let records: Vec<_> = lock(&self.shared.registry).operations.values().cloned().collect();
        for record in records {
            cancel(&record);
        }
    }
}

fn cancel(record: &Record) {
    let mut state = lock(&record.state);
    if !state.terminal {
        record.control.cancel();
        state.finish(record.fallback.cancelled.clone());
        record.changed.notify_all();
    }
}

fn queued(event: MeshEvent) -> Result<QueuedEvent, Diagnostic> {
    let bytes = encode_event(&event)?.len();
    Ok(QueuedEvent { event, bytes })
}

impl RecordState {
    fn finish(&mut self, event: QueuedEvent) {
        self.queued_bytes += event.bytes;
        self.events.push_back(event);
        self.terminal = true;
    }
}

/// The permit survives cancellation and unwinding, including task destruction and
/// serialization. The runner owns no JoinHandle and never waits for plugin exit.
struct WorkerLease {
    shared: Arc<Shared>,
    record: Arc<Record>,
}

impl Drop for WorkerLease {
    fn drop(&mut self) {
        let mut state = lock(&self.record.state);
        if !state.terminal {
            // Panic cleanup observes deadlines too. If the injected clock itself
            // panics, retain an internal terminal without stranding the permit.
            let mut candidate = Some(self.record.fallback.internal.clone());
            let published = catch_unwind(AssertUnwindSafe(|| {
                publish(&self.record, &mut state, &mut candidate, self.shared.limits);
            }));
            if published.is_err() && !state.terminal {
                state.finish(self.record.fallback.internal.clone());
            }
        }
        self.record.changed.notify_all();
        drop(state);
        let mut registry = lock(&self.shared.registry);
        let mut state = lock(&self.record.state);
        state.computing = false;
        registry.live_workers -= 1;
    }
}

fn work(task: MeshTask, lease: WorkerLease) {
    // Catch both plugin panics and publication/encoding panics. Poisoned locks are
    // recoverable; the lease supplies an internal terminal and returns capacity.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let record = &lease.record;
        let body = match task.execute() {
            Ok(result) => MeshEventBody::Completed(result),
            Err(error) => MeshEventBody::Failed(error),
        };
        if lock(&record.state).terminal {
            return;
        }
        let candidate = queued(MeshEvent {
            request_id: record.request_id.clone(),
            operation_id: record.operation_id.clone(),
            seq: 1,
            body,
        })
        .unwrap_or_else(|_| record.fallback.resource.clone());
        let mut state = lock(&record.state);
        if state.terminal {
            return;
        }
        // Borrow the candidate during publication, leaving rejected large payloads
        // to be destroyed after releasing the record and publication locks.
        let mut candidate = Some(candidate);
        publish(record, &mut state, &mut candidate, lease.shared.limits);
        record.changed.notify_all();
        drop(state);
        drop(candidate);
    }));
}

fn publish(
    record: &Record,
    state: &mut RecordState,
    candidate: &mut Option<QueuedEvent>,
    limits: RunnerLimits,
) {
    match record.control.publish(|| {
        if candidate.as_ref().is_some_and(|event| event.bytes <= limits.max_queued_bytes - state.queued_bytes)
        {
            if let Some(event) = candidate.take() {
                state.finish(event);
            }
        } else {
            state.finish(record.fallback.resource.clone());
        }
    }) {
        Ok(()) => {}
        Err(CallStop::Cancelled) => state.finish(record.fallback.cancelled.clone()),
        Err(CallStop::DeadlineExceeded) => state.finish(record.fallback.deadline.clone()),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn resource_limit() -> Diagnostic {
    diagnostic("resource_limit", "mesh runner resource limit exceeded")
}

fn not_found() -> Diagnostic {
    diagnostic("not_found", "mesh operation is not retained")
}
