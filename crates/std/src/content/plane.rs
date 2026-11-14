//! Content of a planar face with any boundary.

use super::side;
use crate::surface::Plane;
use krly3d_core::math::frame::origin;
use krly3d_core::{Attempt, Exactness, Id};
use krly3d_kernel::ports::content::{Content, ContentRequest, FaceContent};
use krly3d_kernel::{Context, Plugin};

pub struct PlaneContent;

impl Plugin<Content> for PlaneContent {
    fn id(&self) -> Id {
        Id::new("plane")
    }

    fn call(&self, _: &Context, request: &ContentRequest) -> Attempt<FaceContent> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        let Some(plane) = face.surface.as_any().downcast_ref::<Plane>() else {
            return Attempt::declined("the plane rule takes a face on a plane");
        };
        // The normal is constant, so the area is the vector area along it, and p·n is the
        // same at every point: the height of the plane over the world origin.
        let outward = plane.unit_normal() * side(face.outward);
        let area = request.body.vector_area(face).dot(&outward);
        let height = origin(plane.frame()).coords.dot(&outward);
        Attempt::Done(FaceContent { area, flux: height * area, exact: Exactness::Exact })
    }
}
