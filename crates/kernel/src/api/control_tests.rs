use super::*;
use crate::ports::query::{Answer, QueryRequest};
use crate::{Context, KernelBuilder, Plugin};
use krly3d_core::{Attempt, FakeClock};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct StopInQuery {
    stop: Arc<CallControl>,
    calls: Arc<AtomicUsize>,
}

impl Plugin<Query> for StopInQuery {
    fn id(&self) -> Id {
        Id::new("stop")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema { about: "cancel during a read-only query", inputs: (0, 0), params: vec![] })
    }

    fn call(&self, _: &Context, _: &QueryRequest) -> Attempt<Answer> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.stop.cancel();
        // Deliberately omit an internal check: query::ask must reject late success.
        Attempt::Done(Answer { text: "late success".into(), data: Value::map([]), exact: Exactness::Exact })
    }
}

fn stopping_session(stop: &Arc<CallControl>) -> (Session, Arc<AtomicUsize>) {
    let mut builder = KernelBuilder::new();
    let calls = Arc::new(AtomicUsize::new(0));
    builder.plugin::<Query>(StopInQuery { stop: Arc::clone(stop), calls: Arc::clone(&calls) });
    (builder.session(), calls)
}

fn assert_cancelled(reply: &Reply) {
    assert!(!reply.ok, "{reply:?}");
    assert_eq!(reply.data, Value::map([("error", Value::from("cancelled"))]));
    assert_eq!(reply.text, "cancelled: cancelled");
}

#[test]
fn cancelled_query_returns_the_public_error_without_poisoning_the_session() {
    let control = Arc::new(CallControl::new());
    let (mut session, calls) = stopping_session(&control);
    assert!(session.call("mark", &Params::new()).ok);
    let before = session.kernel.snapshot();

    assert_cancelled(&session.call_with_control("stop", &Params::new(), Arc::clone(&control)));

    let after = session.kernel.snapshot();
    assert!(Arc::ptr_eq(&before.0, &after.0));
    assert!(Arc::ptr_eq(&before.1, &after.1));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(session.kernel.context().check_control(), Ok(()));
    assert!(session.call("rollback", &Params::new()).ok, "existing session mark remains usable");
    assert!(session.call_with_control("methods", &Params::new(), Arc::new(CallControl::new())).ok);
}

#[test]
fn a_pre_cancelled_call_never_dispatches_or_changes_marks() {
    let control = Arc::new(CallControl::new());
    let (mut session, calls) = stopping_session(&control);
    control.cancel();

    assert_cancelled(&session.call_with_control("stop", &Params::new(), Arc::clone(&control)));
    assert_cancelled(&session.call_with_control("mark", &Params::new(), control));

    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(session.call("mark", &Params::new()).text, "mark 1");
}

#[test]
fn injected_deadline_uses_the_same_public_reply_path() {
    let clock = Arc::new(FakeClock::default());
    let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
    let (mut session, calls) = stopping_session(&control);
    clock.advance(Duration::from_secs(1));

    let reply = session.call_with_control("stop", &Params::new(), control);

    assert!(!reply.ok);
    assert_eq!(reply.data, Value::map([("error", Value::from("deadline_exceeded"))]));
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn nested_batches_forward_control_and_keep_the_existing_failure_reply_shape() {
    let control = Arc::new(CallControl::new());
    let (mut session, calls) = stopping_session(&control);
    let one = |method: &str, args: Params| {
        Value::map([("method", Value::from(method)), ("args", args.into_value())])
    };
    let inner = Params::new().with("calls", vec![one("stop", Params::new()), one("mark", Params::new())]);
    let outer = Params::new().with("calls", vec![one("mark", Params::new()), one("batch", inner)]);

    let reply = session.call_with_control("batch", &outer, control);

    assert!(!reply.ok);
    assert!(reply.text.contains("cancelled: cancelled"));
    assert!(!reply.text.contains("late success"));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(session.call("mark", &Params::new()).text, "mark 1", "batch marks were rolled back");
    let Value::Map(data) = reply.data else { panic!("batch data must remain a map") };
    assert!(matches!(data.get("replies"), Some(Value::List(replies)) if replies.len() == 2));
}

#[test]
fn cancelled_batch_restores_marks_consumed_by_an_earlier_rollback() {
    let control = Arc::new(CallControl::new());
    let (mut session, _) = stopping_session(&control);
    session.call("mark", &Params::new());
    session.call("mark", &Params::new());
    let calls = vec![
        Value::map([
            ("method", Value::from("rollback")),
            ("args", Params::new().with("mark", 1_usize).into_value()),
        ]),
        Value::map([("method", Value::from("stop"))]),
    ];

    assert!(!session.call_with_control("batch", &Params::new().with("calls", calls), control).ok);

    assert!(session.call("rollback", &Params::new().with("mark", 2_usize)).ok);
}

#[test]
fn malformed_later_batch_entry_restores_the_original_marks() {
    let mut session = KernelBuilder::new().session();
    session.call("mark", &Params::new());
    session.call("mark", &Params::new());
    let calls = vec![
        Value::map([
            ("method", Value::from("rollback")),
            ("args", Params::new().with("mark", 1_usize).into_value()),
        ]),
        Value::from("not a call"),
    ];

    let reply = session.call("batch", &Params::new().with("calls", calls));

    assert_eq!(reply.data, Value::map([("error", Value::from("input"))]));
    assert!(session.call("rollback", &Params::new().with("mark", 2_usize)).ok);
}
