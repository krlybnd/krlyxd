//! Every capability shipped with krlyvis, one plugin per file.
//!
//! The kernel crate implements nothing; whatever a standard vision kernel can do is
//! registered here. A crate of your own can add to it the same way.

pub mod camera;
pub mod depth;
pub mod detect;
pub mod make;
pub mod measure;
pub mod outline;
pub mod read;
pub mod stage;
pub mod vanish;

use krlyvis_kernel::VisionBuilder;

/// A builder with every standard plugin registered and the standard stages in order.
pub fn standard() -> VisionBuilder {
    let mut builder = VisionBuilder::new();
    read::register(&mut builder);
    outline::register(&mut builder);
    make::register(&mut builder);
    detect::register(&mut builder);
    vanish::register(&mut builder);
    camera::register(&mut builder);
    measure::register(&mut builder);
    depth::register(&mut builder);
    stage::register(&mut builder);
    builder
}
