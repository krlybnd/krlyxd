//! Port of the `vanish` family: the points the segments run towards. Keyed by method.
//!
//! Lines that are parallel in the world meet in the image at their vanishing point, or
//! stay parallel there when they are parallel to the image; the point is then at
//! infinity, which homogeneous coordinates hold without a special case.

pub mod contract;

use crate::context::Context;
use crate::image::{centre, diagonal, Pixel};
use crate::plugin::Family;
use crate::ports::detect::Segment;
use krly3d_core::{Attempt, Id};
use nalgebra::{Matrix3, Vector3};

pub struct Vanish;

impl Family for Vanish {
    type Context = crate::context::Context;
    const NAME: &'static str = "vanish";
    type Request = VanishRequest;
    type Response = Vec<Vanishing>;
}

pub struct VanishRequest {
    pub segments: Vec<Segment>,
    /// Width and height of the image the segments were found in.
    pub size: (usize, usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Vanishing {
    /// The point in homogeneous pixel coordinates, of unit length; the last coordinate
    /// is zero for a point at infinity.
    pub point: Vector3<f64>,
    /// The indices of the segments running towards it, in increasing order.
    pub members: Vec<usize>,
    /// The summed length of the members in pixels. Points come sorted by it, largest first.
    pub support: f64,
}

impl Vanishing {
    /// The point in pixels, unless it is at infinity.
    pub fn pixel(&self) -> Option<Pixel> {
        let w = self.point.z;
        (w.abs() > f64::EPSILON * self.point.xy().norm())
            .then(|| Pixel::new(self.point.x / w, self.point.y / w))
    }
}

/// The conditioning every vanishing-point computation shares: pixel coordinates moved to
/// the middle of the image and divided by half its diagonal, so that the three
/// homogeneous coordinates are of comparable size.
pub fn conditioning(size: (usize, usize)) -> Matrix3<f64> {
    let (middle, half) = (centre(size), diagonal(size) / 2.0);
    Matrix3::new(1.0 / half, 0.0, -middle.x / half, 0.0, 1.0 / half, -middle.y / half, 0.0, 0.0, 1.0)
}

/// The inverse of [`conditioning`].
pub fn unconditioning(size: (usize, usize)) -> Matrix3<f64> {
    let (middle, half) = (centre(size), diagonal(size) / 2.0);
    Matrix3::new(half, 0.0, middle.x, 0.0, half, middle.y, 0.0, 0.0, 1.0)
}

pub fn vanish(cx: &Context, method: &Id, request: &VanishRequest) -> Attempt<Vec<Vanishing>> {
    cx.call::<Vanish>(method, request)
}

#[cfg(test)]
mod tests {
    use super::{conditioning, unconditioning, Vanishing};
    use crate::image::Pixel;
    use nalgebra::{Matrix3, Vector3};

    #[test]
    fn conditioning_is_undone_by_unconditioning() {
        let product = unconditioning((800, 600)) * conditioning((800, 600));

        assert!((product - Matrix3::identity()).norm() < 1e-12);
    }

    #[test]
    fn a_point_at_infinity_has_no_pixel() {
        let far = Vanishing { point: Vector3::new(0.0, 1.0, 0.0), members: Vec::new(), support: 0.0 };
        let near = Vanishing { point: Vector3::new(4.0, 2.0, 2.0), members: Vec::new(), support: 0.0 };

        assert_eq!(far.pixel(), None);
        assert_eq!(near.pixel(), Some(Pixel::new(2.0, 1.0)));
    }
}
