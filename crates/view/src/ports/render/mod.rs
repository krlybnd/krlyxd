//! Port of the `render` family: a view of some bodies as an image, together with the
//! face seen in each pixel. Renderers are keyed by name.
//!
//! What a pixel means is settled here, not by a renderer: a [`View`] and the space the
//! bodies take give a [`Projection`], and the pixel in a column and a row shows whatever
//! the line of sight [`Projection::ray`] meets first. An image can therefore be read
//! back into the model, and any renderer can stand in for another.

pub mod contract;

use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::{Accuracy, Attempt, Error, Id};
use krly3d_kernel::ports::probe::Ray;
use krly3d_kernel::topo::{Body, FaceId};
use krly3d_kernel::Context;
use krly3d_kernel::Family;
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct Render;

impl Family for Render {
    const NAME: &'static str = "render";
    type Request = RenderRequest;
    type Response = Image;
    type Context = krly3d_kernel::Context;
}

pub struct RenderRequest {
    /// The bodies to draw. Where two of them coincide, the later one is seen.
    pub bodies: Vec<Arc<Body>>,
    pub view: View,
    /// Width and height of the image in pixels.
    pub size: (usize, usize),
    /// The colour of each body, in the order of `bodies`; a body without one, or all of
    /// them when the list is empty, takes the renderer's own colour.
    pub paint: Vec<Paint>,
}

/// The colour a body is painted, and the faces of it painted otherwise. A colour is the
/// one a face shows where it takes the light squarely; the renderer shades it from there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Paint {
    pub body: Option<[u8; CHANNELS]>,
    pub faces: BTreeMap<FaceId, [u8; CHANNELS]>,
}

impl Paint {
    /// The colour of a face, if it or its body has one.
    pub fn of(&self, face: FaceId) -> Option<[u8; CHANNELS]> {
        self.faces.get(&face).copied().or(self.body)
    }
}

/// From where the bodies are looked at. The projection is parallel, every line of sight
/// running along `toward` so that a length is equally long wherever it lies in the image,
/// unless the frame is a [`Frame::Camera`], whose lines of sight spread from its eye.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The direction of sight, from the eye to the bodies. Its length does not matter.
    pub toward: Vec3,
    /// A direction that is to point upward in the image: any that is not along `toward`.
    pub up: Vec3,
    pub frame: Frame,
}

/// How much of space the image takes in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Frame {
    /// Everything that is drawn, with [`MARGIN`] of the image left free on every side.
    Fit,
    /// The point `center` in the middle of the image, and the length `height` in model
    /// units from its top to its bottom.
    Window { center: Point3, height: f64 },
    /// A pinhole camera at `eye`, as the one that took a photograph: its lens axis runs
    /// along `toward`, `focal` is the distance from the pinhole to the image in pixels, and
    /// `centre` the pixel where the lens axis meets the image (its middle when `None`).
    /// Images of the same camera and size match a photograph pixel for pixel.
    Camera { eye: Point3, focal: f64, centre: Option<(f64, f64)> },
}

/// The pinhole of a perspective projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pinhole {
    pub eye: Point3,
    /// In pixels.
    pub focal: f64,
    /// The pixel where the lens axis meets the image; columns from the left, rows from the top.
    pub centre: (f64, f64),
}

/// The share of the width and of the height of a fitted image left free on each side.
pub const MARGIN: f64 = 0.05;

/// Where the pixels of an image lie in space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projection {
    /// The middle of the image, on a plane in front of everything drawn.
    pub center: Point3,
    /// Unit vector to the right in the image.
    pub right: Vec3,
    /// Unit vector upward in the image.
    pub up: Vec3,
    /// Unit vector along every line of sight.
    pub toward: Vec3,
    /// The length of the side of one pixel, in model units; for a camera, in model units per
    /// unit of distance from the eye.
    pub pixel: f64,
    /// Width and height of the image in pixels.
    pub size: (usize, usize),
    /// The pinhole the lines of sight spread from, for a perspective projection.
    pub pinhole: Option<Pinhole>,
}

impl Projection {
    /// The line of sight through the middle of a pixel. Columns count from the left and
    /// rows from the top; everything drawn lies ahead of the origin of the ray.
    pub fn ray(&self, column: usize, row: usize) -> Ray {
        if let Some(pinhole) = &self.pinhole {
            let across = (column as f64 + 0.5 - pinhole.centre.0) / pinhole.focal;
            let down = (row as f64 + 0.5 - pinhole.centre.1) / pinhole.focal;
            return Ray {
                origin: pinhole.eye,
                direction: self.toward + self.right * across - self.up * down,
            };
        }
        let across = (column as f64 + 0.5 - self.size.0 as f64 / 2.0) * self.pixel;
        let down = (row as f64 + 0.5 - self.size.1 as f64 / 2.0) * self.pixel;
        Ray { origin: self.center + self.right * across - self.up * down, direction: self.toward }
    }
}

