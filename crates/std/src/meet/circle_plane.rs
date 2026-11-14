//! A circle against a plane, in closed form.

use crate::curve::Circle;
use crate::surface::Plane;
use krly3d_core::math::frame::{x_axis, y_axis};
use krly3d_core::math::roots::wrap;
use krly3d_core::{Attempt, Exactness, Id};
use krly3d_kernel::ports::meet::{Meet, MeetRequest, Meeting};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::{Context, Plugin};

pub struct CirclePlane;

impl Plugin<Meet> for CirclePlane {
    fn id(&self) -> Id {
        Id::new("circle|plane")
    }

    fn call(&self, cx: &Context, request: &MeetRequest) -> Attempt<Meeting> {
        let (Some(circle), Some(plane)) = (
            request.curve.as_any().downcast_ref::<Circle>(),
            request.surface.as_any().downcast_ref::<Plane>(),
        ) else {
            return Attempt::declined("circle|plane takes a circle and a plane");
        };
        let accuracy = cx.accuracy();
        let normal = plane.unit_normal();
        let height = plane.distance(&circle.center());
        if accuracy.parallel(&circle.axis(), &normal) {
            return Attempt::Done(if accuracy.is_zero_length(height) {
                Meeting::Within
            } else {
                Meeting::At { parameters: Vec::new(), exact: Exactness::Exact }
            });
        }
        // The height of the circle above the plane is height + amplitude·cos(θ − phase).
        let rise_x = circle.radius() * normal.dot(&x_axis(circle.frame()));
        let rise_y = circle.radius() * normal.dot(&y_axis(circle.frame()));
        let (amplitude, phase) = (rise_x.hypot(rise_y), rise_y.atan2(rise_x));
        let cosine = -height / amplitude;
        let mut parameters = Vec::new();
        if cosine.abs() <= 1.0 {
            let spread = cosine.acos();
            parameters.extend([wrap(phase - spread), wrap(phase + spread)]);
            parameters.sort_by(f64::total_cmp);
        }
        Attempt::Done(Meeting::At { parameters, exact: Exactness::Exact })
    }
}

#[cfg(test)]
mod tests {
    use crate::curve::Circle;
    use crate::meet::testing::{met, parameters};
    use crate::surface::Plane;
    use krly3d_core::math::{Iso3, Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::meet::Meeting;
    use std::f64::consts::{FRAC_PI_3, PI};

    fn circle() -> Circle {
        Circle::new(Iso3::identity(), 2.0).unwrap()
    }

    fn plane_at_x(x: f64) -> Plane {
        Plane::through(&Point3::new(x, 0.0, 0.0), &Vec3::x()).unwrap()
    }

    #[test]
    fn a_plane_through_a_circle_meets_it_at_two_angles() {
        let crossings = parameters(met(circle(), plane_at_x(1.0), (-PI + 0.1, PI - 0.1)));

        assert!(
            (crossings[0] + FRAC_PI_3).abs() < 1e-15 && (crossings[1] - FRAC_PI_3).abs() < 1e-15,
            "{crossings:?}"
        );
    }

    #[test]
    fn a_tangent_plane_touches_at_a_double_angle() {
        let crossings = parameters(met(circle(), plane_at_x(2.0), (-PI + 0.1, PI - 0.1)));

        assert_eq!(crossings, [0.0, 0.0]);
    }

    #[test]
    fn a_plane_beyond_the_circle_does_not_meet_it() {
        assert!(parameters(met(circle(), plane_at_x(5.0), (-PI, PI))).is_empty());
    }

    #[test]
    fn a_circle_in_the_plane_is_within_it() {
        let own_plane = Plane::through(&Point3::origin(), &Vec3::z()).unwrap();

        assert_eq!(met(circle(), own_plane, (-PI, PI)), Attempt::Done(Meeting::Within));
    }

    #[test]
    fn a_parallel_plane_at_a_distance_does_not_meet_the_circle() {
        let lifted = Plane::through(&Point3::new(0.0, 0.0, 1.0), &Vec3::z()).unwrap();

        assert!(parameters(met(circle(), lifted, (-PI, PI))).is_empty());
    }
}
