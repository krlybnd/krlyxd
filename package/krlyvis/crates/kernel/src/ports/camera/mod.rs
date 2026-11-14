//! Port of the `camera` family: the camera that took the picture, worked out from the
//! vanishing points. Keyed by method; `camera` is the chain used by default.
//!
//! The camera is a pinhole: a point `X` of the camera frame (x right, y down, z ahead)
//! appears at `K X` with `K = [f 0 cx; 0 f cy; 0 0 1]`, and the vanishing point of a
//! direction `d` is `K d`. Directions found that way are the world axes; where the camera
//! stands is not known from one picture, so lengths come out up to one scale.

pub mod contract;

use crate::context::Context;
use crate::image::Pixel;
use crate::plugin::Family;
use crate::ports::vanish::Vanishing;
use crate::scene::Axis;
use krly3d_core::{Attempt, Id};
use nalgebra::{Matrix3, Vector3};

pub struct Calibrate;

impl Family for Calibrate {
    type Context = crate::context::Context;
    const NAME: &'static str = "camera";
    type Request = CameraRequest;
    type Response = Camera;
}

pub struct CameraRequest {
    /// Sorted by support, largest first, as the `vanish` family gives them.
    pub vanishing: Vec<Vanishing>,
    /// Width and height of the image.
    pub size: (usize, usize),
    /// The focal length in pixels, when the caller knows it.
    pub focal: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    /// Focal length in pixels.
    pub focal: f64,
    /// Where the axis of the lens meets the image.
    pub centre: Pixel,
    /// The world axes x, y, z as columns, in the camera frame. z points up.
    pub rotation: Matrix3<f64>,
    /// For each world axis the vanishing point it was taken from, or `None` where it was
    /// completed as the cross product of the other two.
    pub axes: [Option<usize>; 3],
    /// How far, in degrees, the vanishing point taken for y lies from the direction
    /// square to x and z: near zero when the three points belong to one box-shaped
    /// world, large when one of them is a chance meeting of unrelated lines. `None` when
    /// y was completed by the cross product.
    pub off_square: Option<f64>,
    /// The plugin that worked the camera out.
    pub method: Id,
}

impl Camera {
    /// The matrix `K` taking camera directions to homogeneous pixels.
    pub fn intrinsics(&self) -> Matrix3<f64> {
        Matrix3::new(self.focal, 0.0, self.centre.x, 0.0, self.focal, self.centre.y, 0.0, 0.0, 1.0)
    }

    /// The direction of the line of sight through a pixel, in the camera frame, with a
    /// unit component along the lens axis.
    pub fn ray(&self, pixel: Pixel) -> Vector3<f64> {
        Vector3::new((pixel.x - self.centre.x) / self.focal, (pixel.y - self.centre.y) / self.focal, 1.0)
    }

    /// Where a point of the camera frame appears, if it lies in front of the camera.
    pub fn project(&self, point: &Vector3<f64>) -> Option<Pixel> {
        (point.z > 0.0).then(|| {
            Pixel::new(
                self.centre.x + self.focal * point.x / point.z,
                self.centre.y + self.focal * point.y / point.z,
            )
        })
    }

    /// The world axis as a unit direction in the camera frame.
    pub fn axis(&self, axis: Axis) -> Vector3<f64> {
        self.rotation.column(axis.index()).into_owned()
    }

    /// The vanishing point of a world axis, homogeneous and of unit length.
    pub fn vanishing(&self, axis: Axis) -> Vector3<f64> {
        (self.intrinsics() * self.axis(axis)).normalize()
    }

    /// The horizon, the line every horizontal direction vanishes on, homogeneous.
    pub fn horizon(&self) -> Vector3<f64> {
        let up = self.axis(Axis::Z);
        Vector3::new(
            up.x / self.focal,
            up.y / self.focal,
            up.z - (self.centre.x * up.x + self.centre.y * up.y) / self.focal,
        )
    }
}

pub fn calibrate(cx: &Context, method: &Id, request: &CameraRequest) -> Attempt<Camera> {
    cx.call::<Calibrate>(method, request)
}

#[cfg(test)]
mod tests {
    use crate::ports::sample;
    use crate::scene::Axis;

    #[test]
    fn horizontal_directions_vanish_on_the_horizon() {
        let camera = sample::camera(35.0, 15.0);

        let horizon = camera.horizon();

        for axis in [Axis::X, Axis::Y] {
            assert!(horizon.dot(&camera.vanishing(axis)).abs() < 1e-12);
        }
    }

    #[test]
    fn a_point_on_a_ray_projects_back_to_its_pixel() {
        let camera = sample::camera(35.0, 15.0);
        let pixel = crate::image::Pixel::new(123.0, 456.0);

        let back = camera.project(&(camera.ray(pixel) * 7.5)).unwrap();

        assert!((back - pixel).norm() < 1e-9);
    }
}
