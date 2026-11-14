//! The kernel as its users see it: a document, the model built from it, and one way in
//! for every change.

use crate::context::Context;
use crate::model::{Cache, Delta, Document, Model, NodeRef, Statement, GEOMETRY};
use crate::ports::stage::{EditRun, EditStage};
use crate::topo::Body;
use krly3d_core::Error;
use std::sync::Arc;

pub struct Kernel {
    cx: Context,
    document: Arc<Document>,
    model: Arc<Model>,
    cache: Arc<Cache>,
}

/// A state of the kernel to return to.
#[derive(Clone)]
pub struct Checkpoint {
    document: Arc<Document>,
    model: Arc<Model>,
}

impl Kernel {
    pub fn new(cx: Context) -> Self {
        Self { cx, document: Arc::default(), model: Arc::default(), cache: Arc::default() }
    }

    pub fn context(&self) -> &Context {
        &self.cx
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// The document and the model as shared values, for handing to a query.
    pub fn snapshot(&self) -> (Arc<Document>, Arc<Model>) {
        (Arc::clone(&self.document), Arc::clone(&self.model))
    }

    /// Number of distinct nodes built so far in this session.
    pub fn built_nodes(&self) -> usize {
        self.cache.len()
    }

    /// The body of a node, by name (`part`) or name and version (`part@1`).
    pub fn body(&self, node: &str) -> Result<Arc<Body>, Error> {
        let reference = NodeRef::parse(node)?;
        self.model
            .body(&reference)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("there is no node `{node}`")))
    }

    /// Adds a statement to the document and builds it.
    pub fn apply(&mut self, statement: Statement) -> Result<Delta, Error> {
        let run = self.edit(self.document.with_in(GEOMETRY, statement), false)?;
        self.keep(run)
    }

    /// What `apply` would report, without changing anything.
    pub fn try_out(&self, statement: Statement) -> Result<Delta, Error> {
        let run = self.edit(self.document.with_in(GEOMETRY, statement), true)?;
        run.delta.ok_or_else(no_delta)
    }

    /// Replaces the document.
    pub fn open(&mut self, document: Document) -> Result<Delta, Error> {
        let run = self.edit(document, false)?;
        self.keep(run)
    }

    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint { document: Arc::clone(&self.document), model: Arc::clone(&self.model) }
    }

    pub fn rollback(&mut self, to: Checkpoint) {
        self.document = to.document;
        self.model = to.model;
    }

    /// Passes a changed document through the edit stages of the kernel, in their order.
    fn edit(&self, after: Document, dry: bool) -> Result<EditRun, Error> {
        let mut run = EditRun {
            before: Arc::clone(&self.document),
            after: Arc::new(after),
            model_before: Arc::clone(&self.model),
            model_after: None,
            cache: Arc::clone(&self.cache),
            delta: None,
            dry,
            blocked: false,
            commit: false,
        };
        for stage in self.cx.edit_stages() {
            run = self.cx.call::<EditStage>(stage, &run).into_result()?;
        }
        Ok(run)
    }

    fn keep(&mut self, run: EditRun) -> Result<Delta, Error> {
        if let (true, Some(model)) = (run.commit, &run.model_after) {
            self.document = Arc::clone(&run.after);
            self.model = Arc::clone(model);
        }
        run.delta.ok_or_else(no_delta)
    }
}

fn no_delta() -> Error {
    Error::unsupported("the edit stages reported no delta; is a `delta` stage registered?")
}
