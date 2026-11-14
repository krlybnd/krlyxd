//! Stages: the steps of the two pipelines, and the order the standard kernel runs them in.

pub mod build;
pub mod check;
pub mod commit;
pub mod delta;
pub mod judge;
pub mod name;
pub mod run;

use krly3d_kernel::ports::stage::{EditStage, NodeStage};
use krly3d_kernel::KernelBuilder;

pub fn register(builder: &mut KernelBuilder) {
    builder
        .plugin::<NodeStage>(run::Run)
        .plugin::<NodeStage>(name::Name)
        .plugin::<NodeStage>(check::Check)
        .plugin::<EditStage>(build::Build)
        .plugin::<EditStage>(delta::Diff)
        .plugin::<EditStage>(judge::Judge)
        .plugin::<EditStage>(commit::Commit)
        .node_stages(&["run", "name", "check"])
        .edit_stages(&["build", "delta", "judge", "commit"]);
}
