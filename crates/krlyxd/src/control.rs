//! Cooperative per-call stopping with an optional injected-clock deadline.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::Clock;

/// Why a cooperative checkpoint stopped work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallStop {
    Cancelled,
    DeadlineExceeded,
}

impl fmt::Display for CallStop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "cancelled",
            Self::DeadlineExceeded => "deadline exceeded",
        })
    }
}

impl std::error::Error for CallStop {}

/// A cancellation flag and optional absolute deadline in an injected clock's time base.
///
/// Share with `Arc<CallControl>` when another caller needs to request cancellation.
/// Checks do no scheduling. `publish` serializes a short publication with cancellation;
/// callers still own their terminal-outcome and cache policies.
#[derive(Default)]
pub struct CallControl {
    cancelled: AtomicBool,
    deadline: Option<(Arc<dyn Clock>, Duration)>,
    publication: Mutex<()>,
}

impl CallControl {
    /// A call with no deadline and no cancellation requested.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stop at or after `deadline`, measured from the injected clock's origin.
    pub fn with_deadline(clock: Arc<dyn Clock>, deadline: Duration) -> Self {
        Self { deadline: Some((clock, deadline)), ..Self::default() }
    }

    /// Request cancellation. Repeated requests have the same effect.
    pub fn cancel(&self) {
        let _publication = self.publication.lock().unwrap_or_else(PoisonError::into_inner);
        // The flag communicates only cancellation, not ownership of other memory.
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Run a short publication only if the call can still proceed. Cancellation and
    /// publication use the same mutex: once cancel wins, this closure cannot run.
    /// The deadline is checked at that same publication boundary. Do not call `cancel`
    /// or `publish` on this control from the closure, or perform long work there.
    pub fn publish<T>(&self, f: impl FnOnce() -> T) -> Result<T, CallStop> {
        let _publication = self.publication.lock().unwrap_or_else(PoisonError::into_inner);
        self.check()?;
        Ok(f())
    }

    /// Check at a bounded work checkpoint. Explicit cancellation takes precedence
    /// when cancellation and deadline expiry are both observed.
    pub fn check(&self) -> Result<(), CallStop> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(CallStop::Cancelled);
        }
        if let Some((clock, deadline)) = &self.deadline {
            if clock.now() >= *deadline {
                return Err(CallStop::DeadlineExceeded);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CallControl, CallStop};
    use crate::FakeClock;
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    #[test]
    fn cancellation_first_prevents_the_publication_closure() {
        let control = CallControl::new();
        control.cancel();
        let mut published = false;
        assert_eq!(control.publish(|| published = true), Err(CallStop::Cancelled));
        assert!(!published);
    }

    #[test]
    fn publication_first_finishes_before_cancellation() {
        let control = Arc::new(CallControl::new());
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let publishing = {
            let (control, entered, release) =
                (Arc::clone(&control), Arc::clone(&entered), Arc::clone(&release));
            std::thread::spawn(move || {
                control.publish(|| {
                    entered.wait();
                    release.wait();
                    // A concurrent cancel must wait until this publication releases the gate.
                    assert_eq!(control.check(), Ok(()));
                    42
                })
            })
        };
        entered.wait();
        assert!(control.publication.try_lock().is_err());
        let (requesting, requested) = mpsc::sync_channel(0);
        let cancelling = {
            let control = Arc::clone(&control);
            std::thread::spawn(move || {
                requesting.send(()).unwrap();
                control.cancel();
            })
        };
        requested.recv_timeout(Duration::from_secs(5)).unwrap();
        release.wait();
        assert_eq!(publishing.join().unwrap(), Ok(42));
        cancelling.join().unwrap();
        assert_eq!(control.publish(|| 43), Err(CallStop::Cancelled));
    }

    #[test]
    fn deadline_first_prevents_the_publication_closure() {
        let clock = Arc::new(FakeClock::default());
        let control = CallControl::with_deadline(clock.clone(), Duration::from_secs(1));
        clock.advance(Duration::from_secs(1));
        let mut published = false;
        assert_eq!(control.publish(|| published = true), Err(CallStop::DeadlineExceeded));
        assert!(!published);
    }
}
