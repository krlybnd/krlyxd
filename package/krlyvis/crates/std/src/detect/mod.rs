//! Detectors: the straight segments of an image.

pub mod lsd;

use krlyvis_kernel::ports::detect::Detect;
use krlyvis_kernel::VisionBuilder;

pub fn register(builder: &mut VisionBuilder) {
    builder.plugin::<Detect>(lsd::Lsd::default());
}
