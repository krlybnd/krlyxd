//! Edit stage: decides whether the edit is kept.

use krly3d_core::{Attempt, Id};
use krly3d_kernel::model::Delta;
use krly3d_kernel::ports::stage::{EditRun, EditStage};
use krly3d_kernel::{Context, Plugin};

pub struct Commit;

impl Plugin<EditStage> for Commit {
    fn id(&self) -> Id {
        Id::new("commit")
    }

    fn call(&self, _: &Context, run: &EditRun) -> Attempt<EditRun> {
        // A dry run has been through every stage before this one and stops here, and so
        // does an edit that an earlier stage blocked.
        let commit = !run.dry && !run.blocked;
        let delta = run.delta.clone().map(|delta| Delta { committed: commit, ..delta });
        Attempt::Done(EditRun { commit, delta, ..run.clone() })
    }
}
