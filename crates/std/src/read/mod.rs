//! Readers: text of some format into a document.

pub mod k3d;

use krly3d_kernel::ports::read::Read;
use krly3d_kernel::KernelBuilder;

pub fn register(builder: &mut KernelBuilder) {
    builder.plugin::<Read>(k3d::K3dReader);
}
