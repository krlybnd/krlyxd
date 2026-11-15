//! Time as an injected dependency, so that measuring never makes a test depend on the wall clock.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub trait Clock: Send + Sync {
    /// Time elapsed since the clock was created.
    fn now(&self) -> Duration;
}

/// The monotonic system clock.
#[derive(Clone, Copy, Debug)]
pub struct SystemClock {
    start: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self { start: Instant::now() }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }
}

/// A clock that only moves when told to.
#[derive(Debug, Default)]
pub struct FakeClock {
    nanos: AtomicU64,
}

impl FakeClock {
    pub fn advance(&self, by: Duration) {
        let nanos = u64::try_from(by.as_nanos()).unwrap_or(u64::MAX);
        self.nanos.fetch_add(nanos, Ordering::Relaxed);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        Duration::from_nanos(self.nanos.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, FakeClock};
    use std::time::Duration;

    #[test]
    fn a_fake_clock_moves_only_when_advanced() {
        let clock = FakeClock::default();
        let before = clock.now();

        clock.advance(Duration::from_millis(5));

        assert_eq!(before, Duration::ZERO);
        assert_eq!(clock.now(), Duration::from_millis(5));
    }
}
