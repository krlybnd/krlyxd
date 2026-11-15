//! How far a face or a body reaches along the world axes.

use super::body::{Body, FaceId};
use crate::context::Context;
use crate::ports::trim::{place, Place, TrimRequest};
use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::Vec3;
use krly3d_core::Attempt;
use std::sync::Arc;

/// The smallest box around a face: the boxes of its boundary edges, joined with the
/// points where its surface peaks along an axis if those points belong to the face.
/// Exact for any face whose edges give exact boxes.
pub fn face_bounds(cx: &Context, body: &Arc<Body>, face: FaceId) -> Attempt<Bounds> {
    if let Err(error) = cx.check_control() {
        return Attempt::Failed(error);
    }
    let Some(data) = body.faces.get(face) else {
        return Attempt::declined(format!("the body has no face number {face}"));
    };
    let mut bounds = Bounds::empty();
    for used in data.uses() {
        if let Err(error) = cx.check_control() {
            return Attempt::Failed(error);
        }
        let edge = &body.edges[used.edge];
        bounds = bounds.union(&edge.curve.bounds(edge.range.0, edge.range.1));
    }
    for axis in [Vec3::x(), Vec3::y(), Vec3::z()] {
        if let Err(error) = cx.check_control() {
            return Attempt::Failed(error);
        }
        for point in data.surface.extremes(&axis) {
            if let Err(error) = cx.check_control() {
                return Attempt::Failed(error);
            }
            match place(cx, &TrimRequest { body: Arc::clone(body), face, point }) {
                Attempt::Done(Place::Outside) => {}
                Attempt::Done(_) => bounds = bounds.with(&point),
                Attempt::Declined(reason) => return Attempt::Declined(reason),
                Attempt::Failed(error) => return Attempt::Failed(error),
            }
        }
    }
    cx.publish_control(|| bounds).into()
}

/// The smallest box around a body.
pub fn body_bounds(cx: &Context, body: &Arc<Body>) -> Attempt<Bounds> {
    if let Err(error) = cx.check_control() {
        return Attempt::Failed(error);
    }
    let mut bounds = Bounds::empty();
    for face in 0..body.faces.len() {
        if let Err(error) = cx.check_control() {
            return Attempt::Failed(error);
        }
        match face_bounds(cx, body, face) {
            Attempt::Done(of_face) => bounds = bounds.union(&of_face),
            other => return other,
        }
    }
    cx.publish_control(|| bounds).into()
}
