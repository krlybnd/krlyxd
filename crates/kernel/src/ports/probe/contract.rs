//! What every ray probe must satisfy: hits are ordered, lie on the faces they name, and
//! along any ray the body is entered and left in turn.

use super::{Hit, Ray};
use crate::topo::Body;
use krly3d_core::Accuracy;

pub fn check(body: &Body, ray: &Ray, hits: &[Hit], accuracy: &Accuracy) -> Result<(), String> {
    if !hits.is_sorted_by(|a, b| a.t <= b.t) {
        return Err(format!("hits are not ordered along the ray: {hits:?}"));
    }
    for hit in hits {
        let face =
            body.faces.get(hit.face).ok_or_else(|| format!("a hit names the missing face {}", hit.face))?;
        if !accuracy.same_point(&hit.point, &(ray.origin + ray.direction * hit.t)) {
            return Err(format!("the hit at t={} is not on the ray", hit.t));
        }
        if !accuracy.is_zero_length(face.surface.distance(&hit.point)) {
            return Err(format!("the hit at t={} is not on face `{}`", hit.t, face.name));
        }
    }
    if body.solid && hits.windows(2).any(|pair| pair[0].entering == pair[1].entering) {
        return Err(format!("the ray enters or leaves the solid twice in a row: {hits:?}"));
    }
    if body.solid && (!hits.len().is_multiple_of(2) || hits.first().is_some_and(|hit| !hit.entering)) {
        return Err(format!(
            "a line through a solid must enter first and leave as often as it enters: {hits:?}"
        ));
    }
    Ok(())
}
