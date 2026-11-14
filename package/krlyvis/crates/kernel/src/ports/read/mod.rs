//! Port of the `read` family: the bytes of an image file into a grey image. Keyed by
//! format name.

pub mod contract;

use crate::context::Context;
use crate::image::Gray;
use crate::plugin::Family;
use krly3d_core::{Attempt, Id};

pub struct Read;

impl Family for Read {
    type Context = crate::context::Context;
    const NAME: &'static str = "read";
    type Request = Vec<u8>;
    type Response = Gray;
}

pub fn read(cx: &Context, format: &str, bytes: Vec<u8>) -> Attempt<Gray> {
    cx.call::<Read>(&Id::new(format), &bytes)
}
