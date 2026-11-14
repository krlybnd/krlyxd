//! Inputs whose answer is known, shared by the contract tests: drawn polygons, noise,
//! and a camera with known parameters.

use crate::image::{Gray, Pixel};
use crate::ports::camera::Camera;
use krly3d_core::{Error, Id};
use nalgebra::{Matrix3, Vector3};

/// Width and height of the images of the sample camera.
pub const SIZE: (usize, usize) = (800, 600);

/// Samples per pixel along each side when a polygon is painted.
const SUBSAMPLES: usize = 4;

/// A grey image being painted.
pub struct Canvas {
    width: usize,
    height: usize,
    values: Vec<f64>,
}

impl Canvas {
    pub fn new((width, height): (usize, usize), background: f64) -> Self {
        Self { width, height, values: vec![background; width * height] }
    }

    /// Paints a polygon over what is there, each pixel blended by the share of it the
    /// polygon covers, so that edges are as smooth as in a photograph.
    pub fn fill(&mut self, corners: &[Pixel], value: f64) {
        let Some((columns, rows)) = self.span(corners) else { return };
        let step = 1.0 / SUBSAMPLES as f64;
        let total = (SUBSAMPLES * SUBSAMPLES) as f64;
        for row in rows {
            for column in columns.clone() {
                let mut inside = 0usize;
                for sub_row in 0..SUBSAMPLES {
                    for sub_column in 0..SUBSAMPLES {
                        let at = Pixel::new(
                            column as f64 + (sub_column as f64 + 0.5) * step,
                            row as f64 + (sub_row as f64 + 0.5) * step,
                        );
                        inside += usize::from(contains(corners, at));
                    }
                }
                let cover = inside as f64 / total;
                let slot = &mut self.values[row * self.width + column];
                *slot = *slot * (1.0 - cover) + value * cover;
            }
        }
    }

    /// The pixel columns and rows the polygon can touch, clipped to the canvas.
    fn span(&self, corners: &[Pixel]) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
        let clip = |low: f64, high: f64, size: usize| {
            let start = low.floor().max(0.0) as usize;
            let end = (high.ceil().max(0.0) as usize).min(size);
            (start < end).then_some(start..end)
        };
        let (mut low, mut high) =
            (Pixel::new(f64::INFINITY, f64::INFINITY), Pixel::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
        for corner in corners {
            low = Pixel::new(low.x.min(corner.x), low.y.min(corner.y));
            high = Pixel::new(high.x.max(corner.x), high.y.max(corner.y));
        }
        Some((clip(low.x, high.x, self.width)?, clip(low.y, high.y, self.height)?))
    }

    pub fn image(self) -> Result<Gray, Error> {
        Gray::new(self.width, self.height, self.values)
    }
}

/// Whether a point lies inside a polygon, by the even-odd rule.
fn contains(corners: &[Pixel], at: Pixel) -> bool {
    let mut inside = false;
    let mut previous = corners.len().wrapping_sub(1);
    for (index, corner) in corners.iter().enumerate() {
        let other = corners[previous];
        if (corner.y > at.y) != (other.y > at.y) {
            let crossing = corner.x + (at.y - corner.y) * (other.x - corner.x) / (other.y - corner.y);
            if at.x < crossing {
                inside = !inside;
            }
        }
        previous = index;
    }
    inside
}

/// An image of uniform noise from a fixed seed: no line is drawn in it.
pub fn noise(size: (usize, usize), seed: u64) -> Result<Gray, Error> {
    let mut state = seed;
    let values = (0..size.0 * size.1)
        .map(|_| {
            // The constants of Knuth's MMIX generator; the top bits are the best mixed.
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 56) as f64
        })
        .collect();
    Gray::new(size.0, size.1, values)
}

/// A camera of focal length 700 pixels in the middle of an 800 x 600 image, turned
/// `yaw` degrees from the world x axis toward y and tilted `pitch` degrees downward.
pub fn camera(yaw: f64, pitch: f64) -> Camera {
    let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
    let forward = Vector3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin());
    let right = Vector3::new(yaw.sin(), -yaw.cos(), 0.0);
    let down = forward.cross(&right);
    Camera {
        focal: 700.0,
        centre: Pixel::new(400.0, 300.0),
        // Rows are the camera axes in world coordinates, so the columns are the world
        // axes in camera coordinates.
        rotation: Matrix3::from_rows(&[right.transpose(), down.transpose(), forward.transpose()]),
        axes: [None; 3],
        off_square: None,
        method: Id::new("sample"),
    }
}

/// Where a world point appears to a camera standing at `eye`, if it is in front of it.
pub fn project(camera: &Camera, eye: &Vector3<f64>, point: &Vector3<f64>) -> Option<Pixel> {
    camera.project(&(camera.rotation * (point - eye)))
}

#[cfg(test)]
mod tests {
    use super::{camera, noise, project, Canvas, SIZE};
    use crate::image::Pixel;
    use nalgebra::Vector3;

    #[test]
    fn a_painted_square_covers_its_pixels_and_blends_its_border() {
        let mut canvas = Canvas::new((10, 10), 0.0);

        canvas.fill(
            &[Pixel::new(2.0, 2.0), Pixel::new(6.5, 2.0), Pixel::new(6.5, 6.0), Pixel::new(2.0, 6.0)],
            100.0,
        );
        let image = canvas.image().unwrap();

        assert_eq!(image.at(3, 3), 100.0);
        assert_eq!(image.at(6, 3), 50.0);
        assert_eq!(image.at(8, 3), 0.0);
    }

    #[test]
    fn noise_is_the_same_for_the_same_seed() {
        assert_eq!(noise(SIZE, 3).unwrap(), noise(SIZE, 3).unwrap());
        assert_ne!(noise(SIZE, 3).unwrap(), noise(SIZE, 4).unwrap());
    }

    #[test]
    fn the_sample_camera_sees_the_point_it_looks_at_in_the_middle() {
        let camera = camera(30.0, 10.0);
        let eye = Vector3::new(1.0, 2.0, 3.0);
        let ahead = eye + 5.0 * (camera.rotation.transpose() * Vector3::z());

        let seen = project(&camera, &eye, &ahead).unwrap();

        assert!((seen - Pixel::new(400.0, 300.0)).norm() < 1e-9);
        assert!(
            (camera.rotation * camera.rotation.transpose() - nalgebra::Matrix3::identity()).norm() < 1e-12
        );
        assert!(camera.axis(crate::scene::Axis::Z).y < 0.0, "world up is image up");
    }
}
