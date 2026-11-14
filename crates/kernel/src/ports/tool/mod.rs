//! Port of the `tool` family: an operation that makes a body from parameters and bodies.
//!
//! Every way of creating or changing geometry is a tool, called the same way, described by
//! a schema, and stored as the same kind of record. Tools are keyed by their name.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::schema::Params;
use crate::topo::Body;
use krly3d_core::{Attempt, Id};
use std::sync::Arc;

pub struct Tool;

impl Family for Tool {
    const NAME: &'static str = "tool";
    type Request = ToolRequest;
    type Response = Body;
    type Context = crate::context::Context;
}

pub struct ToolRequest {
    /// Already admitted by the tool's schema: complete and of the right kinds.
    pub params: Params,
    /// The input bodies, in world coordinates.
    pub inputs: Vec<Arc<Body>>,
    /// The names of the nodes the inputs come from, in the same order; empty for a body
    /// that is not a node. A tool uses them to name the faces it takes over.
    pub sources: Vec<String>,
}

/// Runs the tool registered as `tool`: admits the parameters through its schema, then asks
/// its chain. This is how a recipe tool calls the tools it is made of.
pub fn run(cx: &Context, tool: &Id, params: &Params, inputs: &[Arc<Body>]) -> Attempt<Body> {
    run_named(cx, tool, params, inputs, &vec![String::new(); inputs.len()])
}

/// `run` for inputs that are nodes of the model, whose names are `sources`.
pub fn run_named(
    cx: &Context,
    tool: &Id,
    params: &Params,
    inputs: &[Arc<Body>],
    sources: &[String],
) -> Attempt<Body> {
    let Some(schema) = cx.registry::<Tool>().and_then(|registry| registry.schema(tool)) else {
        return Attempt::declined(format!("no tool is registered as `{tool}`"));
    };
    match schema.admit(tool, params, inputs.len()) {
        Ok(params) => {
            cx.call::<Tool>(tool, &ToolRequest { params, inputs: inputs.to_vec(), sources: sources.to_vec() })
        }
        Err(error) => Attempt::Failed(error),
    }
}
