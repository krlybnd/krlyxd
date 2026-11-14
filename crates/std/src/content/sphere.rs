//! Content of a spherical face bounded by circles, by the Gauss–Bonnet theorem.

use super::{direction, side};
use crate::curve::Circle;
use crate::surface::Sphere;
use krly3d_core::math::frame::origin;
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::{Attempt, Exactness, Id};
use krly3d_kernel::ports::content::{Content, ContentRequest, FaceContent};
use krly3d_kernel::topo::{Body, Use};
use krly3d_kernel::{Context, Plugin};
use std::f64::consts::TAU;

pub struct SphereContent;

impl Plugin<Content> for SphereContent {
    fn id(&self) -> Id {
        Id::new("sphere")
    }

    fn call(&self, _: &Context, request: &ContentRequest) -> Attempt<FaceContent> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        let Some(sphere) = face.surface.as_any().downcast_ref::<Sphere>() else {
            return Attempt::declined("the sphere rule takes a face on a sphere");
        };
        // Gauss–Bonnet: Area/r² = 2π·χ − (turning of the boundary), where a region of a
        // sphere with b boundary loops has χ = 2 − b. Along a circle the turning is the
        // angle run times the height of the circle's plane over the centre, divided by r;
        // at a corner it is the exterior angle.
        let (center, radius, sigma) = (origin(sphere.frame()), sphere.radius(), side(face.outward));
        let mut turning = 0.0;
        for uses in &face.loops {
            for (index, used) in uses.iter().enumerate() {
                let edge = &request.body.edges[used.edge];
                let Some(circle) = edge.curve.as_any().downcast_ref::<Circle>() else {
                    return Attempt::declined("a sphere face has a closed-form content only between circles");
                };
                let height = (circle.center() - center).dot(&circle.axis());
                turning += sigma * height / radius * direction(used) * (edge.range.1 - edge.range.0);
                if edge.ends.is_some() {
                    let next = &uses[(index + 1) % uses.len()];
                    let (arriving, corner) = end_of(&request.body, used, true);
                    let (leaving, _) = end_of(&request.body, next, false);
                    let out = face.outward_normal(&corner);
                    turning += arriving.cross(&leaving).dot(&out).atan2(arriving.dot(&leaving));
                }
            }
        }
        let area = radius * radius * (TAU * (2.0 - face.loops.len() as f64) - turning);
        // p = centre + r·n, so p·n_out = centre·n_out + σ·r.
        let flux = center.coords.dot(&request.body.vector_area(face)) + sigma * radius * area;
        Attempt::Done(FaceContent { area, flux, exact: Exactness::Exact })
    }
}

/// Direction of travel and position where a use ends (`last`) or starts.
fn end_of(body: &Body, used: &Use, last: bool) -> (Vec3, Point3) {
    let edge = &body.edges[used.edge];
    let parameter = if used.forward == last { edge.range.1 } else { edge.range.0 };
    (edge.curve.tangent(parameter) * direction(used), edge.curve.point(parameter))
}
