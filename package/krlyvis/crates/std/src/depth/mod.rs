//! Depth methods: how far away things stand and how high they are.

pub mod ground;

use krlyvis_kernel::ports::depth::Depth;
use krlyvis_kernel::VisionBuilder;

pub fn register(builder: &mut VisionBuilder) {
    builder.plugin::<Depth>(ground::Ground::default());
}
