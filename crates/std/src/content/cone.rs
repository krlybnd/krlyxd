//! Content of a conical face bounded by cross-section circles and straight lines.

use super::{direction, side};
use crate::curve::{Circle, Line};
use crate::surface::Cone;
use krly3d_core::math::frame::{origin, z_axis};
use krly3d_core::{Attempt, Exactness, Id};
use krly3d_kernel::ports::content::{Content, ContentRequest, FaceContent};
use krly3d_kernel::{Context, Plugin};

pub struct ConeContent;

impl Plugin<Content> for ConeContent {
    fn id(&self) -> Id {
        Id::new("cone")
    }

    fn call(&self, _: &Context, request: &ContentRequest) -> Attempt<FaceContent> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        let Some(cone) = face.surface.as_any().downcast_ref::<Cone>() else {
            return Attempt::declined("the cone rule takes a face on a cone");
        };
        // With v the distance from the apex along the surface, dA = |v|·sin α du dv, so
        // Area = −∮ ½·sin α·v·|v| du. The antiderivative vanishes at the apex, which is
        // why a face that contains the apex needs no special treatment.
        let (sin, cos) = cone.half_angle().sin_cos();
        let axis = z_axis(cone.frame());
        let mut swept = 0.0;
        for used in face.uses() {
            let edge = &request.body.edges[used.edge];
            if let Some(circle) = edge.curve.as_any().downcast_ref::<Circle>() {
                let slant = cone.frame().inverse_transform_point(&circle.center()).z / cos;
                let turn =
                    circle.axis().dot(&axis).signum() * direction(used) * (edge.range.1 - edge.range.0);
                swept += 0.5 * sin * slant * slant.abs() * turn;
            } else if edge.curve.as_any().downcast_ref::<Line>().is_none() {
                return Attempt::declined(
                    "a cone face has a closed-form content only between circles and straight lines",
                );
            }
        }
        let area = -side(face.outward) * swept;
        // Every point of a cone is the apex plus a vector along the surface, so p·n is
        // the same as apex·n: the flux needs no area term at all.
        let flux = origin(cone.frame()).coords.dot(&request.body.vector_area(face));
        Attempt::Done(FaceContent { area, flux, exact: Exactness::Exact })
    }
}
