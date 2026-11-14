//! Port of the `stage` family: one step of a pipeline.
//!
//! There are two pipelines, each an ordered list of stage names kept by the kernel:
//! node stages turn one operation into a body, edit stages turn a changed document into
//! a model, a delta and a decision to keep it. New behaviour — naming rules, checks,
//! watchers, timing — is a new stage, never a change to an existing one.

pub mod contract;

use crate::model::{Cache, Delta, Document, Model};
use crate::plugin::Family;
use crate::ports::shape::Shape;
use crate::schema::Params;
use crate::topo::Body;
use krly3d_core::Id;
use std::sync::Arc;

pub struct NodeStage;

impl Family for NodeStage {
    const NAME: &'static str = "node stage";
    type Request = NodeRun;
    type Response = NodeRun;
    type Context = crate::context::Context;
}

/// One operation on its way to becoming a body.
#[derive(Clone, Debug)]
pub struct NodeRun {
    pub node: String,
    pub tool: Id,
    pub params: Params,
    /// The input bodies, in world coordinates.
    pub inputs: Vec<Arc<Body>>,
    /// The names of the nodes the inputs come from, in the same order.
    pub sources: Vec<String>,
    pub body: Option<Arc<Body>>,
    /// The node as a shape, when a stage made one; a body alone is its own B-rep leaf.
    pub shape: Option<Arc<dyn Shape>>,
}

pub struct EditStage;

impl Family for EditStage {
    const NAME: &'static str = "edit stage";
    type Request = EditRun;
    type Response = EditRun;
    type Context = crate::context::Context;
}

/// One edit on its way to being kept or dropped.
#[derive(Clone)]
pub struct EditRun {
    pub before: Arc<Document>,
    pub after: Arc<Document>,
    pub model_before: Arc<Model>,
    pub model_after: Option<Arc<Model>>,
    pub cache: Arc<Cache>,
    pub delta: Option<Delta>,
    /// A dry run goes through every stage but is never kept.
    pub dry: bool,
    /// Set by a stage that finds a reason not to keep the edit.
    pub blocked: bool,
    pub commit: bool,
}
