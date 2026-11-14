//! What the statements of the vis layer make, one tool per file: a picture read from a
//! file, the outline of its subject, the outline placed in the world by a frame.

pub mod frame;
pub mod outline;
pub mod picture;

use krly3d_core::Error;
use krlyvis_kernel::ports::make::Make;
use krlyvis_kernel::{Datum, VisionBuilder};

pub fn register(builder: &mut VisionBuilder) {
    builder
        .plugin::<Make>(picture::Picture)
        .plugin::<Make>(outline::OutlineTool)
        .plugin::<Make>(frame::FrameTool);
}

/// The one input a tool reads, which must be of the kind it names.
fn single<'a>(inputs: &'a [Datum], tool: &str, wanted: &str) -> Result<&'a Datum, Error> {
    match inputs {
        [datum] => Ok(datum),
        _ => Err(Error::input(format!("`{tool}` reads one {wanted}, got {} inputs", inputs.len()))),
    }
}
