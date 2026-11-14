//! Fixed test data shared by the contract tests.

use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::{Point3, Sim3, Vec3};
use nalgebra::{Rotation3, Translation3, Unit};

/// A similarity with no special alignment: a turn about a skew axis, a shift and a scale.
/// Contract tests move a plugin's object by it and expect the object to follow.
pub fn similarity() -> Sim3 {
    let rotation = Rotation3::from_axis_angle(&Unit::new_normalize(Vec3::new(1.0, 2.0, 3.0)), 0.7);
    Sim3::from_parts(Translation3::new(0.5, -1.5, 2.0), rotation, 1.5)
}

/// A reflection in a plane with no special alignment. Contract tests reflect a plugin's
/// object in it and expect the image to be the reflected object.
pub fn mirror() -> Result<Mirror, String> {
    Mirror::new(Point3::new(0.3, -1.2, 0.8), Vec3::new(2.0, -1.0, 0.5)).map_err(|error| error.to_string())
}

/// Step of the central differences the contract tests use as an independent check of
/// derivatives: the cube root of the machine epsilon balances truncation against round-off.
pub fn difference_step() -> f64 {
    f64::EPSILON.cbrt()
}
