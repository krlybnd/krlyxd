use std::any::TypeId;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use krlyxd::{
    ApiVersion, ArtifactId, CallControl, CallStop, Clock, Diagnostic, Exactness, FakeClock, Id, Length,
    LengthUnit, OperationId, RequestId,
};

#[test]
fn records_expose_the_agreed_fields_without_a_codec() {
    let protocol = ApiVersion { major: 1, minor: 0 };
    let diagnostic = Diagnostic { code: Id::new("unsupported"), message: "unknown operation".into() };
    let length = Length { value: 0.05, unit: LengthUnit::Model };
    let physical = Length { value: 1.0, unit: LengthUnit::Metre };

    assert_eq!((protocol.major, protocol.minor), (1, 0));
    assert_eq!(diagnostic.code.as_str(), "unsupported");
    assert_eq!(diagnostic.message, "unknown operation");
    assert_eq!(length, Length { value: 0.05, unit: LengthUnit::Model });
    assert_ne!(length.unit, physical.unit);
    assert_eq!(Exactness::Sampled { samples: 12 }.symbol(), '?');
}

#[test]
fn handle_types_are_distinct_owned_text_values() {
    let text = String::from("session/17");
    let request = RequestId::new(&text);
    let operation = OperationId::new(&text);
    let artifact = ArtifactId::new(&text);
    drop(text);

    assert_eq!(request.as_str(), "session/17");
    assert_eq!(operation.as_str(), request.as_str());
    assert_eq!(artifact.as_str(), request.as_str());
    assert_eq!(request.clone(), RequestId::new("session/17"));
    assert_eq!(operation.clone(), OperationId::new("session/17"));
    assert_eq!(artifact.clone(), ArtifactId::new("session/17"));
    let types = BTreeSet::from([
        TypeId::of::<RequestId>(),
        TypeId::of::<OperationId>(),
        TypeId::of::<ArtifactId>(),
        TypeId::of::<Id>(),
    ]);
    assert_eq!(types.len(), 4);
}

#[test]
fn cancellation_is_idempotent_and_isolated_per_call() {
    let control = CallControl::new();
    let other = CallControl::default();
    assert_eq!(control.check(), Ok(()));

    control.cancel();
    control.cancel();

    assert_eq!(control.check(), Err(CallStop::Cancelled));
    assert_eq!(other.check(), Ok(()));
}

#[test]
fn cancellation_can_be_requested_from_another_thread() {
    let control = Arc::new(CallControl::new());
    let canceller = Arc::clone(&control);

    std::thread::scope(|scope| {
        scope.spawn(move || canceller.cancel());
    });

    assert_eq!(control.check(), Err(CallStop::Cancelled));
}

#[test]
fn deadline_is_absolute_and_inclusive_in_the_injected_clock() {
    let clock = Arc::new(FakeClock::default());
    clock.advance(Duration::from_secs(7));
    let control = CallControl::with_deadline(clock.clone(), Duration::from_secs(10));

    for _ in 0..3 {
        assert_eq!(control.check(), Ok(()));
    }
    clock.advance(Duration::from_secs(2));
    assert_eq!(control.check(), Ok(()));
    clock.advance(Duration::from_secs(1));
    assert_eq!(control.check(), Err(CallStop::DeadlineExceeded));
    clock.advance(Duration::from_secs(1));
    assert_eq!(control.check(), Err(CallStop::DeadlineExceeded));
}

#[test]
fn zero_and_already_elapsed_deadlines_stop_immediately() {
    let clock = Arc::new(FakeClock::default());
    let zero = CallControl::with_deadline(clock.clone(), Duration::ZERO);
    assert_eq!(zero.check(), Err(CallStop::DeadlineExceeded));

    clock.advance(Duration::from_secs(2));
    let elapsed = CallControl::with_deadline(clock, Duration::from_secs(1));
    assert_eq!(elapsed.check(), Err(CallStop::DeadlineExceeded));
}

#[test]
fn explicit_cancel_takes_precedence_over_an_elapsed_deadline() {
    let control = CallControl::with_deadline(Arc::new(FakeClock::default()), Duration::ZERO);
    control.cancel();

    assert_eq!(control.check(), Err(CallStop::Cancelled));
    assert_eq!(CallStop::Cancelled.to_string(), "cancelled");
    assert_eq!(CallStop::DeadlineExceeded.to_string(), "deadline exceeded");
}

#[test]
fn checking_a_cancelled_call_does_not_read_the_clock() {
    #[derive(Default)]
    struct CountingClock(std::sync::atomic::AtomicUsize);
    impl Clock for CountingClock {
        fn now(&self) -> Duration {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Duration::ZERO
        }
    }
    let clock = Arc::new(CountingClock::default());
    let control = CallControl::with_deadline(clock.clone(), Duration::from_secs(1));
    assert_eq!(control.check(), Ok(()));
    control.cancel();
    assert_eq!(control.check(), Err(CallStop::Cancelled));
    assert_eq!(clock.0.load(std::sync::atomic::Ordering::Relaxed), 1);
}
