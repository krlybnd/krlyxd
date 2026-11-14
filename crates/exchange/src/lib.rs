//! Exchange with other programs: answers that are whole files in the formats they read.
//!
//! A package of its own, registered from outside like any plugin. The writers read a body
//! through the kernel's ports and the concrete curves and surfaces of `krly3d-std`.

mod part21;
pub mod step;

use krly3d_kernel::ports::query::Query;
use krly3d_kernel::KernelBuilder;

pub fn register(builder: &mut KernelBuilder) {
    builder.plugin::<Query>(step::Step);
}
