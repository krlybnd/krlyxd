//! Node stage: runs the tool of the operation.

use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::stage::{NodeRun, NodeStage};
use krly3d_kernel::ports::tool;
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct Run;

impl Plugin<NodeStage> for Run {
    fn id(&self) -> Id {
        Id::new("run")
    }

    fn call(&self, cx: &Context, run: &NodeRun) -> Attempt<NodeRun> {
        tool::run_named(cx, &run.tool, &run.params, &run.inputs, &run.sources)
            .map(|body| NodeRun { body: Some(Arc::new(body)), ..run.clone() })
    }
}
