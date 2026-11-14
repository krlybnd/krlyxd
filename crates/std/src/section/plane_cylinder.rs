//! A plane cuts a cylinder in a circle when square to the axis and in two straight
//! lines when along it.

use super::{circle, curves, line, none, touching};
use crate::surface::{Cylinder, Plane};
use krly3d_core::math::frame::{origin, z_axis};
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::section::{Cut, Section, SectionRequest};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::{Context, Plugin};

pub struct PlaneCylinder;

impl Plugin<Section> for PlaneCylinder {
    fn id(&self) -> Id {
        Id::new("plane|cylinder")
    }

    fn call(&self, cx: &Context, request: &SectionRequest) -> Attempt<Cut> {
        let (Some(plane), Some(cylinder)) = (
            request.first.as_any().downcast_ref::<Plane>(),
            request.second.as_any().downcast_ref::<Cylinder>(),
        ) else {
            return Attempt::declined("plane|cylinder takes a plane and a cylinder");
        };
        let accuracy = cx.accuracy();
        let (normal, axis, base) = (plane.unit_normal(), z_axis(cylinder.frame()), origin(cylinder.frame()));
        if accuracy.parallel(&normal, &axis) {
            // Square to the axis: the cross-section where the axis pierces the plane.
            let center = base - axis * (plane.distance(&base) / normal.dot(&axis));
            return curves(circle(center, axis, cylinder.radius()).map(|circle| vec![circle]));
        }
        if !accuracy.perpendicular(&normal, &axis) {
            return Attempt::declined(
                "a plane oblique to a cylinder cuts an ellipse, for which there is no curve plugin",
            );
        }
        // Along the axis: the plane passes the axis at `height` and meets the surface in
        // the two rulings √(r² − h²) to either side of the foot of the axis.
        let height = plane.distance(&base);
        if accuracy.is_zero_length(height.abs() - cylinder.radius()) {
            return touching(line(base - normal * height, axis).map(|ruling| vec![ruling]));
        }
        if height.abs() > cylinder.radius() {
            return none();
        }
        let foot = base - normal * height;
        let half = normal.cross(&axis) * (cylinder.radius() * cylinder.radius() - height * height).sqrt();
        curves(line(foot + half, axis).and_then(|first| Ok(vec![first, line(foot - half, axis)?])))
    }
}

#[cfg(test)]
mod tests {
    use crate::section::testing::{cut, summaries};
    use crate::surface::{Cylinder, Plane};
    use krly3d_core::math::{Iso3, Point3, Vec3};
    use krly3d_core::Attempt;

    fn cylinder() -> Cylinder {
        Cylinder::new(Iso3::identity(), 5.0).unwrap()
    }

    #[test]
    fn a_plane_square_to_the_axis_cuts_the_cross_section() {
        let level = Plane::through(&Point3::new(1.0, 2.0, 7.0), &Vec3::z()).unwrap();

        assert_eq!(summaries(cut(level, cylinder())), ["circle r=5 axis=+z"]);
    }

    #[test]
    fn a_plane_along_the_axis_cuts_two_rulings() {
        let upright = Plane::through(&Point3::new(3.0, 0.0, 0.0), &Vec3::x()).unwrap();

        assert_eq!(summaries(cut(upright, cylinder())), ["line along +z", "line along +z"]);
    }

    #[test]
    fn an_oblique_plane_is_declined_and_a_tangent_plane_touches_along_a_ruling() {
        let oblique = Plane::through(&Point3::origin(), &Vec3::new(1.0, 0.0, 1.0)).unwrap();
        let tangent = Plane::through(&Point3::new(5.0, 0.0, 0.0), &Vec3::x()).unwrap();

        assert!(matches!(cut(oblique, cylinder()), Attempt::Declined(_)));
        assert_eq!(summaries(cut(tangent, cylinder())), ["touching line along +z"]);
    }
}
