//! What every stage must satisfy: it adds to the scene but never changes the image or
//! the question.

use super::Stage;
use crate::context::Context;
use crate::scene::Scene;
use krly3d_core::{Attempt, Id};
use std::sync::Arc;

pub fn check(cx: &Context, stage: &str, scene: &Scene) -> Result<(), String> {
    match cx.call::<Stage>(&Id::new(stage), scene) {
        Attempt::Done(next) if Arc::ptr_eq(&next.image, &scene.image) && next.ask == scene.ask => Ok(()),
        Attempt::Done(_) => Err(format!("stage `{stage}` changed the image or the question")),
        other => Err(format!("stage `{stage}` did not run: {other:?}")),
    }
}
