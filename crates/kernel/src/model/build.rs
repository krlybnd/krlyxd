//! Building bodies from a document, remembering every node by what it was made from.

use crate::context::Context;
use crate::ports::shape::{Leaf, Shape};
use crate::ports::stage::{NodeRun, NodeStage};
use crate::topo::Body;
use krly3d_core::document::{self, Document, Key, Keyed, NodeRef, Statement, Versions};
use krly3d_core::Error;
use std::sync::Arc;

/// What a node was made into: its shape and the B-rep reading of it.
pub type Made = (Arc<dyn Shape>, Arc<Body>);

/// Built nodes by key, shared by every document built in a session.
pub type Cache = document::Cache<Made>;

#[derive(Clone, Debug)]
pub struct Built {
    /// The node as a shape: the foundation every reading starts from.
    pub shape: Arc<dyn Shape>,
    /// Its B-rep reading.
    pub body: Arc<Body>,
    pub key: Key,
    /// The bodies the tool was given, in the order of the statement.
    pub inputs: Vec<Arc<Body>>,
}

impl Keyed for Built {
    fn key(&self) -> Key {
        self.key
    }
}

/// Every version of every node of a document.
#[derive(Clone, Debug, Default)]
pub struct Model {
    versions: Versions<Built>,
}

impl Model {
    pub fn built(&self, node: &NodeRef) -> Option<&Built> {
        self.versions.get(node)
    }

    pub fn body(&self, node: &NodeRef) -> Option<&Arc<Body>> {
        self.built(node).map(|built| &built.body)
    }

    pub fn shape(&self, node: &NodeRef) -> Option<&Arc<dyn Shape>> {
        self.built(node).map(|built| &built.shape)
    }

    /// Node names with the number of versions each has, in name order.
    pub fn nodes(&self) -> impl Iterator<Item = (&str, usize)> {
        self.versions.nodes()
    }
}

/// Builds every statement of the document in order (see `krly3d_core::document::build`).
pub fn build(cx: &Context, document: &Document, cache: &Cache) -> Result<Model, Error> {
    let bodies = |inputs: &[&Built]| inputs.iter().map(|built| Arc::clone(&built.body)).collect::<Vec<_>>();
    let versions = document::build(
        document.statements(),
        cache,
        |statement, inputs| run_node(cx, statement, bodies(inputs)),
        |(shape, body), key, inputs| Built { shape, body, key, inputs: bodies(inputs) },
    )?;
    Ok(Model { versions })
}

/// Passes a node through the node stages of the kernel, in their order.
fn run_node(cx: &Context, statement: &Statement, inputs: Vec<Arc<Body>>) -> Result<Made, Error> {
    let op = &statement.op;
    let sources = op.inputs.iter().map(|input| input.name.clone()).collect();
    let mut run = NodeRun {
        node: statement.name.clone(),
        tool: op.tool.clone(),
        params: op.params.clone(),
        inputs,
        sources,
        body: None,
        shape: None,
    };
    for stage in cx.node_stages() {
        run = cx.call::<NodeStage>(stage, &run).into_result()?;
    }
    let body = run
        .body
        .ok_or_else(|| Error::unsupported("the node stages made no body; is a `run` stage registered?"))?;
    let shape = run.shape.unwrap_or_else(|| Leaf::of(Arc::clone(&body)));
    Ok((shape, body))
}
