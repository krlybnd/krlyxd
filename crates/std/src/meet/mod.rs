//! Meet rules: where a curve meets a surface.

pub mod circle_plane;
pub mod circle_quadric;
pub mod line_surface;

use krly3d_core::math::Point3;
use krly3d_core::Accuracy;
use krly3d_kernel::ports::meet::Meet;
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::KernelBuilder;

pub fn register(builder: &mut KernelBuilder) {
    builder
        .plugin::<Meet>(line_surface::LineSurface)
        .plugin::<Meet>(circle_plane::CirclePlane)
        .plugin::<Meet>(circle_quadric::CircleQuadric);
}

/// Whether every given point lies on the surface. Along a curve, the equation of the
/// surface has a known degree; more zeros than that degree mean the whole curve is on it.
fn all_on(surface: &dyn Surface, points: impl IntoIterator<Item = Point3>, accuracy: &Accuracy) -> bool {
    points.into_iter().all(|point| accuracy.is_zero_length(surface.distance(&point)))
}

#[cfg(test)]
mod testing {
    use crate::standard;
    use krly3d_core::{Accuracy, Attempt};
    use krly3d_kernel::ports::curve::Curve;
    use krly3d_kernel::ports::meet::{contract, meet, MeetRequest, Meeting};
    use krly3d_kernel::ports::surface::Surface;
    use std::sync::Arc;

    /// Asks the standard registry, the way a tool does, and on success checks the meet
    /// contract over `range` before handing the answer to the test.
    pub(crate) fn met(curve: impl Curve, surface: impl Surface, range: (f64, f64)) -> Attempt<Meeting> {
        let mut builder = standard();
        builder.accuracy(Accuracy::for_scale(10.0).unwrap());
        let cx = builder.context();
        let request = MeetRequest { curve: Arc::new(curve), surface: Arc::new(surface) };
        let attempt = meet(&cx, &request);
        if let Attempt::Done(meeting) = &attempt {
            contract::check(meeting, &request, range, 2000, cx.accuracy()).unwrap();
        }
        attempt
    }

    pub(crate) fn parameters(attempt: Attempt<Meeting>) -> Vec<f64> {
        match attempt {
            Attempt::Done(Meeting::At { parameters, .. }) => parameters,
            other => panic!("expected crossings, got {other:?}"),
        }
    }
}
