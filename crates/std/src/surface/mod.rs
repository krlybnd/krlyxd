//! Surfaces: implementations of the `surface` port.

pub mod cone;
pub mod cylinder;
pub mod ellipsoid;
pub mod nurbs;
pub mod plane;
pub mod sphere;
pub mod torus;

pub use cone::Cone;
pub use cylinder::Cylinder;
pub use ellipsoid::Ellipsoid;
pub use nurbs::Nurbs;
pub use plane::Plane;
pub use sphere::Sphere;
pub use torus::Torus;

use crate::curve::{Circle, Line};
use krly3d_core::math::{Iso3, Point3};
use krly3d_core::Error;
use krly3d_kernel::ports::surface::Stretch;
use nalgebra::Translation3;
use std::sync::Arc;

/// A radius or similar size given to a surface constructor.
fn positive(what: &str, value: f64) -> Result<f64, Error> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(Error::input(format!("{what} must be a positive finite number, got {value}")))
    }
}

/// The straight stretch from one point to another, unless they coincide.
fn straight(from: Point3, to: Point3) -> Option<Stretch> {
    let line = Line::new(from, to - from).ok()?;
    Some(Stretch { curve: Arc::new(line), from: 0.0, to: 1.0 })
}

/// The stretch of a circle from the angle `from` through the signed turn `by`, unless
/// there is no circle (a zero radius at an apex) or nothing to turn.
fn arc(circle: Result<Circle, Error>, from: f64, by: f64) -> Option<Stretch> {
    let circle = circle.ok().filter(|_| by != 0.0)?;
    Some(Stretch { curve: Arc::new(circle), from, to: from + by })
}

/// The same axes at another origin.
fn shifted(frame: &Iso3, origin: Point3) -> Iso3 {
    Iso3::from_parts(Translation3::from(origin.coords), frame.rotation)
}
