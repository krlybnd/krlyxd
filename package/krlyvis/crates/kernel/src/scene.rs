//! What one analysis knows: the image, what was asked, and what each stage found.

use crate::image::Gray;
use crate::ports::camera::Camera;
use crate::ports::depth::Depths;
use crate::ports::detect::Segment;
use crate::ports::measure::Measured;
use crate::ports::vanish::Vanishing;
use krly3d_core::{Error, Id};
use std::sync::Arc;
use std::time::Duration;

/// A world axis. z is the vertical; x and y are the two horizontal directions found in
/// the image, x the one with more lines running along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    pub fn letter(self) -> char {
        match self {
            Self::X => 'x',
            Self::Y => 'y',
            Self::Z => 'z',
        }
    }

    /// The two axes spanning the plane square to this one, in cyclic order.
    pub fn others(self) -> [Axis; 2] {
        match self {
            Self::X => [Self::Y, Self::Z],
            Self::Y => [Self::Z, Self::X],
            Self::Z => [Self::X, Self::Y],
        }
    }

    /// The axis square to the plane named by the letters of the two axes spanning it
    /// (`xz`, `zx`, `xy`, `yz` in either order).
    pub fn square_to(plane: &str) -> Result<Axis, Error> {
        let mut spanned = [false; 3];
        for letter in plane.chars() {
            let axis = match letter {
                'x' => Self::X,
                'y' => Self::Y,
                'z' => Self::Z,
                _ => return Err(Error::input(format!("`{plane}` is not a plane: use two of x, y, z"))),
            };
            spanned[axis.index()] = true;
        }
        match (plane.chars().count(), spanned) {
            (2, [false, true, true]) => Ok(Self::X),
            (2, [true, false, true]) => Ok(Self::Y),
            (2, [true, true, false]) => Ok(Self::Z),
            _ => Err(Error::input(format!("`{plane}` is not a plane: use two different letters of x, y, z"))),
        }
    }
}

/// A segment whose true length is known, by its id in the scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Known {
    pub segment: usize,
    pub length: f64,
}

/// What the caller wants beyond finding the lines.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    /// The focal length in pixels, when it is known from the lens and the sensor.
    pub focal: Option<f64>,
    /// The `camera` chain to calibrate with.
    pub camera: Id,
    /// The plane to measure in, named by the world axis square to it.
    pub plane: Option<Axis>,
    /// The length that sets the unit of the measurements: in the plane, or as the height
    /// of a segment standing on the ground.
    pub known: Option<Known>,
    /// The camera's height above the ground, when known; it sets the unit of depths.
    pub eye: Option<f64>,
}

impl Default for Ask {
    fn default() -> Self {
        Self { focal: None, camera: Id::new("camera"), plane: None, known: None, eye: None }
    }
}

/// The state the stages pass along. Segment ids are their positions in `segments`.
#[derive(Clone, Debug)]
pub struct Scene {
    pub image: Arc<Gray>,
    pub ask: Ask,
    pub segments: Vec<Segment>,
    pub vanishing: Vec<Vanishing>,
    pub camera: Option<Camera>,
    pub measured: Vec<Measured>,
    pub depth: Option<Depths>,
    /// Why a stage could not do its part; a note never stops the analysis.
    pub notes: Vec<String>,
    /// How long each stage took.
    pub timings: Vec<(Id, Duration)>,
}

impl Scene {
    pub fn new(image: Arc<Gray>, ask: Ask) -> Self {
        Self {
            image,
            ask,
            segments: Vec::new(),
            vanishing: Vec::new(),
            camera: None,
            measured: Vec::new(),
            depth: None,
            notes: Vec::new(),
            timings: Vec::new(),
        }
    }

    /// The vanishing point each segment runs towards, by segment id.
    pub fn vanishing_of(&self) -> Vec<Option<usize>> {
        let mut of = vec![None; self.segments.len()];
        for (index, point) in self.vanishing.iter().enumerate() {
            for &member in &point.members {
                if let Some(slot) = of.get_mut(member) {
                    *slot = Some(index);
                }
            }
        }
        of
    }
}

#[cfg(test)]
mod tests {
    use super::Axis;

    #[test]
    fn a_plane_is_named_by_the_two_axes_spanning_it_in_either_order() {
        assert_eq!(Axis::square_to("xz").unwrap(), Axis::Y);
        assert_eq!(Axis::square_to("zx").unwrap(), Axis::Y);
        assert_eq!(Axis::square_to("yz").unwrap(), Axis::X);
        assert_eq!(Axis::square_to("xy").unwrap(), Axis::Z);
    }

    #[test]
    fn a_name_that_is_not_a_plane_is_refused() {
        for wrong in ["xx", "x", "xyz", "ab", ""] {
            assert!(Axis::square_to(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn the_other_two_axes_follow_in_cyclic_order() {
        assert_eq!(Axis::X.others(), [Axis::Y, Axis::Z]);
        assert_eq!(Axis::Y.others(), [Axis::Z, Axis::X]);
        assert_eq!(Axis::Z.others(), [Axis::X, Axis::Y]);
    }
}
