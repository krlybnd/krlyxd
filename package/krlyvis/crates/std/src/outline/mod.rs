//! Outlines: where the subject of a picture is, as polygons.

pub mod holes;
pub mod mask;
pub mod simplify;
mod simplify_holes;
pub mod trace;

use krly3d_core::Error;
use krlyvis_kernel::ports::outline::Outline;
use krlyvis_kernel::{Context, VisionBuilder};

/// Bound pixel/edge work between cooperative checks, including single long rows.
/// Legacy mask entry points use the same algorithm with no attached context.
struct Checkpoints<'a> {
    cx: Option<&'a Context>,
    remaining: usize,
}

impl<'a> Checkpoints<'a> {
    fn new(cx: Option<&'a Context>) -> Result<Self, Error> {
        let work = Self { cx, remaining: 0 };
        work.check()?;
        Ok(work)
    }

    fn check(&self) -> Result<(), Error> {
        self.cx.map_or(Ok(()), Context::check_control)
    }

    fn step(&mut self) -> Result<(), Error> {
        if self.remaining == 0 {
            self.check()?;
            self.remaining = 1023;
        } else {
            self.remaining -= 1;
        }
        Ok(())
    }
}

pub fn register(builder: &mut VisionBuilder) {
    builder.plugin::<Outline>(trace::Trace);
    builder.plugin::<Outline>(holes::TraceHoles);
}

#[cfg(test)]
mod control_tests {
    use krly3d_core::{CallControl, Clock};
    use krlyvis_kernel::{Context, VisionBuilder};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    pub(super) struct BudgetClock {
        pub(super) checks: AtomicUsize,
        allowed: usize,
    }

    impl Clock for BudgetClock {
        fn now(&self) -> Duration {
            if self.checks.fetch_add(1, Ordering::Relaxed) >= self.allowed {
                Duration::from_secs(1)
            } else {
                Duration::ZERO
            }
        }
    }

    pub(super) fn context(allowed: usize) -> (Context, Arc<BudgetClock>) {
        let clock = Arc::new(BudgetClock { checks: AtomicUsize::new(0), allowed });
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
        (VisionBuilder::new().context().with_call_control(control), clock)
    }
}
