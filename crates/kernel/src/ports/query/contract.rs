//! What every query must satisfy: it describes itself, it says something, and asking
//! twice gives the same answer.

use super::{ask, Query};
use crate::context::Context;
use crate::model::{Document, Model};
use crate::schema::Params;
use krly3d_core::{Attempt, Id};
use std::sync::Arc;

pub fn check(
    cx: &Context,
    query: &Id,
    params: &Params,
    document: &Arc<Document>,
    model: &Arc<Model>,
) -> Result<(), String> {
    let schema = cx.registry::<Query>().and_then(|registry| registry.schema(query));
    let schema = schema.ok_or_else(|| format!("query `{query}` has no schema"))?;
    if schema.about.is_empty() || schema.params.iter().any(|param| param.about.is_empty()) {
        return Err(format!("query `{query}` does not say what it or one of its parameters is for"));
    }
    match (ask(cx, query, params, document, model), ask(cx, query, params, document, model)) {
        (Attempt::Done(first), Attempt::Done(second)) if first == second && !first.text.is_empty() => Ok(()),
        (Attempt::Done(first), Attempt::Done(_)) if first.text.is_empty() => {
            Err(format!("query `{query}` answers with no text"))
        }
        (first, second) => {
            Err(format!("query `{query}` answers differently when asked twice: {first:?} then {second:?}"))
        }
    }
}
