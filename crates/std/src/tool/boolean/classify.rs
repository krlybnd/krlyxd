//! Where a piece of a face lies with respect to the other solid.

use super::work::{settle, Stop};
use krly3d_core::math::{Point3, Uv};
use krly3d_kernel::ports::probe::{side, Side};
use krly3d_kernel::ports::trim::{place, Place, TrimRequest};
use krly3d_kernel::topo::{Body, FaceId};
use krly3d_kernel::Context;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Inside,
    Outside,
    /// On a face of the other solid that looks the same way.
    OnSame,
    /// On a face of the other solid that looks the opposite way.
    OnOpposite,
}

/// A piece is not crossed by the other solid's boundary, so one point of it speaks for
/// all of it.
pub(crate) fn classify(
    cx: &Context,
    pieces: &Arc<Body>,
    piece: FaceId,
    other: &Arc<Body>,
) -> Result<Class, Stop> {
    let point = inner_point(cx, pieces, piece)?;
    match settle(side(cx, other, &point))? {
        Side::Inside => Ok(Class::Inside),
        Side::Outside => Ok(Class::Outside),
        Side::Boundary => {
            let mine = pieces.faces[piece].outward_normal(&point);
            for (index, face) in other.faces.iter().enumerate() {
                let on_it = cx.accuracy().is_zero_length(face.surface.distance(&point))
                    && settle(place(cx, &TrimRequest { body: Arc::clone(other), face: index, point }))?
                        != Place::Outside;
                if on_it && cx.accuracy().parallel(&mine, &face.outward_normal(&point)) {
                    return Ok(if mine.dot(&face.outward_normal(&point)) > 0.0 {
                        Class::OnSame
                    } else {
                        Class::OnOpposite
                    });
                }
            }
            Err(Stop::Declined(format!(
                "a piece of face `{}` meets the other solid where no section was found",
                pieces.faces[piece].name
            )))
        }
    }
}

/// A point strictly inside a piece: beside the middle of one of its edges, as close as it
/// takes for the trim rule to confirm it. A piece without boundary is a whole surface.
fn inner_point(cx: &Context, pieces: &Arc<Body>, piece: FaceId) -> Result<Point3, Stop> {
    let face = &pieces.faces[piece];
    let Some(used) = face.uses().next() else {
        return Ok(face.surface.point(Uv::origin()));
    };
    let edge = &pieces.edges[used.edge];
    let on = edge.point(0.5);
    let travel = edge.curve.tangent(edge.parameter(0.5)) * if used.forward { 1.0 } else { -1.0 };
    let left = face.outward_normal(&on).cross(&travel).normalize();
    let mut step = 0.25 * edge.curve.length(edge.range.0, edge.range.1).min(cx.accuracy().scale());
    while step > cx.accuracy().length() {
        let candidate = face.surface.point(face.surface.project(&(on + left * step)));
        if settle(place(cx, &TrimRequest { body: Arc::clone(pieces), face: piece, point: candidate }))?
            == Place::Inside
        {
            return Ok(candidate);
        }
        step *= 0.5;
    }
    Err(Stop::Declined(format!("no point was found inside a piece of face `{}`", face.name)))
}
