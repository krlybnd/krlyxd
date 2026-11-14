//! Tolerances scaled to the size of the model.

use crate::error::Error;
use crate::math::{Point3, Vec3};

/// Every tolerance the kernel uses, derived from the size of the model.
///
/// One absolute epsilon is wrong for both a watch part and a building. Here the caller
/// states the characteristic size of the model once and all tolerances follow from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Accuracy {
    scale: f64,
    resolution: f64,
    angular: f64,
}

impl Accuracy {
    /// Relative length resolution used unless the caller chooses another.
    pub const RESOLUTION: f64 = 1e-9;
    /// Angular resolution in radians used unless the caller chooses another.
    pub const ANGULAR: f64 = 1e-9;

    /// Tolerances for a model whose characteristic size is `scale`, in model units.
    pub fn for_scale(scale: f64) -> Result<Self, Error> {
        positive("scale", scale)?;
        Ok(Self { scale, ..Self::default() })
    }

    pub fn with_resolution(self, resolution: f64) -> Result<Self, Error> {
        fraction("resolution", resolution)?;
        Ok(Self { resolution, ..self })
    }

    pub fn with_angular(self, angular: f64) -> Result<Self, Error> {
        fraction("angular", angular)?;
        Ok(Self { angular, ..self })
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Lengths below this are zero; points closer than this are the same point.
    pub fn length(&self) -> f64 {
        self.scale * self.resolution
    }

    /// Areas below this are zero.
    pub fn area(&self) -> f64 {
        self.length() * self.scale
    }

    /// Volumes below this are zero.
    pub fn volume(&self) -> f64 {
        self.area() * self.scale
    }

    /// Angles below this, in radians, are zero.
    pub fn angle(&self) -> f64 {
        self.angular
    }

    /// Tolerance for unit-free ratios and normalised residuals.
    pub fn relative(&self) -> f64 {
        self.resolution
    }

    pub fn is_zero_length(&self, length: f64) -> bool {
        length.abs() <= self.length()
    }

    pub fn same_point(&self, a: &Point3, b: &Point3) -> bool {
        (a - b).norm() <= self.length()
    }

    pub fn parallel(&self, u: &Vec3, v: &Vec3) -> bool {
        u.cross(v).norm() <= self.angular * u.norm() * v.norm()
    }

    pub fn perpendicular(&self, u: &Vec3, v: &Vec3) -> bool {
        u.dot(v).abs() <= self.angular * u.norm() * v.norm()
    }
}

impl Default for Accuracy {
    fn default() -> Self {
        Self { scale: 1.0, resolution: Self::RESOLUTION, angular: Self::ANGULAR }
    }
}

fn positive(name: &str, value: f64) -> Result<(), Error> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(Error::input(format!("accuracy {name} must be a positive finite number, got {value}")))
    }
}

fn fraction(name: &str, value: f64) -> Result<(), Error> {
    positive(name, value)?;
    if value < 1.0 {
        Ok(())
    } else {
        Err(Error::input(format!("accuracy {name} must be below 1, got {value}")))
    }
}

#[cfg(test)]
mod tests {
    use super::Accuracy;
    use crate::error::ErrorKind;
    use crate::math::{Point3, Vec3};

    #[test]
    fn length_tolerance_grows_with_the_model() {
        let part = Accuracy::for_scale(0.01).unwrap();
        let building = Accuracy::for_scale(100.0).unwrap();

        assert_eq!(building.length() / part.length(), 10_000.0);
    }

    #[test]
    fn rejects_a_scale_that_is_not_positive() {
        let error = Accuracy::for_scale(0.0).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Input);
    }

    #[test]
    fn rejects_a_resolution_of_one_or_more() {
        let error = Accuracy::default().with_resolution(1.0).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Input);
    }

    #[test]
    fn points_closer_than_the_length_tolerance_are_the_same_point() {
        let accuracy = Accuracy::for_scale(10.0).unwrap();
        let origin = Point3::origin();

        let near = Point3::new(0.5 * accuracy.length(), 0.0, 0.0);
        let far = Point3::new(2.0 * accuracy.length(), 0.0, 0.0);

        assert!(accuracy.same_point(&origin, &near));
        assert!(!accuracy.same_point(&origin, &far));
    }

    #[test]
    fn parallel_ignores_length_and_direction_sense() {
        let accuracy = Accuracy::default();
        let axis = Vec3::new(0.0, 0.0, 2.0);

        assert!(accuracy.parallel(&axis, &Vec3::new(0.0, 0.0, -7.0)));
        assert!(!accuracy.parallel(&axis, &Vec3::new(0.0, 1e-6, 1.0)));
    }

    #[test]
    fn perpendicular_holds_for_orthogonal_vectors_only() {
        let accuracy = Accuracy::default();
        let axis = Vec3::new(0.0, 0.0, 2.0);

        assert!(accuracy.perpendicular(&axis, &Vec3::new(3.0, -1.0, 0.0)));
        assert!(!accuracy.perpendicular(&axis, &Vec3::new(3.0, 0.0, 1e-6)));
    }
}
