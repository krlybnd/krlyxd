//! Port of the `detect` family: the straight segments of an image. Keyed by detector
//! name.

pub mod contract;

use crate::context::Context;
use crate::image::{Gray, Pixel};
use crate::plugin::Family;
use krly3d_core::{Attempt, Id};
use nalgebra::{Vector2, Vector3};
use std::sync::Arc;

pub struct Detect;

impl Family for Detect {
    type Context = crate::context::Context;
    const NAME: &'static str = "detect";
    type Request = Arc<Gray>;
    type Response = Vec<Segment>;
}

/// A straight edge found in an image, in pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub from: Pixel,
    pub to: Pixel,
    /// Width of the band of pixels the segment was found in.
    pub width: f64,
    /// How surely the segment is not chance: minus the decimal logarithm of the number
    /// of such segments expected in an image of noise of the same size.
    pub significance: f64,
}

impl Segment {
    pub fn length(&self) -> f64 {
        (self.to - self.from).norm()
    }

    pub fn midpoint(&self) -> Pixel {
        nalgebra::center(&self.from, &self.to)
    }

    /// The unit direction from `from` to `to`.
    pub fn direction(&self) -> Vector2<f64> {
        (self.to - self.from).normalize()
    }

    /// The line carrying the segment, in homogeneous coordinates.
    pub fn line(&self) -> Vector3<f64> {
        self.from.to_homogeneous().cross(&self.to.to_homogeneous())
    }
}

pub fn detect(cx: &Context, detector: &Id, image: &Arc<Gray>) -> Attempt<Vec<Segment>> {
    cx.call::<Detect>(detector, image)
}

#[cfg(test)]
mod tests {
    use super::Segment;
    use crate::image::Pixel;

    #[test]
    fn the_line_of_a_segment_passes_through_its_ends() {
        let segment =
            Segment { from: Pixel::new(1.0, 2.0), to: Pixel::new(4.0, 6.0), width: 1.0, significance: 3.0 };

        let line = segment.line();

        assert!(line.dot(&segment.from.to_homogeneous()).abs() < 1e-12);
        assert!(line.dot(&segment.to.to_homogeneous()).abs() < 1e-12);
        assert_eq!(segment.length(), 5.0);
        assert_eq!(segment.midpoint(), Pixel::new(2.5, 4.0));
    }
}
