//! What a format that is both read and written must satisfy: writing a document and
//! reading the text back gives the same document.

use super::write;
use crate::context::Context;
use crate::model::Document;
use crate::ports::read::read;
use krly3d_core::Attempt;

pub fn check(cx: &Context, format: &str, document: &Document) -> Result<(), String> {
    let text = match write(cx, format, document) {
        Attempt::Done(text) => text,
        other => return Err(format!("`{format}` cannot write the document: {other:?}")),
    };
    match read(cx, format, &text) {
        Attempt::Done(back) if back == *document => Ok(()),
        Attempt::Done(back) => Err(format!("`{format}` reads back a different document:\n{text}\n{back:?}")),
        other => Err(format!("`{format}` cannot read what it wrote: {other:?}\n{text}")),
    }
}
