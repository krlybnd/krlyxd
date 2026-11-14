//! What every trim rule must satisfy: the middle of each boundary edge is on the
//! boundary, a small step to the side of the face is inside, a small step to the other
//! side is outside.

use super::{place, Place, TrimRequest};
use crate::context::Context;
use crate::topo::{Body, FaceId};
use krly3d_core::Attempt;
use std::sync::Arc;

/// `step` is how far the test steps off each edge; it must be well below the size of the
/// smallest feature of the face.
pub fn check(cx: &Context, body: &Arc<Body>, face: FaceId, step: f64) -> Result<(), String> {
    let data = body.faces.get(face).ok_or_else(|| format!("the body has no face number {face}"))?;
    for used in data.uses() {
        let edge = &body.edges[used.edge];
        let on = edge.point(0.5);
        let travel = edge.curve.tangent(edge.parameter(0.5)) * if used.forward { 1.0 } else { -1.0 };
        let left = data.outward_normal(&on).cross(&travel).normalize();
        let beside = |offset: f64| data.surface.point(data.surface.project(&(on + left * offset)));
        for (point, expected) in
            [(on, Place::Boundary), (beside(step), Place::Inside), (beside(-step), Place::Outside)]
        {
            match place(cx, &TrimRequest { body: Arc::clone(body), face, point }) {
                Attempt::Done(found) if found == expected => {}
                other => {
                    return Err(format!(
                        "face `{}`: expected {expected:?} at {point:?}, got {other:?}",
                        data.name
                    ))
                }
            }
        }
    }
    Ok(())
}
