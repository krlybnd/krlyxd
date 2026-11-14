//! Trim rule for flat faces: the winding number of the boundary around the point, in
//! closed form for straight and circular edges.

use super::on_boundary;
use crate::curve::{Circle, Line};
use crate::surface::Plane;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::trim::{Place, Trim, TrimRequest};
use krly3d_kernel::{Context, Plugin};
use nalgebra::Vector2;
use std::f64::consts::TAU;

pub struct PlaneTrim;

impl Plugin<Trim> for PlaneTrim {
    fn id(&self) -> Id {
        Id::new("plane")
    }

    fn call(&self, cx: &Context, request: &TrimRequest) -> Attempt<Place> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        let Some(plane) = face.surface.as_any().downcast_ref::<Plane>() else {
            return Attempt::declined("the plane rule takes a face on a plane");
        };
        if on_boundary(&request.body, face, &request.point, cx.accuracy()) {
            return Attempt::Done(Place::Boundary);
        }
        let flat = |point| plane.frame().inverse_transform_point(&point).xy().coords;
        let eye = flat(request.point);
        // The sum of the angles under which the boundary edges are seen from the point is
        // a whole number of turns: one for a point of the face, none for any other.
        let mut seen = 0.0;
        for used in face.uses() {
            let edge = &request.body.edges[used.edge];
            let (first, last) =
                if used.forward { (edge.range.0, edge.range.1) } else { (edge.range.1, edge.range.0) };
            let (from, to): (Vector2<f64>, Vector2<f64>) =
                (flat(edge.curve.point(first)) - eye, flat(edge.curve.point(last)) - eye);
            let straight = from.perp(&to).atan2(from.dot(&to));
            if edge.curve.as_any().downcast_ref::<Line>().is_some() {
                seen += straight;
            } else if let Some(circle) = edge.curve.as_any().downcast_ref::<Circle>() {
                if !cx.accuracy().parallel(&circle.axis(), &plane.unit_normal()) {
                    return Attempt::declined(
                        "a circular edge of a flat face lies outside the plane of the face",
                    );
                }
                // Seen from inside its circle an arc turns the whole way it runs; seen from
                // outside it only swings back and forth, like the chord between its ends.
                let turning = circle.axis().dot(&plane.unit_normal()) * (last - first);
                let inside = (flat(circle.center()) - eye).norm() < circle.radius();
                seen += match (inside, turning > 0.0) {
                    (false, _) => straight,
                    (true, true) if straight > 0.0 => straight,
                    (true, true) => straight + TAU,
                    (true, false) if straight < 0.0 => straight,
                    (true, false) => straight - TAU,
                };
            } else {
                return Attempt::declined(
                    "a flat face is trimmed in closed form only by straight and circular edges",
                );
            }
        }
        // Counted about the outward normal, a point of an ordinary face is circled once.
        // Loops that enclose negative area describe the plane around a hole instead, and
        // there the points circled are the ones that do not belong.
        let outward = plane.unit_normal() * if face.outward { 1.0 } else { -1.0 };
        let turns = (seen * if face.outward { 1.0 } else { -1.0 } / TAU).round();
        let bounded = request.body.vector_area(face).dot(&outward) > 0.0;
        let inside = if bounded { turns == 1.0 } else { turns == 0.0 };
        Attempt::Done(if inside { Place::Inside } else { Place::Outside })
    }
}
