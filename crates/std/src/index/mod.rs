//! Indexes: search structures derived from a body.

pub mod keys;

use krly3d_kernel::ports::index::Indexing;
use krly3d_kernel::KernelBuilder;

pub fn register(builder: &mut KernelBuilder) {
    builder.plugin::<Indexing>(keys::KeyIndexer);
}
