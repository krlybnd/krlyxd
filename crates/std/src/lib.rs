//! Every capability shipped with krly3d, one plugin per file.
//!
//! The kernel crate implements nothing; whatever a standard kernel can do is registered
//! here. A crate of your own can add to it the same way.

pub mod content;
pub mod curve;
pub mod index;
pub mod interpret;
pub mod keys;
pub mod measure;
pub mod meet;
pub mod probe;
pub mod query;
pub mod read;
pub mod section;
pub mod stage;
pub mod surface;
pub mod sweep;
pub mod tool;
pub mod trim;
pub mod write;

/// Helpers for the tests of this crate and of plugin crates built on it.
#[cfg(any(test, feature = "testing"))]
pub use tool::testing;

use krly3d_kernel::KernelBuilder;

/// A builder with every standard plugin registered.
pub fn standard() -> KernelBuilder {
    let mut builder = KernelBuilder::new();
    sweep::register(&mut builder);
    meet::register(&mut builder);
    section::register(&mut builder);
    content::register(&mut builder);
    measure::register(&mut builder);
    keys::register(&mut builder);
    index::register(&mut builder);
    query::register(&mut builder);
    interpret::register(&mut builder);
    tool::register(&mut builder);
    trim::register(&mut builder);
    probe::register(&mut builder);
    stage::register(&mut builder);
    read::register(&mut builder);
    write::register(&mut builder);
    builder
}
