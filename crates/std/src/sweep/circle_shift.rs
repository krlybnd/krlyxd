//! A circle shifted along its axis sweeps a cylinder.

use crate::curve::Circle;
use crate::surface::Cylinder;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::ports::sweep::{Motion, Sweep, SweepRequest};
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct CircleShift;

impl Plugin<Sweep> for CircleShift {
    fn id(&self) -> Id {
        Id::new("circle|shift")
    }

    fn call(&self, cx: &Context, request: &SweepRequest) -> Attempt<Arc<dyn Surface>> {
        let (Some(circle), Motion::Shift { by }) =
            (request.curve.as_any().downcast_ref::<Circle>(), request.motion)
        else {
            return Attempt::declined("circle|shift takes a circle and a shift");
        };
        if !cx.accuracy().parallel(&circle.axis(), &by) {
            return Attempt::declined("a circle shifted off its axis sweeps an elliptic cylinder, for which there is no surface plugin");
        }
        Cylinder::new(*circle.frame(), circle.radius())
            .map(|cylinder| Arc::new(cylinder) as Arc<dyn Surface>)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use crate::curve::Circle;
    use crate::surface::Cylinder;
    use crate::sweep::testing::{kind, swept};
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::sweep::Motion;

    fn circle() -> Circle {
        Circle::new(frame(&Point3::new(1.0, 0.0, 2.0), &Vec3::y(), &Vec3::x()).unwrap(), 3.0).unwrap()
    }

    #[test]
    fn a_circle_shifted_along_its_axis_sweeps_a_cylinder_of_its_radius() {
        let surface = swept(circle(), Motion::shift(Vec3::new(0.0, -8.0, 0.0)).unwrap());

        let Attempt::Done(surface) = surface else { panic!("{}", kind(&surface)) };
        let cylinder = surface.as_any().downcast_ref::<Cylinder>().unwrap();
        assert_eq!(cylinder.radius(), 3.0);
    }

    #[test]
    fn a_circle_shifted_off_its_axis_is_declined() {
        let surface = swept(circle(), Motion::shift(Vec3::new(1.0, 5.0, 0.0)).unwrap());

        assert_eq!(kind(&surface), "declined");
    }
}
