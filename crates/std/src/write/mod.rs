//! Writers: a document as text of some format.

pub mod k3d;

use krly3d_kernel::ports::write::Write;
use krly3d_kernel::KernelBuilder;

pub fn register(builder: &mut KernelBuilder) {
    builder.plugin::<Write>(k3d::K3dWriter);
}
