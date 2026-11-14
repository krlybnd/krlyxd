//! Two planes cross along a straight line.

use super::{curves, line, none};
use crate::surface::Plane;
use krly3d_core::math::frame::origin;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::section::{Cut, Section, SectionRequest};
use krly3d_kernel::{Context, Plugin};

pub struct PlanePlane;

impl Plugin<Section> for PlanePlane {
    fn id(&self) -> Id {
        Id::new("plane|plane")
    }

    fn call(&self, cx: &Context, request: &SectionRequest) -> Attempt<Cut> {
        let (Some(first), Some(second)) =
            (request.first.as_any().downcast_ref::<Plane>(), request.second.as_any().downcast_ref::<Plane>())
        else {
            return Attempt::declined("plane|plane takes two planes");
        };
        let (n1, n2) = (first.unit_normal(), second.unit_normal());
        let between = origin(second.frame()) - origin(first.frame());
        if cx.accuracy().parallel(&n1, &n2) {
            return if cx.accuracy().is_zero_length(between.dot(&n1)) {
                Attempt::Done(Cut::Same)
            } else {
                none()
            };
        }
        // The line runs along n1 × n2. A point of it: from the first origin, within the
        // first plane and square to the line, as far as it takes to reach the second plane.
        let direction = n1.cross(&n2);
        let towards = direction.cross(&n1);
        let point = origin(first.frame()) + towards * (between.dot(&n2) / towards.dot(&n2));
        curves(line(point, direction).map(|line| vec![line]))
    }
}

#[cfg(test)]
mod tests {
    use crate::section::testing::{cut, summaries};
    use crate::surface::Plane;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::section::Cut;

    fn plane(at: [f64; 3], normal: [f64; 3]) -> Plane {
        Plane::through(&Point3::from(at), &Vec3::from(normal)).unwrap()
    }

    #[test]
    fn two_planes_cross_along_a_line() {
        let found = cut(plane([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]), plane([3.0, 0.0, 0.0], [1.0, 0.0, 0.0]));

        assert_eq!(summaries(found), ["line along +y"]);
    }

    #[test]
    fn parallel_planes_do_not_meet_and_a_plane_is_the_same_as_itself() {
        let apart = cut(plane([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]), plane([0.0, 0.0, 5.0], [0.0, 0.0, -1.0]));
        let same = cut(plane([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]), plane([7.0, 1.0, 2.0], [0.0, 0.0, -1.0]));

        assert!(summaries(apart).is_empty());
        assert!(matches!(same, Attempt::Done(Cut::Same)));
    }
}
