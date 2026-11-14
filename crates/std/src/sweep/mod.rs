//! Sweep rules: the surface a profile edge traces under a motion.

pub mod circle_shift;
pub mod circle_turn;
pub mod line_shift;
pub mod line_turn;

use krly3d_kernel::ports::sweep::Sweep;
use krly3d_kernel::KernelBuilder;

pub fn register(builder: &mut KernelBuilder) {
    builder
        .plugin::<Sweep>(line_shift::LineShift)
        .plugin::<Sweep>(circle_shift::CircleShift)
        .plugin::<Sweep>(line_turn::LineTurn)
        .plugin::<Sweep>(circle_turn::CircleTurn);
}

#[cfg(test)]
mod testing {
    use crate::standard;
    use krly3d_core::{Accuracy, Attempt};
    use krly3d_kernel::ports::curve::Curve;
    use krly3d_kernel::ports::surface::Surface;
    use krly3d_kernel::ports::sweep::{contract, sweep, Motion, SweepRequest};
    use std::sync::Arc;

    /// Sweeps through the standard registry, the way a tool does, and on success checks
    /// the sweep contract before handing the surface to the test.
    pub(crate) fn swept(curve: impl Curve, motion: Motion) -> Attempt<Arc<dyn Surface>> {
        let mut builder = standard();
        builder.accuracy(Accuracy::for_scale(10.0).unwrap());
        let cx = builder.context();
        let request = SweepRequest { curve: Arc::new(curve), motion };
        let attempt = sweep(&cx, &request);
        if let Attempt::Done(surface) = &attempt {
            contract::check(surface.as_ref(), &request, &[-2.0, 0.0, 0.9, 3.5], cx.accuracy()).unwrap();
        }
        attempt
    }

    pub(crate) fn kind(attempt: &Attempt<Arc<dyn Surface>>) -> &'static str {
        match attempt {
            Attempt::Done(surface) => surface.classes()[0],
            Attempt::Declined(_) => "declined",
            Attempt::Failed(_) => "failed",
        }
    }
}
