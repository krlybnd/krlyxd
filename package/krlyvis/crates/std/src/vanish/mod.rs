//! Vanishing-point methods.

pub mod vote;

use krlyvis_kernel::ports::vanish::Vanish;
use krlyvis_kernel::VisionBuilder;

pub fn register(builder: &mut VisionBuilder) {
    builder.plugin::<Vanish>(vote::Vote::default());
}
