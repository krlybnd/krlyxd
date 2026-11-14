//! Port of the `read` family: text of some format into a document. Keyed by format name.

use crate::context::Context;
use crate::model::Document;
use crate::plugin::Family;
use krly3d_core::{Attempt, Id};

pub struct Read;

impl Family for Read {
    const NAME: &'static str = "read";
    type Request = String;
    type Response = Document;
    type Context = crate::context::Context;
}

pub fn read(cx: &Context, format: &str, text: &str) -> Attempt<Document> {
    cx.call::<Read>(&Id::new(format), &text.to_string())
}
