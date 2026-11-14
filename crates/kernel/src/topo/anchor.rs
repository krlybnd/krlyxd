//! A point to stand on: one point of a face, the same every time it is asked for, with
//! the direction that points out of the material there.

use super::body::{Body, Face, FaceId};
use crate::context::Context;
use crate::ports::trim::{place, Place, TrimRequest};
use krly3d_core::math::{Point3, Uv, Vec3};
use krly3d_core::Attempt;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub point: Point3,
    /// Unit normal pointing out of the material at the point.
    pub normal: Vec3,
}

/// A point of the face that the trim rules confirm to lie inside it, not on its boundary.
///
/// The middle of the boundary is tried first, which is the centre of a rectangle or a
/// disc. Where that falls into a hole or outside a bent outline, points between it and
/// the middle of each edge follow, and then points a step into the face from each edge.
pub fn anchor(cx: &Context, body: &Arc<Body>, face: FaceId) -> Attempt<Anchor> {
    let Some(data) = body.faces.get(face) else {
        return Attempt::declined(format!("the body has no face number {face}"));
    };
    let mut unplaced = None;
    for candidate in candidates(body, data, cx.accuracy().length()) {
        let point = data.surface.point(data.surface.project(&candidate));
        match place(cx, &TrimRequest { body: Arc::clone(body), face, point }) {
            Attempt::Done(Place::Inside) => {
                return Attempt::Done(Anchor { point, normal: data.outward_normal(&point) })
            }
            Attempt::Done(_) => {}
            // A point the trim rules cannot place is not one to stand on; the next may be.
            Attempt::Declined(reason) => unplaced = Some(reason),
            Attempt::Failed(error) => return Attempt::Failed(error),
        }
    }
    let reason = unplaced.map_or_else(String::new, |reason| format!(": {reason}"));
    Attempt::declined(format!("no point tried lies inside face `{}`{reason}", data.name))
}

/// The points to try, in order. They need not lie on the surface exactly; the caller
/// projects them.
fn candidates(body: &Body, face: &Face, tolerance: f64) -> impl Iterator<Item = Point3> {
    // Two points placed evenly on every edge: their mean is the centre of a polygon's
    // edge midpoints and of a full circle alike.
    let samples: Vec<Vec3> = face
        .uses()
        .flat_map(|used| [0.25, 0.75].map(|fraction| body.edges[used.edge].point(fraction).coords))
        .collect();
    let centre = if samples.is_empty() {
        face.surface.point(Uv::origin())
    } else {
        Point3::from(samples.iter().sum::<Vec3>() / samples.len() as f64)
    };
    let shores: Vec<(Point3, Vec3)> = face
        .uses()
        .filter_map(|used| {
            let edge = &body.edges[used.edge];
            let middle = edge.point(0.5);
            let travel = edge.curve.tangent(edge.parameter(0.5)) * if used.forward { 1.0 } else { -1.0 };
            // The face lies to the left of its boundary seen from outside.
            let left = face.outward_normal(&middle).cross(&travel).try_normalize(0.0)?;
            Some((middle, left * edge.curve.length(edge.range.0, edge.range.1)))
        })
        .collect();
    let towards_centre: Vec<(Point3, Vec3)> =
        shores.iter().map(|(middle, _)| (*middle, centre - middle)).collect();
    std::iter::once(centre).chain(
        towards_centre
            .into_iter()
            .chain(shores)
            .flat_map(move |(from, reach)| halving(from, reach, tolerance)),
    )
}

/// The points half of `reach` from `from`, then a quarter, an eighth, … for as long as
/// the step is longer than the length tolerance.
fn halving(from: Point3, reach: Vec3, tolerance: f64) -> impl Iterator<Item = Point3> {
    std::iter::successors(Some(reach * 0.5), |step| Some(step * 0.5))
        .take_while(move |step| step.norm() > tolerance)
        .map(move |step| from + step)
}
