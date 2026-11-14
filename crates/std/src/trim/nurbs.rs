//! Trim rule for a NURBS face bounded by two iso-curves of constant `v`, as the side of a
//! loft is: a point belongs to it when its `v` lies between theirs. Decided in parameter
//! space, since the surface's foot point gives `v` directly.

use super::on_boundary;
use crate::surface::Nurbs;
use krly3d_core::math::Uv;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::ports::trim::{Place, Trim, TrimRequest};
use krly3d_kernel::{Context, Plugin};

pub struct NurbsTrim;

impl Plugin<Trim> for NurbsTrim {
    fn id(&self) -> Id {
        Id::new("nurbs")
    }

    fn call(&self, cx: &Context, request: &TrimRequest) -> Attempt<Place> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        let Some(surface) = face.surface.as_any().downcast_ref::<Nurbs>() else {
            return Attempt::declined("the nurbs rule takes a face on a NURBS surface");
        };
        let Some((low, high)) = surface.band(&super::loop_points(&request.body, face)) else {
            return Attempt::declined(
                "a NURBS face is trimmed only between two iso-curves of constant v yet",
            );
        };
        if on_boundary(&request.body, face, &request.point, cx.accuracy()) {
            return Attempt::Done(Place::Boundary);
        }
        let uv = surface.project(&request.point);
        let near = |v: f64| cx.accuracy().same_point(&surface.point(Uv::new(uv.x, v)), &request.point);
        Attempt::Done(if near(low) || near(high) {
            Place::Boundary
        } else if uv.y > low && uv.y < high {
            Place::Inside
        } else {
            Place::Outside
        })
    }
}
