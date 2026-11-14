//! Content of a cylindrical face bounded by cross-section circles and straight lines.

use super::{direction, side};
use crate::curve::{Circle, Line};
use crate::surface::Cylinder;
use krly3d_core::math::frame::{origin, z_axis};
use krly3d_core::{Attempt, Exactness, Id};
use krly3d_kernel::ports::content::{Content, ContentRequest, FaceContent};
use krly3d_kernel::{Context, Plugin};

pub struct CylinderContent;

impl Plugin<Content> for CylinderContent {
    fn id(&self) -> Id {
        Id::new("cylinder")
    }

    fn call(&self, _: &Context, request: &ContentRequest) -> Attempt<FaceContent> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        let Some(cylinder) = face.surface.as_any().downcast_ref::<Cylinder>() else {
            return Attempt::declined("the cylinder rule takes a face on a cylinder");
        };
        // Area = ∬ r du dv = −∮ r·v du around the boundary (Green, integrated in the
        // height v so that nothing depends on where the angle u wraps). Straight edges
        // are rulings, along which u is constant.
        let axis = z_axis(cylinder.frame());
        let mut height_times_turn = 0.0;
        for used in face.uses() {
            let edge = &request.body.edges[used.edge];
            if let Some(circle) = edge.curve.as_any().downcast_ref::<Circle>() {
                let height = cylinder.frame().inverse_transform_point(&circle.center()).z;
                let turn =
                    circle.axis().dot(&axis).signum() * direction(used) * (edge.range.1 - edge.range.0);
                height_times_turn += height * turn;
            } else if edge.curve.as_any().downcast_ref::<Line>().is_none() {
                return Attempt::declined(
                    "a cylinder face has a closed-form content only between circles and straight lines",
                );
            }
        }
        let sigma = side(face.outward);
        let area = -sigma * cylinder.radius() * height_times_turn;
        // p = (point of the axis) + r·n, so p·n_out = (axis point)·n_out + σ·r.
        let flux = origin(cylinder.frame()).coords.dot(&request.body.vector_area(face))
            + sigma * cylinder.radius() * area;
        Attempt::Done(FaceContent { area, flux, exact: Exactness::Exact })
    }
}
