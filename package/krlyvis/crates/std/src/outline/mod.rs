//! Outlines: where the subject of a picture is, as polygons.

pub mod mask;
pub mod simplify;
pub mod trace;

use krlyvis_kernel::ports::outline::Outline;
use krlyvis_kernel::VisionBuilder;

pub fn register(builder: &mut VisionBuilder) {
    builder.plugin::<Outline>(trace::Trace);
}
