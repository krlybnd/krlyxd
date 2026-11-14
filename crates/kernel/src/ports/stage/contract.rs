//! What every node stage must satisfy: it is a function of the run it is given, so
//! passing its own result through it again changes nothing.

use super::{NodeRun, NodeStage};
use crate::context::Context;
use krly3d_core::{Attempt, Id};

pub fn check(cx: &Context, stage: &Id, run: &NodeRun) -> Result<(), String> {
    let once = match cx.call::<NodeStage>(stage, run) {
        Attempt::Done(once) => once,
        other => return Err(format!("stage `{stage}` did not finish: {:?}", other.map(|run| run.node))),
    };
    let twice = match cx.call::<NodeStage>(stage, &once) {
        Attempt::Done(twice) => twice,
        other => {
            return Err(format!("stage `{stage}` rejects its own result: {:?}", other.map(|run| run.node)))
        }
    };
    let names = |run: &NodeRun| {
        run.body.as_ref().map(|body| body.faces.iter().map(|face| face.name.clone()).collect::<Vec<_>>())
    };
    if names(&once) == names(&twice) {
        Ok(())
    } else {
        Err(format!("stage `{stage}` changes its own result when run again"))
    }
}
