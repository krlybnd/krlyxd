//! Port of the `sweep` family: the surface a profile edge traces under a motion.
//!
//! Rules are keyed `curve|motion`, e.g. `line|shift` or `circle|turn`.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::ports::curve::Curve;
use crate::ports::pair;
use crate::ports::surface::Surface;
use krly3d_core::math::{Iso3, Point3, Vec3};
use krly3d_core::{Attempt, Error};
use nalgebra::{Rotation3, Translation3, Unit};
use std::f64::consts::TAU;
use std::sync::Arc;

pub struct Sweep;

impl Family for Sweep {
    const NAME: &'static str = "sweep";
    type Request = SweepRequest;
    type Response = Arc<dyn Surface>;
    type Context = crate::context::Context;
}

pub struct SweepRequest {
    pub curve: Arc<dyn Curve>,
    pub motion: Motion,
}

/// A one-parameter rigid motion. Every such motion is a shift, a turn or their
/// combination, so the set is closed by geometry and not by the kernel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    Shift {
        by: Vec3,
    },
    /// `angle` radians about the line through `origin` along `axis`.
    Turn {
        origin: Point3,
        axis: Unit<Vec3>,
        angle: f64,
    },
}

impl Motion {
    pub fn shift(by: Vec3) -> Result<Self, Error> {
        if by.norm().is_finite() && by.norm() > 0.0 {
            Ok(Self::Shift { by })
        } else {
            Err(Error::input(format!("a shift needs a finite, non-zero vector, got {by:?}")))
        }
    }

    pub fn turn(origin: Point3, axis: Vec3, angle: f64) -> Result<Self, Error> {
        let axis = Unit::try_new(axis, 0.0)
            .filter(|unit| unit.norm().is_finite())
            .ok_or_else(|| Error::input(format!("a turn needs an axis with a direction, got {axis:?}")))?;
        if angle.is_finite() && angle != 0.0 && angle.abs() <= TAU {
            Ok(Self::Turn { origin, axis, angle })
        } else {
            Err(Error::input(format!("a turn needs a non-zero angle of at most one full turn, got {angle}")))
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Shift { .. } => "shift",
            Self::Turn { .. } => "turn",
        }
    }

    /// Where the motion has carried the space after `fraction` of it (0 = start, 1 = end).
    pub fn at(&self, fraction: f64) -> Iso3 {
        match self {
            Self::Shift { by } => Iso3::from_parts(Translation3::from(by * fraction), Rotation3::identity()),
            Self::Turn { origin, axis, angle } => {
                Iso3::rotation_wrt_point(Rotation3::from_axis_angle(axis, angle * fraction), *origin)
            }
        }
    }
}

/// Asks the sweep rules for the classes of the curve and the kind of the motion.
pub fn sweep(cx: &Context, request: &SweepRequest) -> Attempt<Arc<dyn Surface>> {
    pair::ask::<Sweep>(cx, request.curve.classes(), &[request.motion.kind()], request)
}

#[cfg(test)]
mod tests {
    use super::Motion;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::ErrorKind;
    use std::f64::consts::PI;

    #[test]
    fn a_shift_carries_points_along_its_vector() {
        let motion = Motion::shift(Vec3::new(0.0, 0.0, 10.0)).unwrap();

        let halfway = motion.at(0.5) * Point3::new(1.0, 2.0, 3.0);

        assert_eq!(halfway, Point3::new(1.0, 2.0, 8.0));
    }

    #[test]
    fn a_turn_carries_points_around_its_axis_line() {
        let motion = Motion::turn(Point3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 2.0), PI).unwrap();

        let end = motion.at(1.0) * Point3::new(3.0, 0.0, 5.0);

        assert!((end - Point3::new(-1.0, 0.0, 5.0)).norm() < 1e-15);
    }

    #[test]
    fn motions_that_move_nothing_are_rejected() {
        let no_shift = Motion::shift(Vec3::zeros()).unwrap_err();
        let no_axis = Motion::turn(Point3::origin(), Vec3::zeros(), 1.0).unwrap_err();
        let no_angle = Motion::turn(Point3::origin(), Vec3::z(), 0.0).unwrap_err();

        assert_eq!([no_shift.kind, no_axis.kind, no_angle.kind], [ErrorKind::Input; 3]);
    }
}
