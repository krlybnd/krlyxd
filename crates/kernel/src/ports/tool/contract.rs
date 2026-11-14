//! What every tool must satisfy: it describes itself, and what it makes is a valid body.

use crate::context::Context;
use crate::ports::tool::Tool;
use crate::topo::{check, Body};
use krly3d_core::Id;

pub fn check_result(cx: &Context, tool: &Id, body: &Body) -> Result<(), String> {
    let schema = cx
        .registry::<Tool>()
        .and_then(|registry| registry.schema(tool))
        .ok_or_else(|| format!("tool `{tool}` has no schema"))?;
    if schema.about.is_empty() || schema.params.iter().any(|param| param.about.is_empty()) {
        return Err(format!("tool `{tool}` does not say what it or one of its parameters is for"));
    }
    check(body, cx.accuracy()).map_err(|error| format!("tool `{tool}` made an invalid body: {error}"))
}
