//! A picture's frame: where its pixels are in the world.
//!
//! A drawing in one of the three orthographic views shows a world plane at a scale: image
//! x is one world axis and image y another, each stretched and shifted. Two pixels whose
//! world coordinates are known fix both, signs included, so a drawing whose y runs down or
//! whose nose points up needs no other care.

use crate::image::Pixel;
use krly3d_core::Error;

/// A drawing seen along a world axis: `top` along -z, `side` along +y, `front` along +x.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Top,
    Side,
    Front,
}

impl View {
    pub fn named(name: &str) -> Result<Self, Error> {
        match name {
            "top" => Ok(Self::Top),
            "side" => Ok(Self::Side),
            "front" => Ok(Self::Front),
            other => Err(Error::input(format!("a view is top, side or front, not `{other}`"))),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Side => "side",
            Self::Front => "front",
        }
    }

    /// The direction the drawing is seen in, a world vector.
    pub fn looking(self) -> [f64; 3] {
        match self {
            Self::Top => [0.0, 0.0, -1.0],
            Self::Side => [0.0, 1.0, 0.0],
            Self::Front => [1.0, 0.0, 0.0],
        }
    }

    /// The world axes that image x and image y show when nothing else is said.
    pub fn axes(self) -> [usize; 2] {
        match self {
            Self::Top => [0, 1],
            Self::Side => [0, 2],
            Self::Front => [1, 2],
        }
    }
}

pub const AXIS_NAMES: [&str; 3] = ["x", "y", "z"];

/// Pixel to world: world[axes[k]] = scale[k] · pixel[k] + offset[k].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub view: View,
    pub axes: [usize; 2],
    pub scale: [f64; 2],
    pub offset: [f64; 2],
}

/// How far the two scales may differ before a frame is refused: a drawing is drawn at one
/// scale, so a larger difference means a reference point was mistyped.
const ANISOTROPY: f64 = 0.03;

impl Frame {
    /// The frame in which pixel `first.0` is at world `first.1` and `second.0` at `second.1`,
    /// the world pairs given along `axes`.
    pub fn through(
        view: View,
        axes: [usize; 2],
        first: (Pixel, [f64; 2]),
        second: (Pixel, [f64; 2]),
    ) -> Result<Self, Error> {
        if axes[0] == axes[1] || axes.iter().any(|axis| *axis > 2) || !view_shows(view, axes) {
            return Err(Error::input(format!(
                "a {} view shows the axes {} and {}, not {}{}",
                view.name(),
                AXIS_NAMES[view.axes()[0]],
                AXIS_NAMES[view.axes()[1]],
                AXIS_NAMES[axes[0].min(2)],
                AXIS_NAMES[axes[1].min(2)]
            )));
        }
        let mut scale = [0.0; 2];
        let mut offset = [0.0; 2];
        for k in 0..2 {
            let apart = second.0[k] - first.0[k];
            if apart.abs() < 1.0 {
                return Err(Error::input(format!(
                    "the two reference pixels must be at least a pixel apart along image {}",
                    ["x", "y"][k]
                )));
            }
            scale[k] = (second.1[k] - first.1[k]) / apart;
            offset[k] = first.1[k] - scale[k] * first.0[k];
        }
        let (larger, smaller) = (scale[0].abs().max(scale[1].abs()), scale[0].abs().min(scale[1].abs()));
        if smaller <= 0.0 || larger / smaller - 1.0 > ANISOTROPY {
            return Err(Error::input(format!(
                "the reference points give {} and {} world units per pixel along image x and y; a drawing has one scale",
                scale[0].abs(),
                scale[1].abs()
            )));
        }
        Ok(Self { view, axes, scale, offset })
    }

    /// The world coordinates along `axes` of a pixel.
    pub fn world(&self, pixel: &Pixel) -> [f64; 2] {
        [self.scale[0] * pixel.x + self.offset[0], self.scale[1] * pixel.y + self.offset[1]]
    }

    /// World units per pixel.
    pub fn unit(&self) -> f64 {
        0.5 * (self.scale[0].abs() + self.scale[1].abs())
    }
}

fn view_shows(view: View, axes: [usize; 2]) -> bool {
    let mut shown = view.axes();
    let mut asked = axes;
    shown.sort_unstable();
    asked.sort_unstable();
    shown == asked
}

#[cfg(test)]
mod tests {
    use super::{Frame, View};
    use crate::image::Pixel;

    #[test]
    fn two_known_pixels_fix_the_frame_with_its_signs() {
        // A top view drawn nose up: image x runs along world y, image y along world x.
        let frame = Frame::through(
            View::Top,
            [1, 0],
            (Pixel::new(822.0, 12.0), [0.0, 0.0]),
            (Pixel::new(1068.0, 712.0), [54.9, 156.25]),
        )
        .unwrap();

        let world = frame.world(&Pixel::new(582.0, 455.0));

        assert!((world[0] + 53.561).abs() < 1e-3 && (world[1] - 98.884).abs() < 1e-3, "{world:?}");
    }

    #[test]
    fn a_frame_with_two_scales_is_refused() {
        let refused = Frame::through(
            View::Side,
            [0, 2],
            (Pixel::new(0.0, 0.0), [0.0, 0.0]),
            (Pixel::new(100.0, 100.0), [10.0, -20.0]),
        );

        assert!(refused.unwrap_err().message.contains("one scale"));
    }

    #[test]
    fn a_view_refuses_an_axis_it_does_not_show() {
        let refused = Frame::through(
            View::Front,
            [0, 2],
            (Pixel::new(0.0, 0.0), [0.0, 0.0]),
            (Pixel::new(10.0, 10.0), [1.0, 1.0]),
        );

        assert!(refused.is_err());
    }
}
