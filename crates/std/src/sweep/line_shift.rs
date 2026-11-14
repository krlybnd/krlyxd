//! A line shifted sideways sweeps a plane.

use crate::curve::Line;
use crate::surface::Plane;
use krly3d_core::math::frame::frame;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::ports::sweep::{Motion, Sweep, SweepRequest};
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct LineShift;

impl Plugin<Sweep> for LineShift {
    fn id(&self) -> Id {
        Id::new("line|shift")
    }

    fn call(&self, cx: &Context, request: &SweepRequest) -> Attempt<Arc<dyn Surface>> {
        let (Some(line), Motion::Shift { by }) =
            (request.curve.as_any().downcast_ref::<Line>(), request.motion)
        else {
            return Attempt::declined("line|shift takes a line and a shift");
        };
        if cx.accuracy().parallel(&line.direction(), &by) {
            return Attempt::Failed(Error::geometry("a line shifted along itself sweeps no surface"));
        }
        // The normal is direction × shift, so the profile direction and the shift form a
        // right-handed pair seen from the side the normal points to.
        frame(&line.origin(), &line.direction().cross(&by), &line.direction())
            .map(|placed| Arc::new(Plane::new(placed)) as Arc<dyn Surface>)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use crate::curve::Line;
    use crate::sweep::testing::{kind, swept};
    use krly3d_core::math::{Point3, Uv, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::sweep::Motion;

    #[test]
    fn a_line_shifted_sideways_sweeps_a_plane_with_normal_direction_cross_shift() {
        let line = Line::new(Point3::new(1.0, 2.0, 3.0), Vec3::x()).unwrap();

        let surface = swept(line, Motion::shift(Vec3::new(0.0, 4.0, 0.0)).unwrap());

        assert_eq!(kind(&surface), "plane");
        let Attempt::Done(plane) = surface else { panic!() };
        assert!((plane.normal(Uv::origin()) - Vec3::z()).norm() < 1e-15);
    }

    #[test]
    fn a_line_shifted_along_itself_fails() {
        let line = Line::new(Point3::origin(), Vec3::x()).unwrap();

        let surface = swept(line, Motion::shift(Vec3::new(-3.0, 0.0, 0.0)).unwrap());

        assert_eq!(kind(&surface), "failed");
    }
}
