//! A line against any surface: the surface's own line equation, wrapped as a rule.

use super::all_on;
use crate::curve::Line;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::ports::meet::{Meet, MeetRequest, Meeting};
use krly3d_kernel::{Context, Plugin};

pub struct LineSurface;

impl Plugin<Meet> for LineSurface {
    fn id(&self) -> Id {
        Id::new("line|surface")
    }

    fn call(&self, cx: &Context, request: &MeetRequest) -> Attempt<Meeting> {
        let Some(line) = request.curve.as_any().downcast_ref::<Line>() else {
            return Attempt::declined("line|surface takes a line");
        };
        let accuracy = cx.accuracy();
        // Five points settle it for any surface of degree up to four, which covers the
        // quadrics and the torus. A surface of higher degree needs a rule of its own.
        let stride = accuracy.scale() / line.direction().norm();
        if all_on(
            request.surface.as_ref(),
            (-2..=2).map(|step| line.point(f64::from(step) * stride)),
            accuracy,
        ) {
            return Attempt::Done(Meeting::Within);
        }
        let mut parameters = Vec::new();
        let exact = request.surface.line(&line.origin(), &line.direction(), &mut parameters);
        parameters.sort_by(f64::total_cmp);
        Attempt::Done(Meeting::At { parameters, exact })
    }
}

#[cfg(test)]
mod tests {
    use crate::curve::Line;
    use crate::meet::testing::{met, parameters};
    use crate::surface::{Cylinder, Plane, Sphere, Torus};
    use krly3d_core::math::{Iso3, Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::meet::Meeting;

    #[test]
    fn a_line_through_a_sphere_meets_it_twice() {
        let line = Line::new(Point3::new(-20.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)).unwrap();
        let sphere = Sphere::new(Iso3::identity(), 5.0).unwrap();

        let crossings = parameters(met(line, sphere, (0.0, 20.0)));

        assert_eq!(crossings, [7.5, 12.5]);
    }

    #[test]
    fn a_line_through_the_tube_of_a_torus_meets_it_four_times() {
        let line = Line::new(Point3::new(-20.0, 0.5, 0.3), Vec3::x()).unwrap();
        let torus = Torus::new(Iso3::identity(), 6.0, 2.0).unwrap();

        let crossings = parameters(met(line, torus, (0.0, 40.0)));

        assert_eq!(crossings.len(), 4);
    }

    #[test]
    fn a_line_lying_in_a_plane_is_within_it() {
        let line = Line::new(Point3::new(3.0, -1.0, 2.0), Vec3::new(1.0, 4.0, 0.0)).unwrap();
        let plane = Plane::through(&Point3::new(0.0, 0.0, 2.0), &Vec3::z()).unwrap();

        assert_eq!(met(line, plane, (-5.0, 5.0)), Attempt::Done(Meeting::Within));
    }

    #[test]
    fn a_ruling_of_a_cylinder_is_within_it() {
        let line = Line::new(Point3::new(0.0, 3.0, -4.0), Vec3::z()).unwrap();
        let cylinder = Cylinder::new(Iso3::identity(), 3.0).unwrap();

        assert_eq!(met(line, cylinder, (-5.0, 5.0)), Attempt::Done(Meeting::Within));
    }

    #[test]
    fn a_line_beside_a_cylinder_does_not_meet_it() {
        let line = Line::new(Point3::new(0.0, 4.0, -4.0), Vec3::x()).unwrap();
        let cylinder = Cylinder::new(Iso3::identity(), 3.0).unwrap();

        assert!(parameters(met(line, cylinder, (-9.0, 9.0))).is_empty());
    }
}
