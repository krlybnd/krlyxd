//! Two spheres cross in a circle about the line of their centres.

use super::{circle, curves, lens, none, Lens};
use crate::surface::Sphere;
use krly3d_core::math::frame::origin;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::section::{Cut, Section, SectionRequest};
use krly3d_kernel::{Context, Plugin};

pub struct SphereSphere;

impl Plugin<Section> for SphereSphere {
    fn id(&self) -> Id {
        Id::new("sphere|sphere")
    }

    fn call(&self, cx: &Context, request: &SectionRequest) -> Attempt<Cut> {
        let (Some(first), Some(second)) = (
            request.first.as_any().downcast_ref::<Sphere>(),
            request.second.as_any().downcast_ref::<Sphere>(),
        ) else {
            return Attempt::declined("sphere|sphere takes two spheres");
        };
        let between = origin(second.frame()) - origin(first.frame());
        match lens(between.norm(), first.radius(), second.radius(), cx.accuracy()) {
            // Spheres that touch share a single point, and a point is no curve.
            Lens::Apart | Lens::Touching => none(),
            Lens::Same => Attempt::Done(Cut::Same),
            Lens::Crossing { along, half } => {
                let center = origin(first.frame()) + between.normalize() * along;
                curves(circle(center, between, half).map(|circle| vec![circle]))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::section::testing::{cut, summaries};
    use crate::surface::Sphere;
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::section::Cut;

    fn sphere(x: f64, radius: f64) -> Sphere {
        Sphere::new(frame(&Point3::new(x, 0.0, 0.0), &Vec3::z(), &Vec3::x()).unwrap(), radius).unwrap()
    }

    #[test]
    fn equal_spheres_six_apart_cross_in_a_circle_of_radius_four() {
        assert_eq!(summaries(cut(sphere(0.0, 5.0), sphere(6.0, 5.0))), ["circle r=4 axis=+x"]);
    }

    #[test]
    fn spheres_apart_or_one_inside_the_other_do_not_meet() {
        assert!(summaries(cut(sphere(0.0, 5.0), sphere(11.0, 5.0))).is_empty());
        assert!(summaries(cut(sphere(0.0, 5.0), sphere(1.0, 2.0))).is_empty());
    }

    #[test]
    fn touching_spheres_share_no_curve_and_a_sphere_is_the_same_as_itself() {
        assert!(summaries(cut(sphere(0.0, 5.0), sphere(10.0, 5.0))).is_empty());
        assert!(matches!(cut(sphere(0.0, 5.0), sphere(0.0, 5.0)), Attempt::Done(Cut::Same)));
    }
}
