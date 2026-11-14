//! Port of the `content` family: the area of a face and its share of the volume.
//!
//! Rules are keyed by one surface class, e.g. `plane` or `sphere`. By the divergence
//! theorem the volume of a solid is one third of the integral of `p·n` over its faces, so
//! each face contributes its own `flux` and the measure just adds them up.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::ports::pair;
use crate::topo::{Body, Face, FaceId};
use krly3d_core::{Attempt, Error, Exactness};
use std::sync::Arc;

pub struct Content;

impl Family for Content {
    const NAME: &'static str = "content";
    type Request = ContentRequest;
    type Response = FaceContent;
    type Context = crate::context::Context;
}

pub struct ContentRequest {
    pub body: Arc<Body>,
    pub face: FaceId,
}

impl ContentRequest {
    pub fn target(&self) -> Result<&Face, Error> {
        self.body
            .faces
            .get(self.face)
            .ok_or_else(|| Error::not_found(format!("the body has no face number {}", self.face)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceContent {
    pub area: f64,
    /// Integral of `p·n` over the face, with `p` from the world origin and `n` the outward
    /// normal. One third of it is the face's share of the volume.
    pub flux: f64,
    pub exact: Exactness,
}

/// Asks the content rules for the classes of the face's surface.
pub fn content(cx: &Context, request: &ContentRequest) -> Attempt<FaceContent> {
    match request.target() {
        Ok(face) => pair::ask_class::<Content>(cx, face.surface.classes(), request),
        Err(error) => Attempt::Failed(error),
    }
}
