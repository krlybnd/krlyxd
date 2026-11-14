//! What every reader must satisfy: a file decodes to the image it holds, and the first
//! half of it is reported as a format error rather than read as something else.

use super::read;
use crate::context::Context;
use crate::image::Gray;
use krly3d_core::{Attempt, ErrorKind};

pub fn check(cx: &Context, format: &str, bytes: &[u8], expected: &Gray) -> Result<(), String> {
    match read(cx, format, bytes.to_vec()) {
        Attempt::Done(image) if &image == expected => {}
        other => return Err(format!("`{format}` read {other:?}, expected {expected:?}")),
    }
    let cut = bytes[..bytes.len() / 2].to_vec();
    match read(cx, format, cut) {
        Attempt::Failed(error) if error.kind == ErrorKind::Format => Ok(()),
        other => Err(format!("`{format}` read a file cut short as {other:?}")),
    }
}
