//! A plane cuts a sphere in a circle.

use super::{circle, curves, none};
use crate::surface::{Plane, Sphere};
use krly3d_core::math::frame::origin;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::section::{Cut, Section, SectionRequest};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::{Context, Plugin};

pub struct PlaneSphere;

impl Plugin<Section> for PlaneSphere {
    fn id(&self) -> Id {
        Id::new("plane|sphere")
    }

    fn call(&self, cx: &Context, request: &SectionRequest) -> Attempt<Cut> {
        let (Some(plane), Some(sphere)) = (
            request.first.as_any().downcast_ref::<Plane>(),
            request.second.as_any().downcast_ref::<Sphere>(),
        ) else {
            return Attempt::declined("plane|sphere takes a plane and a sphere");
        };
        // The height of the centre over the plane decides: a circle of radius √(r² − h²).
        let center = origin(sphere.frame());
        let height = plane.distance(&center);
        // A tangent plane shares a single point with the sphere, and a point is no curve.
        if cx.accuracy().is_zero_length(height.abs() - sphere.radius()) || height.abs() > sphere.radius() {
            return none();
        }
        let radius = (sphere.radius() * sphere.radius() - height * height).sqrt();
        curves(
            circle(center - plane.unit_normal() * height, plane.unit_normal(), radius)
                .map(|circle| vec![circle]),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::section::testing::{cut, summaries};
    use crate::surface::{Plane, Sphere};
    use krly3d_core::math::{Iso3, Point3, Vec3};

    fn plane_at(height: f64) -> Plane {
        Plane::through(&Point3::new(0.0, 0.0, height), &Vec3::z()).unwrap()
    }

    #[test]
    fn a_plane_through_a_sphere_cuts_a_circle_of_the_radius_pythagoras_gives() {
        let found = cut(plane_at(3.0), Sphere::new(Iso3::identity(), 5.0).unwrap());

        assert_eq!(summaries(found), ["circle r=4 axis=+z"]);
    }

    #[test]
    fn the_rule_answers_whichever_surface_is_named_first() {
        let found = cut(Sphere::new(Iso3::identity(), 5.0).unwrap(), plane_at(3.0));

        assert_eq!(summaries(found), ["circle r=4 axis=+z"]);
    }

    #[test]
    fn a_plane_beyond_the_sphere_and_a_tangent_plane_share_no_curve_with_it() {
        let beyond = cut(plane_at(6.0), Sphere::new(Iso3::identity(), 5.0).unwrap());
        let tangent = cut(plane_at(5.0), Sphere::new(Iso3::identity(), 5.0).unwrap());

        assert!(summaries(beyond).is_empty());
        assert!(summaries(tangent).is_empty());
    }
}
