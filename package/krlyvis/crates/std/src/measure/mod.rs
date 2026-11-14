//! Measuring methods: true lengths in a world plane.

pub mod plane;

use krlyvis_kernel::ports::measure::Measure;
use krlyvis_kernel::VisionBuilder;

pub fn register(builder: &mut VisionBuilder) {
    builder.plugin::<Measure>(plane::Plane);
}
