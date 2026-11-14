//! Node stage: refuses a body that is not valid.

use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::stage::{NodeRun, NodeStage};
use krly3d_kernel::topo;
use krly3d_kernel::{Context, Plugin};

pub struct Check;

impl Plugin<NodeStage> for Check {
    fn id(&self) -> Id {
        Id::new("check")
    }

    fn call(&self, cx: &Context, run: &NodeRun) -> Attempt<NodeRun> {
        let Some(body) = &run.body else {
            return Attempt::Failed(Error::unsupported(
                "the `check` stage needs a body; put `run` before it",
            ));
        };
        match topo::check(body, cx.accuracy()) {
            Ok(()) => Attempt::Done(run.clone()),
            Err(error) => Attempt::Failed(error),
        }
    }
}
