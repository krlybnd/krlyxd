//! A line turned about an axis sweeps a cylinder, a plane or a cone.

use crate::curve::Line;
use crate::surface::{Cone, Cylinder, Plane};
use krly3d_core::math::frame::frame;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::ports::sweep::{Motion, Sweep, SweepRequest};
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct LineTurn;

impl Plugin<Sweep> for LineTurn {
    fn id(&self) -> Id {
        Id::new("line|turn")
    }

    fn call(&self, cx: &Context, request: &SweepRequest) -> Attempt<Arc<dyn Surface>> {
        let (Some(line), Motion::Turn { origin, axis, .. }) =
            (request.curve.as_any().downcast_ref::<Line>(), request.motion)
        else {
            return Attempt::declined("line|turn takes a line and a turn");
        };
        let accuracy = cx.accuracy();
        let (direction, axis) = (line.direction(), axis.into_inner());
        let offset = line.origin() - origin;
        let height = offset.dot(&axis);
        let radial = offset - axis * height;
        if accuracy.parallel(&direction, &axis) {
            if accuracy.is_zero_length(radial.norm()) {
                return Attempt::Failed(Error::geometry("a line on the axis sweeps no surface"));
            }
            return frame(&origin, &axis, &radial)
                .and_then(|placed| Cylinder::new(placed, radial.norm()))
                .map(|cylinder| Arc::new(cylinder) as Arc<dyn Surface>)
                .into();
        }
        if accuracy.perpendicular(&direction, &axis) {
            return frame(&(origin + axis * height), &axis, &direction)
                .map(|placed| Arc::new(Plane::new(placed)) as Arc<dyn Surface>)
                .into();
        }
        // Oblique: the line sweeps a cone when it meets the axis, with the apex where they meet.
        let across = axis.cross(&direction);
        if !accuracy.is_zero_length(offset.dot(&across) / across.norm()) {
            return Attempt::declined("a line skew to the axis turns into a hyperboloid of one sheet, for which there is no surface plugin");
        }
        let apex = origin + axis * (offset.cross(&direction).dot(&across) / across.norm_squared());
        let half_angle = (direction.dot(&axis).abs() / direction.norm()).acos();
        frame(&apex, &axis, &(direction - axis * direction.dot(&axis)))
            .and_then(|placed| Cone::new(placed, half_angle))
            .map(|cone| Arc::new(cone) as Arc<dyn Surface>)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use crate::curve::Line;
    use crate::surface::{Cone, Cylinder};
    use crate::sweep::testing::{kind, swept};
    use krly3d_core::math::frame::origin;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::sweep::Motion;
    use std::f64::consts::{FRAC_PI_4, TAU};

    fn about_z() -> Motion {
        Motion::turn(Point3::new(1.0, 1.0, 0.0), Vec3::z(), TAU).unwrap()
    }

    #[test]
    fn a_line_parallel_to_the_axis_sweeps_a_cylinder_of_its_distance() {
        let line = Line::new(Point3::new(4.0, 5.0, 2.0), Vec3::new(0.0, 0.0, -3.0)).unwrap();

        let surface = swept(line, about_z());

        let Attempt::Done(surface) = surface else { panic!("{}", kind(&surface)) };
        let cylinder = surface.as_any().downcast_ref::<Cylinder>().unwrap();
        assert!((cylinder.radius() - 5.0).abs() < 1e-15);
    }

    #[test]
    fn a_line_perpendicular_to_the_axis_sweeps_a_plane() {
        let line = Line::new(Point3::new(4.0, 5.0, 2.0), Vec3::new(1.0, -2.0, 0.0)).unwrap();

        assert_eq!(kind(&swept(line, about_z())), "plane");
    }

    #[test]
    fn an_oblique_line_meeting_the_axis_sweeps_a_cone_with_its_apex_there() {
        // From (3, 1, 0) the line climbs towards the axis x = 1, y = 1 and meets it at height 2.
        let line = Line::new(Point3::new(3.0, 1.0, 0.0), Vec3::new(-1.0, 0.0, 1.0)).unwrap();

        let surface = swept(line, about_z());

        let Attempt::Done(surface) = surface else { panic!("{}", kind(&surface)) };
        let cone = surface.as_any().downcast_ref::<Cone>().unwrap();
        assert!((origin(cone.frame()) - Point3::new(1.0, 1.0, 2.0)).norm() < 1e-15);
        assert!((cone.half_angle() - FRAC_PI_4).abs() < 1e-15);
    }

    #[test]
    fn a_line_on_the_axis_fails() {
        let line = Line::new(Point3::new(1.0, 1.0, 7.0), Vec3::z()).unwrap();

        assert_eq!(kind(&swept(line, about_z())), "failed");
    }

    #[test]
    fn a_line_skew_to_the_axis_is_declined() {
        let line = Line::new(Point3::new(4.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 1.0)).unwrap();

        assert_eq!(kind(&swept(line, about_z())), "declined");
    }
}
