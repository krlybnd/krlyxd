//! What every key family must satisfy: its keys carry its prefix, can be written in a
//! search, and are the same when asked for again; the faces it relates a face to exist
//! and are other faces.

use super::{Keys, KeysRequest};
use crate::context::Context;
use krly3d_core::{Attempt, Id};

pub fn check(cx: &Context, prefix: &Id, request: &KeysRequest) -> Result<(), String> {
    let (first, second) = (cx.call::<Keys>(prefix, request), cx.call::<Keys>(prefix, request));
    if first != second {
        return Err(format!("key family `{prefix}` answers differently when asked twice"));
    }
    let found = match first {
        Attempt::Done(found) => found,
        // Nothing to say about this face.
        Attempt::Declined(_) => return Ok(()),
        Attempt::Failed(error) => return Err(format!("key family `{prefix}` failed: {error}")),
    };
    for (position, key) in found.keys.iter().enumerate() {
        let value = key.strip_prefix(prefix.as_str()).and_then(|rest| rest.strip_prefix(':'));
        if value.is_none_or(str::is_empty) {
            return Err(format!("key `{key}` does not read `{prefix}:value`"));
        }
        // A search is split at spaces and reads `^` as the start of a weight.
        if key.contains(|c: char| c.is_whitespace() || c == '^') {
            return Err(format!("key `{key}` cannot be written in a search"));
        }
        if found.keys[..position].contains(key) {
            return Err(format!("key `{key}` is stated twice for one face"));
        }
    }
    if let Some(related) = found
        .related
        .iter()
        .find(|related| **related == request.face || **related >= request.body.faces.len())
    {
        return Err(format!(
            "key family `{prefix}` relates face {} to {related}, which is itself or no face",
            request.face
        ));
    }
    Ok(())
}
