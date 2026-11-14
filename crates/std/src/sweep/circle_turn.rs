//! A circle turned about an axis in its own plane sweeps a sphere or a torus.

use crate::curve::Circle;
use crate::surface::{Sphere, Torus};
use krly3d_core::math::frame::frame;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::ports::sweep::{Motion, Sweep, SweepRequest};
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct CircleTurn;

impl Plugin<Sweep> for CircleTurn {
    fn id(&self) -> Id {
        Id::new("circle|turn")
    }

    fn call(&self, cx: &Context, request: &SweepRequest) -> Attempt<Arc<dyn Surface>> {
        let (Some(circle), Motion::Turn { origin, axis, .. }) =
            (request.curve.as_any().downcast_ref::<Circle>(), request.motion)
        else {
            return Attempt::declined("circle|turn takes a circle and a turn");
        };
        let accuracy = cx.accuracy();
        let (normal, axis) = (circle.axis(), axis.into_inner());
        let offset = circle.center() - origin;
        if !accuracy.perpendicular(&normal, &axis) || !accuracy.is_zero_length(offset.dot(&normal)) {
            return Attempt::declined("only a circle whose plane contains the axis has a sweep rule");
        }
        let height = offset.dot(&axis);
        let radial = offset - axis * height;
        let ring = radial.norm();
        if accuracy.is_zero_length(ring) {
            return frame(&circle.center(), &axis, &axis.cross(&normal))
                .and_then(|placed| Sphere::new(placed, circle.radius()))
                .map(|sphere| Arc::new(sphere) as Arc<dyn Surface>)
                .into();
        }
        if ring <= circle.radius() + accuracy.length() {
            return Attempt::declined(
                "the circle reaches the axis, so the torus it sweeps would cross itself",
            );
        }
        frame(&(origin + axis * height), &axis, &radial)
            .and_then(|placed| Torus::new(placed, ring, circle.radius()))
            .map(|torus| Arc::new(torus) as Arc<dyn Surface>)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use crate::curve::Circle;
    use crate::surface::{Sphere, Torus};
    use crate::sweep::testing::{kind, swept};
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::sweep::Motion;
    use std::f64::consts::TAU;

    /// A circle of radius 2 in the xz plane, centred `distance` from the z axis.
    fn circle(distance: f64) -> Circle {
        Circle::new(frame(&Point3::new(distance, 0.0, 5.0), &Vec3::y(), &Vec3::x()).unwrap(), 2.0).unwrap()
    }

    fn about_z() -> Motion {
        Motion::turn(Point3::origin(), Vec3::z(), TAU).unwrap()
    }

    #[test]
    fn a_circle_centred_on_the_axis_sweeps_a_sphere_of_its_radius() {
        let surface = swept(circle(0.0), about_z());

        let Attempt::Done(surface) = surface else { panic!("{}", kind(&surface)) };
        let sphere = surface.as_any().downcast_ref::<Sphere>().unwrap();
        assert_eq!(sphere.radius(), 2.0);
    }

    #[test]
    fn a_circle_beside_the_axis_sweeps_a_torus() {
        let surface = swept(circle(6.0), about_z());

        let Attempt::Done(surface) = surface else { panic!("{}", kind(&surface)) };
        let torus = surface.as_any().downcast_ref::<Torus>().unwrap();
        assert_eq!((torus.major(), torus.minor()), (6.0, 2.0));
    }

    #[test]
    fn a_circle_crossing_the_axis_is_declined() {
        assert_eq!(kind(&swept(circle(1.0), about_z())), "declined");
    }

    #[test]
    fn a_circle_whose_plane_misses_the_axis_is_declined() {
        let tilted =
            Circle::new(frame(&Point3::new(6.0, 0.0, 5.0), &Vec3::z(), &Vec3::x()).unwrap(), 2.0).unwrap();

        assert_eq!(kind(&swept(tilted, about_z())), "declined");
    }
}
