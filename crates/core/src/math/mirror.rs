//! Reflection in a plane.

use super::frame::{frame, origin, x_axis, z_axis};
use super::{Iso3, Point3, Vec3};
use crate::Error;

/// A reflection in the plane through `point` with unit normal `normal`:
/// `p ↦ p − 2 n (n · (p − point))`.
///
/// A reflection turns space inside out, so it is not a rigid motion and no frame can
/// carry it. Shapes built on a frame are reflected by building a new right-handed frame
/// for their image; see `shape_frame` and `path_frame`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mirror {
    point: Point3,
    normal: Vec3,
}

impl Mirror {
    pub fn new(point: Point3, normal: Vec3) -> Result<Self, Error> {
        let length = normal.norm();
        if !(length.is_finite() && length > 0.0) {
            return Err(Error::input("a mirror needs a normal with a direction"));
        }
        Ok(Self { point, normal: normal / length })
    }

    pub fn point(&self, point: &Point3) -> Point3 {
        point - self.normal * (2.0 * self.normal.dot(&(point - self.point)))
    }

    pub fn vector(&self, vector: &Vec3) -> Vec3 {
        vector - self.normal * (2.0 * self.normal.dot(vector))
    }

    /// The frame of the image of a shape that is symmetric about its frame's z axis: the
    /// origin and both axes are reflected, and y is chosen to keep the frame right-handed.
    /// The image is the reflected point set; its parameters run the other way round the axis.
    pub fn shape_frame(&self, placed: &Iso3) -> Result<Iso3, Error> {
        frame(&self.point(&origin(placed)), &self.vector(&z_axis(placed)), &self.vector(&x_axis(placed)))
    }

    /// The frame of the image of a path that turns about its frame's z axis, such as a
    /// circle: the axis is reflected and reversed, so that the image at each parameter is
    /// the reflected point at that parameter.
    pub fn path_frame(&self, placed: &Iso3) -> Result<Iso3, Error> {
        frame(&self.point(&origin(placed)), &-self.vector(&z_axis(placed)), &self.vector(&x_axis(placed)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::frame::y_axis;

    fn mirror() -> Mirror {
        Mirror::new(Point3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 1.0, 0.0)).unwrap()
    }

    #[test]
    fn a_reflection_undoes_itself_and_keeps_points_of_the_plane() {
        let point = Point3::new(-4.0, 0.5, 7.0);

        assert!((mirror().point(&mirror().point(&point)) - point).norm() < 1e-12);
        assert!((mirror().point(&Point3::new(1.0, 2.0, 9.0)) - Point3::new(1.0, 2.0, 9.0)).norm() < 1e-12);
    }

    #[test]
    fn the_image_of_a_frame_is_right_handed_with_its_axis_reflected_or_reversed() {
        let placed =
            frame(&Point3::new(0.0, 1.0, 0.0), &Vec3::new(0.0, 0.0, 1.0), &Vec3::new(1.0, 0.0, 0.0)).unwrap();

        let shape = mirror().shape_frame(&placed).unwrap();
        let path = mirror().path_frame(&placed).unwrap();

        assert!((z_axis(&shape) - mirror().vector(&z_axis(&placed))).norm() < 1e-12);
        assert!((y_axis(&path) - mirror().vector(&y_axis(&placed))).norm() < 1e-12);
        assert!((x_axis(&shape).cross(&y_axis(&shape)) - z_axis(&shape)).norm() < 1e-12);
        assert!((x_axis(&path).cross(&y_axis(&path)) - z_axis(&path)).norm() < 1e-12);
    }

    #[test]
    fn a_normal_without_direction_is_refused() {
        assert!(Mirror::new(Point3::origin(), Vec3::zeros()).is_err());
    }
}