impl View {
    /// The pixels of an image of `size` in space, for bodies that lie within `around`.
    pub fn projection(
        &self,
        accuracy: &Accuracy,
        around: &Bounds,
        size: (usize, usize),
    ) -> Result<Projection, Error> {
        if size.0 == 0 || size.1 == 0 {
            return Err(Error::input("an image is at least one pixel wide and one pixel high"));
        }
        let length = self.toward.norm();
        if !(length.is_finite() && length > 0.0) {
            return Err(Error::input("the direction of sight needs a direction, got no length"));
        }
        if !self.up.norm().is_finite() || accuracy.parallel(&self.up, &self.toward) {
            return Err(Error::input("the upward direction of a view must not run along the line of sight"));
        }
        let toward = self.toward / length;
        let right = toward.cross(&self.up).normalize();
        let up = right.cross(&toward);
        if let Frame::Camera { eye, focal, centre } = self.frame {
            if !(focal.is_finite() && focal > 0.0) {
                return Err(Error::input(format!(
                    "the focal length of a camera is a positive number of pixels, got {focal}"
                )));
            }
            let centre = centre.unwrap_or((size.0 as f64 / 2.0, size.1 as f64 / 2.0));
            let pinhole = Some(Pinhole { eye, focal, centre });
            return Ok(Projection { center: eye, right, up, toward, pixel: 1.0 / focal, size, pinhole });
        }
        let reach = Reach::of(around, [&right, &up, &toward]);
        let (middle, pixel) = match (self.frame, &reach) {
            (Frame::Window { center, height }, _) if height.is_finite() && height > 0.0 => {
                (center, height / size.1 as f64)
            }
            (Frame::Window { .. }, _) => {
                return Err(Error::input("the height of the window of a view must be a positive length"))
            }
            (Frame::Fit, Some(reach)) => {
                let free = 1.0 - 2.0 * MARGIN;
                let pixel =
                    (reach.span(0) / (size.0 as f64 * free)).max(reach.span(1) / (size.1 as f64 * free));
                if accuracy.is_zero_length(pixel) {
                    return Err(Error::input("there is nothing with a size to fit the view to"));
                }
                (Point3::from(right * reach.middle(0) + up * reach.middle(1)), pixel)
            }
            (Frame::Fit, None) => return Err(Error::input("there is nothing to fit the view to")),
            (Frame::Camera { .. }, _) => return Err(Error::input("a camera has no window to fit")),
        };
        // The image plane stands off from the nearest point drawn by the size of what is
        // drawn, so that every hit lies clearly ahead of the origin of its ray.
        let ahead = match &reach {
            Some(reach) => reach.low[2] - around.size().norm() - accuracy.scale(),
            None => middle.coords.dot(&toward),
        };
        let center = middle + toward * (ahead - middle.coords.dot(&toward));
        Ok(Projection { center, right, up, toward, pixel, size, pinhole: None })
    }
}

/// How far a box reaches along three directions.
struct Reach {
    low: [f64; 3],
    high: [f64; 3],
}

impl Reach {
    fn of(around: &Bounds, axes: [&Vec3; 3]) -> Option<Self> {
        if around.is_empty() {
            return None;
        }
        let mut reach = Self { low: [f64::INFINITY; 3], high: [f64::NEG_INFINITY; 3] };
        for corner in 0..8 {
            let pick =
                |axis: usize| if corner >> axis & 1 == 0 { around.low[axis] } else { around.high[axis] };
            let point = Vec3::new(pick(0), pick(1), pick(2));
            for (index, axis) in axes.iter().enumerate() {
                reach.low[index] = reach.low[index].min(point.dot(axis));
                reach.high[index] = reach.high[index].max(point.dot(axis));
            }
        }
        Some(reach)
    }

    fn span(&self, axis: usize) -> f64 {
        self.high[axis] - self.low[axis]
    }

    fn middle(&self, axis: usize) -> f64 {
        (self.high[axis] + self.low[axis]) / 2.0
    }
}

/// How many bytes of [`Image::pixels`] one pixel takes: red, green and blue.
pub const CHANNELS: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub projection: Projection,
    /// Red, green and blue of every pixel, a byte each, row by row from the top left.
    pub pixels: Vec<u8>,
    /// What every pixel shows, in the same order; `None` where nothing is drawn.
    pub seen: Vec<Option<Seen>>,
}

impl Image {
    pub fn width(&self) -> usize {
        self.projection.size.0
    }

    pub fn height(&self) -> usize {
        self.projection.size.1
    }
}

/// A face of one of the bodies that were drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Seen {
    /// Index of the body among the bodies of the request.
    pub body: usize,
    pub face: FaceId,
}

/// Asks the renderer registered as `renderer` for an image.
pub fn render(cx: &Context, renderer: &str, request: &RenderRequest) -> Attempt<Image> {
    cx.call::<Render>(&Id::new(renderer), request)
}

#[cfg(test)]
mod tests {
    use super::{Frame, View, MARGIN};
    use krly3d_core::math::bounds::Bounds;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::{Accuracy, ErrorKind};

