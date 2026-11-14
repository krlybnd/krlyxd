//! Content of a whole torus, of the part of one between two cross-section circles, or of
//! the band of one between two circles around its axis.

use super::{direction, side};
use crate::curve::Circle;
use crate::surface::Torus;
use krly3d_core::math::frame::{origin, z_axis};
use krly3d_core::math::Uv;
use krly3d_core::{Accuracy, Attempt, Exactness, Id};
use krly3d_kernel::ports::content::{Content, ContentRequest, FaceContent};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::topo::{Body, Face};
use krly3d_kernel::{Context, Plugin};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

pub struct TorusContent;

impl Plugin<Content> for TorusContent {
    fn id(&self) -> Id {
        Id::new("torus")
    }

    fn call(&self, cx: &Context, request: &ContentRequest) -> Attempt<FaceContent> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        let Some(torus) = face.surface.as_any().downcast_ref::<Torus>() else {
            return Attempt::declined("the torus rule takes a face on a torus");
        };
        if let Some((from, span)) = band_between_parallels(&request.body, face, torus, cx.accuracy()) {
            return Attempt::Done(band(&request.body, face, torus, from, span));
        }
        // The face spans the angle `span` about the axis and the whole way around the tube.
        let span = if face.loops.is_empty() {
            TAU
        } else {
            match span_between_tube_circles(&request.body, face, torus, cx.accuracy()) {
                Some(span) => span,
                None => {
                    return Attempt::declined(
                        "a torus face has a closed-form content only when whole, cut by two circles around the tube, or by two circles around the axis",
                    )
                }
            }
        };
        let (ring, tube, sigma) = (torus.major(), torus.minor(), side(face.outward));
        // dA = r·(R + r·cos v) du dv over one turn of v: the cosine term integrates away.
        let area = span * TAU * tube * ring;
        // p = axis point + R·e(u) + r·n gives p·n = (axis point)·n + R·cos v + r, and
        // ∬ cos v dA = span·π·r².
        let cosine = span * PI * tube * tube;
        let flux = origin(torus.frame()).coords.dot(&request.body.vector_area(face))
            + sigma * (tube * area + ring * cosine);
        Attempt::Done(FaceContent { area, flux, exact: Exactness::Exact })
    }
}

/// The content of the band all the way round the axis from the tube angle `from` through
/// `span`. With dA = r·(R + r·cos v) du dv, the area is 2πr·(R·Δv + r·Δsin v), and p·n =
/// (axis point)·n + R·cos v + r as for the whole torus, with
/// ∬ cos v dA = 2πr·(R·Δsin v + r·Δ(v/2 + sin 2v / 4)).
fn band(body: &Body, face: &Face, torus: &Torus, from: f64, span: f64) -> FaceContent {
    let (ring, tube, sigma) = (torus.major(), torus.minor(), side(face.outward));
    let to = from + span;
    let rise = to.sin() - from.sin();
    let area = TAU * tube * (ring * span + tube * rise);
    let half = |v: f64| 0.5 * v + 0.25 * (2.0 * v).sin();
    let cosine = TAU * tube * (ring * rise + tube * (half(to) - half(from)));
    let flux =
        origin(torus.frame()).coords.dot(&body.vector_area(face)) + sigma * (tube * area + ring * cosine);
    FaceContent { area, flux, exact: Exactness::Exact }
}

/// For a face bounded by two whole circles around the axis of the torus: the tube angle
/// where it starts and how far it reaches in the sense of increasing `v`. Seen from
/// outside the face lies to the left of its boundary; at the circle it starts from, that
/// is the way `v` increases.
fn band_between_parallels(
    body: &Body,
    face: &Face,
    torus: &Torus,
    accuracy: &Accuracy,
) -> Option<(f64, f64)> {
    if face.loops.len() != 2 {
        return None;
    }
    let axis = z_axis(torus.frame());
    let (mut start, mut end) = (None, None);
    for uses in &face.loops {
        let [used] = uses.as_slice() else { return None };
        let edge = &body.edges[used.edge];
        let circle = edge.curve.as_any().downcast_ref::<Circle>()?;
        let around_axis = edge.ends.is_none()
            && accuracy.parallel(&circle.axis(), &axis)
            && accuracy.is_zero_length((circle.center() - origin(torus.frame())).cross(&axis).norm());
        if !around_axis {
            return None;
        }
        let point = edge.curve.point(edge.range.0);
        let at = torus.project(&point);
        let travel = edge.curve.tangent(edge.range.0) * direction(used);
        let left = face.outward_normal(&point).cross(&travel);
        // The chord between a quarter turn of the tube before and after is along its tangent.
        let along_v =
            torus.point(Uv::new(at.x, at.y + FRAC_PI_2)) - torus.point(Uv::new(at.x, at.y - FRAC_PI_2));
        let slot = if left.dot(&along_v) > 0.0 { &mut start } else { &mut end };
        if slot.replace(at.y).is_some() {
            return None;
        }
    }
    let (start, end) = (start?, end?);
    Some((start, (end - start).rem_euclid(TAU))).filter(|(_, span)| *span > 0.0)
}

/// The angle about the axis between the two boundary circles, measured across the face.
/// Seen with the outward side up, the face lies to the left of its boundary, so the
/// circle run in the sense of increasing `v` is the one at the larger `u`.
fn span_between_tube_circles(body: &Body, face: &Face, torus: &Torus, accuracy: &Accuracy) -> Option<f64> {
    let axis = z_axis(torus.frame());
    let (mut upper, mut lower) = (None, None);
    for uses in &face.loops {
        let [used] = uses.as_slice() else { return None };
        let edge = &body.edges[used.edge];
        let circle = edge.curve.as_any().downcast_ref::<Circle>()?;
        let around_tube = edge.ends.is_none()
            && accuracy.perpendicular(&circle.axis(), &axis)
            && accuracy.is_zero_length(circle.radius() - torus.minor());
        if !around_tube {
            return None;
        }
        // On a circle the chord between the points a quarter turn before and after a
        // point is parallel to the tangent there: it shows which way v increases.
        let at = torus.project(&edge.curve.point(edge.range.0));
        let quarter = |turns: f64| torus.point(Uv::new(at.x, at.y + turns * FRAC_PI_2));
        let along_v = edge.curve.tangent(edge.range.0).dot(&(quarter(1.0) - quarter(-1.0)));
        let rising = along_v * direction(used) * side(face.outward) > 0.0;
        let slot = if rising { &mut upper } else { &mut lower };
        if slot.replace(at.x).is_some() {
            return None;
        }
    }
    Some((upper? - lower?).rem_euclid(TAU)).filter(|span| *span > 0.0)
}
