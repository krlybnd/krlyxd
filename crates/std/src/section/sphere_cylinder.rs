//! A sphere centred on the axis of a cylinder crosses it in two circles.

use super::{circle, curves, none, touching};
use crate::surface::{Cylinder, Sphere};
use krly3d_core::math::frame::{origin, z_axis};
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::section::{Cut, Section, SectionRequest};
use krly3d_kernel::{Context, Plugin};

pub struct SphereCylinder;

impl Plugin<Section> for SphereCylinder {
    fn id(&self) -> Id {
        Id::new("sphere|cylinder")
    }

    fn call(&self, cx: &Context, request: &SectionRequest) -> Attempt<Cut> {
        let (Some(sphere), Some(cylinder)) = (
            request.first.as_any().downcast_ref::<Sphere>(),
            request.second.as_any().downcast_ref::<Cylinder>(),
        ) else {
            return Attempt::declined("sphere|cylinder takes a sphere and a cylinder");
        };
        let accuracy = cx.accuracy();
        let (center, axis) = (origin(sphere.frame()), z_axis(cylinder.frame()));
        let off_axis = (center - origin(cylinder.frame())).cross(&axis).norm();
        if !accuracy.is_zero_length(off_axis) {
            return Attempt::declined("a sphere off the axis of a cylinder cuts a curve of degree four, for which there is no curve plugin");
        }
        if accuracy.is_zero_length(sphere.radius() - cylinder.radius()) {
            return touching(circle(center, axis, cylinder.radius()).map(|equator| vec![equator]));
        }
        if sphere.radius() < cylinder.radius() {
            return none();
        }
        // The cylinder leaves the sphere √(R² − r²) above and below its centre.
        let up = axis * (sphere.radius() * sphere.radius() - cylinder.radius() * cylinder.radius()).sqrt();
        let above = circle(center + up, axis, cylinder.radius());
        curves(above.and_then(|above| Ok(vec![above, circle(center - up, axis, cylinder.radius())?])))
    }
}

#[cfg(test)]
mod tests {
    use crate::section::testing::{cut, summaries};
    use crate::surface::{Cylinder, Sphere};
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Iso3, Point3, Vec3};
    use krly3d_core::Attempt;

    fn sphere_at(x: f64, z: f64) -> Sphere {
        Sphere::new(frame(&Point3::new(x, 0.0, z), &Vec3::z(), &Vec3::x()).unwrap(), 5.0).unwrap()
    }

    #[test]
    fn a_sphere_on_the_axis_meets_the_cylinder_in_two_cross_sections() {
        let found = cut(sphere_at(0.0, 7.0), Cylinder::new(Iso3::identity(), 3.0).unwrap());

        assert_eq!(summaries(found), ["circle r=3 axis=+z", "circle r=3 axis=+z"]);
    }

    #[test]
    fn a_sphere_inside_the_cylinder_does_not_meet_it() {
        assert!(summaries(cut(sphere_at(0.0, 0.0), Cylinder::new(Iso3::identity(), 8.0).unwrap())).is_empty());
    }

    #[test]
    fn a_sphere_as_wide_as_the_cylinder_touches_it_along_its_equator() {
        let found = cut(sphere_at(0.0, 7.0), Cylinder::new(Iso3::identity(), 5.0).unwrap());

        assert_eq!(summaries(found), ["touching circle r=5 axis=+z"]);
    }

    #[test]
    fn a_sphere_off_the_axis_is_declined() {
        let found = cut(sphere_at(1.0, 0.0), Cylinder::new(Iso3::identity(), 3.0).unwrap());

        assert!(matches!(found, Attempt::Declined(_)));
    }
}
