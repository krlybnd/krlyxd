//! Where a stretch of a path on a surface crosses a boundary edge.
//!
//! Lines and circles are flat curves, so a curve meets a circle where it meets the plane
//! of the circle at the circle's radius from its centre. That reduces every case to the
//! `meet` rules that already exist.

use crate::curve::{Circle, Line};
use crate::surface::{Plane, Sphere};
use krly3d_core::math::Point3;
use krly3d_core::{Accuracy, Attempt};
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::ports::meet::{meet, MeetRequest, Meeting};
use krly3d_kernel::ports::surface::{Stretch, Surface};
use krly3d_kernel::topo::{nearest_parameter, Edge};
use krly3d_kernel::Context;
use std::sync::Arc;

/// A point shared by a stretch and an edge.
pub(crate) struct Crossing {
    /// How far along the stretch, from 0 at its start to 1 at its end.
    pub fraction: f64,
    /// Parameter of the point on the curve of the edge.
    pub parameter: f64,
    pub point: Point3,
}

/// Why the crossings of a stretch and an edge are not a list of points.
#[derive(Debug)]
pub(crate) enum Uncrossable {
    /// The two run along the same curve.
    Along,
    /// No rule says where curves of these two kinds cross.
    NoRule(String),
}

impl Uncrossable {
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::Along => "the path runs along an edge".to_string(),
            Self::NoRule(reason) => reason.clone(),
        }
    }
}

pub(crate) fn crossings(cx: &Context, stretch: &Stretch, edge: &Edge) -> Result<Vec<Crossing>, Uncrossable> {
    let accuracy = cx.accuracy();
    let range = (stretch.from.min(stretch.to), stretch.from.max(stretch.to));
    let mut found = Vec::new();
    for point in shared_points(cx, &stretch.curve, &edge.curve)? {
        let on_stretch = nearest_parameter(stretch.curve.as_ref(), range, &point);
        let on_edge = edge.nearest(&point);
        if accuracy.same_point(&stretch.curve.point(on_stretch), &point)
            && accuracy.same_point(&edge.curve.point(on_edge), &point)
        {
            found.push(Crossing {
                fraction: (on_stretch - stretch.from) / (stretch.to - stretch.from),
                parameter: on_edge,
                point,
            });
        }
    }
    Ok(found)
}

/// The points two whole curves have in common.
fn shared_points(cx: &Context, a: &Arc<dyn Curve>, b: &Arc<dyn Curve>) -> Result<Vec<Point3>, Uncrossable> {
    let circle = |curve: &Arc<dyn Curve>| curve.as_any().downcast_ref::<Circle>().copied();
    let line = |curve: &Arc<dyn Curve>| curve.as_any().downcast_ref::<Line>().copied();
    match (circle(a), circle(b), line(a), line(b)) {
        (Some(circle), ..) => on_circle(cx, b, &circle),
        (_, Some(circle), ..) => on_circle(cx, a, &circle),
        (_, _, Some(first), Some(second)) => Ok(lines_meet(&first, &second, cx.accuracy())),
        _ => Err(Uncrossable::NoRule(format!(
            "there is no rule for where a {} crosses a {}",
            a.classes()[0],
            b.classes()[0]
        ))),
    }
}

fn on_circle(cx: &Context, curve: &Arc<dyn Curve>, circle: &Circle) -> Result<Vec<Point3>, Uncrossable> {
    let met = |surface: Arc<dyn Surface>| match meet(cx, &MeetRequest { curve: Arc::clone(curve), surface }) {
        Attempt::Done(meeting) => Ok(meeting),
        Attempt::Declined(reason) => Err(Uncrossable::NoRule(reason)),
        Attempt::Failed(error) => Err(Uncrossable::NoRule(error.to_string())),
    };
    let points = |parameters: Vec<f64>| parameters.into_iter().map(|t| curve.point(t)).collect::<Vec<_>>();
    match met(Arc::new(Plane::new(*circle.frame())))? {
        Meeting::At { parameters, .. } => {
            let at_radius = |point: &Point3| {
                cx.accuracy().is_zero_length((point - circle.center()).norm() - circle.radius())
            };
            Ok(points(parameters).into_iter().filter(at_radius).collect())
        }
        // A curve in the plane of the circle meets the circle where it meets the sphere
        // with the same centre and radius.
        Meeting::Within => match met(Arc::new(
            Sphere::new(*circle.frame(), circle.radius())
                .map_err(|error| Uncrossable::NoRule(error.to_string()))?,
        ))? {
            Meeting::At { parameters, .. } => Ok(points(parameters)),
            Meeting::Within => Err(Uncrossable::Along),
        },
    }
}

/// The point two straight lines share, if they cross. Parallel lines do not cross; lines
/// that coincide are left to the caller, who meets them as a path running along an edge.
fn lines_meet(a: &Line, b: &Line, accuracy: &Accuracy) -> Vec<Point3> {
    let (d, e) = (a.direction(), b.direction());
    if accuracy.parallel(&d, &e) {
        return Vec::new();
    }
    let (normal, between) = (d.cross(&e), b.origin() - a.origin());
    let on_a = a.point(between.cross(&e).dot(&normal) / normal.norm_squared());
    let on_b = b.point(between.cross(&d).dot(&normal) / normal.norm_squared());
    if accuracy.same_point(&on_a, &on_b) {
        vec![on_a]
    } else {
        Vec::new()
    }
}
