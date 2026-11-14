//! Port of the `trim` family: whether a point of a face's surface belongs to the face.
//!
//! Rules are keyed by one surface class. A face is a piece of a surface cut out by its
//! boundary; every question about "is this point of the body" — a ray hit, a section, a
//! bounding box — ends here.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::ports::pair;
use crate::topo::{Body, Face, FaceId};
use krly3d_core::math::Point3;
use krly3d_core::{Attempt, Error};
use std::sync::Arc;

pub struct Trim;

impl Family for Trim {
    const NAME: &'static str = "trim";
    type Request = TrimRequest;
    type Response = Place;
    type Context = crate::context::Context;
}

pub struct TrimRequest {
    pub body: Arc<Body>,
    pub face: FaceId,
    /// A point on the surface of the face.
    pub point: Point3,
}

impl TrimRequest {
    pub fn target(&self) -> Result<&Face, Error> {
        self.body
            .faces
            .get(self.face)
            .ok_or_else(|| Error::not_found(format!("the body has no face number {}", self.face)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Inside,
    Outside,
    /// On an edge of the face, within the length tolerance.
    Boundary,
}

/// Asks the trim rules for the classes of the face's surface.
pub fn place(cx: &Context, request: &TrimRequest) -> Attempt<Place> {
    match request.target() {
        Ok(face) => pair::ask_class::<Trim>(cx, face.surface.classes(), request),
        Err(error) => Attempt::Failed(error),
    }
}
