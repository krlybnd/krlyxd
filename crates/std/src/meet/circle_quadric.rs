//! A circle against any quadric: a trigonometric equation of degree two.

use super::all_on;
use crate::curve::Circle;
use krly3d_core::math::frame::{x_axis, y_axis};
use krly3d_core::math::roots::harmonic;
use krly3d_core::{Attempt, Exactness, Id};
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::ports::meet::{Meet, MeetRequest, Meeting};
use krly3d_kernel::{Context, Plugin};
use std::f64::consts::TAU;

pub struct CircleQuadric;

impl Plugin<Meet> for CircleQuadric {
    fn id(&self) -> Id {
        Id::new("circle|quadric")
    }

    fn call(&self, cx: &Context, request: &MeetRequest) -> Attempt<Meeting> {
        let (Some(circle), Some(quadric)) =
            (request.curve.as_any().downcast_ref::<Circle>(), request.surface.quadric())
        else {
            return Attempt::declined("circle|quadric takes a circle and a surface with a quadric form");
        };
        // Along a circle the value of a quadric is a trigonometric polynomial of degree
        // two, which has four zeros per turn at most; five mean it vanishes everywhere.
        let fifths = (0..5).map(|step| circle.point(f64::from(step) * TAU / 5.0));
        if all_on(request.surface.as_ref(), fifths, cx.accuracy()) {
            return Attempt::Done(Meeting::Within);
        }
        let u = x_axis(circle.frame()) * circle.radius();
        let v = y_axis(circle.frame()) * circle.radius();
        let angles = harmonic(quadric.along_conic(&circle.center(), &u, &v));
        Attempt::Done(Meeting::At { parameters: angles.as_slice().to_vec(), exact: Exactness::Exact })
    }
}

#[cfg(test)]
mod tests {
    use crate::curve::Circle;
    use crate::meet::testing::{met, parameters};
    use crate::surface::{Cone, Cylinder, Sphere, Torus};
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Iso3, Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::meet::Meeting;
    use std::f64::consts::PI;

    /// Radius 5 about the origin in the plane spanned by `x` and `y`.
    fn circle(x: Vec3, y: Vec3) -> Circle {
        Circle::new(frame(&Point3::origin(), &x.cross(&y), &x).unwrap(), 5.0).unwrap()
    }

    #[test]
    fn two_great_circles_of_equal_spheres_cross_where_the_spheres_do() {
        // The spheres of radius 5 about the origin and about (6, 0, 0) meet at x = 3, z = ±4.
        let sphere =
            Sphere::new(frame(&Point3::new(6.0, 0.0, 0.0), &Vec3::z(), &Vec3::x()).unwrap(), 5.0).unwrap();

        let crossings = parameters(met(circle(Vec3::x(), Vec3::z()), sphere, (-PI + 0.1, PI - 0.1)));

        let expected = 4.0_f64.atan2(3.0);
        assert!(
            (crossings[0] + expected).abs() < 1e-14 && (crossings[1] - expected).abs() < 1e-14,
            "{crossings:?}"
        );
    }

    #[test]
    fn a_circle_across_a_cylinder_meets_it_four_times() {
        // In the yz plane the circle is 5·|cos θ| away from the z axis: radius 3 at cos θ = ±0.6.
        let cylinder = Cylinder::new(Iso3::identity(), 3.0).unwrap();

        let crossings = parameters(met(circle(Vec3::y(), Vec3::z()), cylinder, (-PI + 0.1, PI - 0.1)));

        let (near, far) = (0.6_f64.acos(), (-0.6_f64).acos());
        let expected = [-far, -near, near, far];
        assert!(
            crossings.len() == 4 && crossings.iter().zip(expected).all(|(c, e)| (c - e).abs() < 1e-14),
            "{crossings:?}"
        );
    }

    #[test]
    fn a_circle_through_both_nappes_of_a_cone_meets_it_four_times() {
        let cone = Cone::new(Iso3::identity(), 0.5).unwrap();

        let crossings = parameters(met(circle(Vec3::y(), Vec3::z()), cone, (-PI + 0.1, PI - 0.1)));

        assert_eq!(crossings.len(), 4, "{crossings:?}");
    }

    #[test]
    fn a_great_circle_of_a_sphere_is_within_it() {
        let sphere = Sphere::new(Iso3::identity(), 5.0).unwrap();

        assert_eq!(met(circle(Vec3::x(), Vec3::z()), sphere, (-PI, PI)), Attempt::Done(Meeting::Within));
    }

    #[test]
    fn a_circle_against_a_torus_has_no_rule_yet() {
        let torus = Torus::new(Iso3::identity(), 6.0, 2.0).unwrap();

        let attempt = met(circle(Vec3::x(), Vec3::z()), torus, (-PI, PI));

        assert_eq!(attempt, Attempt::declined("no meet rule for `circle|torus`"));
    }
}
