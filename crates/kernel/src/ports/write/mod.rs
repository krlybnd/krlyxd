//! Port of the `write` family: a document as text of some format. Keyed by format name.

pub mod contract;

use crate::context::Context;
use crate::model::Document;
use crate::plugin::Family;
use krly3d_core::{Attempt, Id};

pub struct Write;

impl Family for Write {
    const NAME: &'static str = "write";
    type Request = Document;
    type Response = String;
    type Context = crate::context::Context;
}

pub fn write(cx: &Context, format: &str, document: &Document) -> Attempt<String> {
    cx.call::<Write>(&Id::new(format), document)
}
