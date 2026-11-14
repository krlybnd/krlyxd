//! Edit stage: builds the model of the changed document.

use krly3d_core::{Attempt, Id};
use krly3d_kernel::model::build;
use krly3d_kernel::ports::stage::{EditRun, EditStage};
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct Build;

impl Plugin<EditStage> for Build {
    fn id(&self) -> Id {
        Id::new("build")
    }

    fn call(&self, cx: &Context, run: &EditRun) -> Attempt<EditRun> {
        match build(cx, &run.after, &run.cache) {
            Ok(model) => Attempt::Done(EditRun { model_after: Some(Arc::new(model)), ..run.clone() }),
            Err(error) => Attempt::Failed(error),
        }
    }
}
