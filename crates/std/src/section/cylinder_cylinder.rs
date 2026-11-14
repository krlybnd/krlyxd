//! Two cylinders with parallel axes cross in two straight lines.

use super::{curves, lens, line, none, touching, Lens};
use crate::surface::Cylinder;
use krly3d_core::math::frame::{origin, z_axis};
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::section::{Cut, Section, SectionRequest};
use krly3d_kernel::{Context, Plugin};

/// How far from the first axis, towards the second, two touching circles of the given
/// radii touch: at the first radius, on the far side when the first lies inside the second.
fn touch_distance(first: f64, second: f64, apart: f64) -> f64 {
    if apart < second && first < second {
        -first
    } else {
        first
    }
}

pub struct CylinderCylinder;

impl Plugin<Section> for CylinderCylinder {
    fn id(&self) -> Id {
        Id::new("cylinder|cylinder")
    }

    fn call(&self, cx: &Context, request: &SectionRequest) -> Attempt<Cut> {
        let (Some(first), Some(second)) = (
            request.first.as_any().downcast_ref::<Cylinder>(),
            request.second.as_any().downcast_ref::<Cylinder>(),
        ) else {
            return Attempt::declined("cylinder|cylinder takes two cylinders");
        };
        let axis = z_axis(first.frame());
        if !cx.accuracy().parallel(&axis, &z_axis(second.frame())) {
            return Attempt::declined("cylinders whose axes are not parallel cut a curve of degree four, for which there is no curve plugin");
        }
        // Seen along the axes the cylinders are two circles; where those cross, rulings stand.
        let between = origin(second.frame()) - origin(first.frame());
        let across = between - axis * between.dot(&axis);
        match lens(across.norm(), first.radius(), second.radius(), cx.accuracy()) {
            Lens::Apart => none(),
            Lens::Same => Attempt::Done(Cut::Same),
            Lens::Touching => touching(
                line(
                    origin(first.frame())
                        + across.normalize() * touch_distance(first.radius(), second.radius(), across.norm()),
                    axis,
                )
                .map(|ruling| vec![ruling]),
            ),
            Lens::Crossing { along, half } => {
                let towards = across.normalize();
                let middle = origin(first.frame()) + towards * along;
                let side = axis.cross(&towards) * half;
                curves(line(middle + side, axis).and_then(|one| Ok(vec![one, line(middle - side, axis)?])))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::section::testing::{cut, summaries};
    use crate::surface::Cylinder;
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::section::Cut;

    fn upright(x: f64, radius: f64) -> Cylinder {
        Cylinder::new(frame(&Point3::new(x, 0.0, 3.0), &Vec3::z(), &Vec3::x()).unwrap(), radius).unwrap()
    }

    #[test]
    fn parallel_cylinders_that_overlap_cross_in_two_rulings() {
        assert_eq!(summaries(cut(upright(0.0, 5.0), upright(6.0, 5.0))), ["line along +z", "line along +z"]);
    }

    #[test]
    fn parallel_cylinders_touching_from_outside_or_inside_share_one_ruling() {
        // The contract check in `cut` puts the ruling on both cylinders.
        assert_eq!(summaries(cut(upright(0.0, 5.0), upright(10.0, 5.0))), ["touching line along +z"]);
        assert_eq!(summaries(cut(upright(0.0, 5.0), upright(2.0, 3.0))), ["touching line along +z"]);
        assert_eq!(summaries(cut(upright(0.0, 3.0), upright(2.0, 5.0))), ["touching line along +z"]);
    }

    #[test]
    fn parallel_cylinders_apart_do_not_meet_and_a_cylinder_is_the_same_as_itself() {
        assert!(summaries(cut(upright(0.0, 5.0), upright(20.0, 5.0))).is_empty());
        assert!(matches!(cut(upright(0.0, 5.0), upright(0.0, 5.0)), Attempt::Done(Cut::Same)));
    }

    #[test]
    fn crossed_cylinders_are_declined() {
        let lying = Cylinder::new(frame(&Point3::origin(), &Vec3::x(), &Vec3::y()).unwrap(), 2.0).unwrap();

        assert!(matches!(cut(upright(0.0, 5.0), lying), Attempt::Declined(_)));
    }
}