    fn block() -> Bounds {
        Bounds::of([Point3::new(-30.0, -20.0, 0.0), Point3::new(30.0, 20.0, 10.0)])
    }

    fn from_the_front(frame: Frame) -> View {
        View { toward: Vec3::new(0.0, 2.0, 0.0), up: Vec3::z(), frame }
    }

    fn close(a: &Vec3, b: &Vec3) -> bool {
        (a - b).norm() < 1e-12
    }

    #[test]
    fn seen_from_the_front_x_points_right_and_z_points_up() {
        let view = from_the_front(Frame::Fit);

        let projection = view.projection(&Accuracy::default(), &block(), (400, 300)).unwrap();

        assert!(close(&projection.right, &Vec3::x()) && close(&projection.up, &Vec3::z()));
        assert!(close(&projection.toward, &Vec3::y()));
    }

    #[test]
    fn a_tilted_up_direction_is_straightened_against_the_line_of_sight() {
        let view = View { toward: Vec3::new(-1.0, 1.0, -1.0), up: Vec3::z(), frame: Frame::Fit };

        let projection = view.projection(&Accuracy::default(), &block(), (400, 300)).unwrap();

        assert!(projection.up.dot(&projection.toward).abs() < 1e-12);
        assert!(projection.right.dot(&projection.toward).abs() < 1e-12);
        assert!(close(&projection.right.cross(&projection.up), &-projection.toward));
        assert!(projection.up.z > 0.0);
    }

    #[test]
    fn a_fitted_view_leaves_the_margin_free_on_the_tighter_side() {
        let view = from_the_front(Frame::Fit);

        let projection = view.projection(&Accuracy::default(), &block(), (400, 300)).unwrap();

        let left = projection.ray(0, 0).origin;
        let right = projection.ray(399, 299).origin;
        assert!((projection.pixel - 60.0 / (400.0 * (1.0 - 2.0 * MARGIN))).abs() < 1e-12);
        assert!((left.x + right.x).abs() < 1e-9 && ((left.z + right.z) / 2.0 - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_window_is_centred_where_asked_and_as_high_as_asked() {
        let view = from_the_front(Frame::Window { center: Point3::new(1.0, 0.0, 2.0), height: 30.0 });

        let projection = view.projection(&Accuracy::default(), &block(), (400, 300)).unwrap();

        let (top, bottom) = (projection.ray(200, 0).origin, projection.ray(200, 299).origin);
        assert_eq!(projection.pixel, 0.1);
        assert!(((top.z + bottom.z) / 2.0 - 2.0).abs() < 1e-9);
        assert!((top.z - bottom.z - 29.9).abs() < 1e-9 && (top.x - 1.05).abs() < 1e-9);
    }

    #[test]
    fn every_line_of_sight_starts_in_front_of_what_is_drawn() {
        let view = View {
            toward: Vec3::new(-1.0, 1.0, -1.0),
            up: Vec3::z(),
            frame: Frame::Window { center: Point3::new(0.0, 0.0, 5.0), height: 80.0 },
        };

        let projection = view.projection(&Accuracy::default(), &block(), (40, 30)).unwrap();

        let origin = projection.ray(20, 15).origin;
        let nearest = Point3::new(30.0, -20.0, 10.0);
        assert!((nearest - origin).dot(&projection.toward) > 0.0);
    }

    #[test]
    fn rows_count_downward_and_columns_to_the_right() {
        let view = from_the_front(Frame::Fit);

        let projection = view.projection(&Accuracy::default(), &block(), (400, 300)).unwrap();

        let (first, across, below) = (projection.ray(0, 0), projection.ray(1, 0), projection.ray(0, 1));
        assert!(close(&(across.origin - first.origin), &(Vec3::x() * projection.pixel)));
        assert!(close(&(below.origin - first.origin), &(-Vec3::z() * projection.pixel)));
    }

    #[test]
    fn a_view_that_cannot_be_set_up_is_refused() {
        let accuracy = Accuracy::default();
        let along = View { toward: Vec3::z(), up: Vec3::z(), frame: Frame::Fit };
        let nowhere = View { toward: Vec3::zeros(), up: Vec3::z(), frame: Frame::Fit };
        let flat = from_the_front(Frame::Window { center: Point3::origin(), height: 0.0 });

        let errors = [
            along.projection(&accuracy, &block(), (4, 3)).unwrap_err(),
            nowhere.projection(&accuracy, &block(), (4, 3)).unwrap_err(),
            flat.projection(&accuracy, &block(), (4, 3)).unwrap_err(),
            from_the_front(Frame::Fit).projection(&accuracy, &Bounds::empty(), (4, 3)).unwrap_err(),
            from_the_front(Frame::Fit).projection(&accuracy, &block(), (0, 3)).unwrap_err(),
        ];

        assert!(errors.iter().all(|error| error.kind == ErrorKind::Input), "{errors:?}");
    }
}
