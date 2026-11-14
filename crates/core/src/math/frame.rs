//! Building and reading orthonormal frames.

use super::{Iso3, Point3, Sim3, Vec3};
use crate::error::Error;
use nalgebra::{Rotation3, Translation3};

/// Right-handed frame at `origin` with the given `z` axis; `x` is `x_hint` made
/// perpendicular to `z`.
///
/// Fails only when a direction cannot be normalised at all. Whether `x_hint` is too close
/// to `z` for the purpose at hand is for the caller to decide, with its `Accuracy`.
pub fn frame(origin: &Point3, z: &Vec3, x_hint: &Vec3) -> Result<Iso3, Error> {
    let z = unit(z, "frame axis")?;
    let x = unit(&(x_hint - z * x_hint.dot(&z)), "frame x direction")?;
    let y = z.cross(&x);
    let rotation = Rotation3::from_basis_unchecked(&[x, y, z]);
    Ok(Iso3::from_parts(Translation3::from(origin.coords), rotation))
}

/// Frame at `origin` with the given `z` axis and a deterministic choice of `x`:
/// perpendicular to `z`, starting from the world axis `z` is least aligned with.
pub fn frame_about(origin: &Point3, z: &Vec3) -> Result<Iso3, Error> {
    let weights = z.abs();
    let hint = if weights.x <= weights.y && weights.x <= weights.z {
        Vec3::x()
    } else if weights.y <= weights.z {
        Vec3::y()
    } else {
        Vec3::z()
    };
    frame(origin, z, &hint)
}

/// The smallest turn that takes the direction `from` to the direction `to`.
///
/// Rodrigues' formula about the axis `from × to` by the angle `atan2(|from × to|, from · to)`,
/// which stays accurate near both ends of its range. Directions that already agree give
/// the identity exactly; opposite ones a half turn `2kkᵀ − I` about an axis `k` square to
/// `from`, which is exact for an axis along a coordinate direction.
pub fn turning(from: &Vec3, to: &Vec3) -> Result<Rotation3<f64>, Error> {
    let (from, to) = (unit(from, "the direction turned from")?, unit(to, "the direction turned to")?);
    let across = from.cross(&to);
    let (sine, cosine) = (across.norm(), from.dot(&to));
    if sine > 0.0 {
        return Ok(Rotation3::from_axis_angle(
            &nalgebra::Unit::new_unchecked(across / sine),
            sine.atan2(cosine),
        ));
    }
    if cosine > 0.0 {
        return Ok(Rotation3::identity());
    }
    let axis = x_axis(&frame_about(&Point3::origin(), &from)?);
    Ok(Rotation3::from_matrix_unchecked(axis * axis.transpose() * 2.0 - nalgebra::Matrix3::identity()))
}

pub fn origin(frame: &Iso3) -> Point3 {
    Point3::from(frame.translation.vector)
}

pub fn x_axis(frame: &Iso3) -> Vec3 {
    frame.rotation.matrix().column(0).into_owned()
}

pub fn y_axis(frame: &Iso3) -> Vec3 {
    frame.rotation.matrix().column(1).into_owned()
}

pub fn z_axis(frame: &Iso3) -> Vec3 {
    frame.rotation.matrix().column(2).into_owned()
}

/// The frame after the whole space is moved by `by`; lengths measured in the frame scale
/// by `by.scaling()`, which the caller applies to its own radii.
pub fn placed(frame: &Iso3, by: &Sim3) -> Iso3 {
    let moved_origin = by * origin(frame);
    Iso3::from_parts(Translation3::from(moved_origin.coords), by.isometry.rotation * frame.rotation)
}

fn unit(vector: &Vec3, what: &str) -> Result<Vec3, Error> {
    let norm = vector.norm();
    if norm.is_finite() && norm > 0.0 {
        Ok(vector / norm)
    } else {
        Err(Error::geometry(format!("{what} has no direction")))
    }
}

#[cfg(test)]
mod tests {
    use super::{frame, frame_about, origin, placed, turning, x_axis, y_axis, z_axis};

    fn turned(from: Vec3, to: Vec3) -> Vec3 {
        turning(&from, &to).unwrap() * from.normalize()
    }

    #[test]
    fn a_turn_takes_one_direction_onto_the_other() {
        for (from, to) in [
            (Vec3::z(), Vec3::new(1.0, 1.0, 1.0)),
            (Vec3::new(1.0, -2.0, 0.5), Vec3::new(-3.0, 0.1, 2.0)),
            (Vec3::z(), Vec3::new(1e-9, 0.0, -1.0)),
        ] {
            assert!((turned(from, to) - to.normalize()).norm() < 1e-12, "{from:?} to {to:?}");
        }
    }

    #[test]
    fn agreeing_and_opposite_directions_turn_exactly() {
        assert_eq!(turning(&Vec3::z(), &Vec3::new(0.0, 0.0, 5.0)).unwrap(), Rotation3::identity());
        let half = turning(&Vec3::z(), &-Vec3::z()).unwrap();

        assert_eq!(half * Vec3::z(), -Vec3::z());
        assert!((half.matrix().determinant() - 1.0).abs() < 1e-15);
    }

    #[test]
    fn a_direction_of_no_length_cannot_be_turned() {
        assert!(turning(&Vec3::zeros(), &Vec3::x()).is_err());
    }
    use crate::error::ErrorKind;
    use crate::math::{Point3, Sim3, Vec3};
    use nalgebra::{Rotation3, Translation3};
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn the_x_hint_is_made_perpendicular_to_the_axis() {
        let built =
            frame(&Point3::new(1.0, 2.0, 3.0), &Vec3::new(0.0, 0.0, 5.0), &Vec3::new(1.0, 0.0, 9.0)).unwrap();

        assert_eq!(origin(&built), Point3::new(1.0, 2.0, 3.0));
        assert_eq!(x_axis(&built), Vec3::x());
        assert_eq!(y_axis(&built), Vec3::y());
        assert_eq!(z_axis(&built), Vec3::z());
    }

    #[test]
    fn a_frame_maps_local_coordinates_to_world() {
        let built = frame(&Point3::new(10.0, 0.0, 0.0), &Vec3::x(), &Vec3::y()).unwrap();

        let world = built * Point3::new(1.0, 2.0, 3.0);

        assert_eq!(world, Point3::new(13.0, 1.0, 2.0));
    }

    #[test]
    fn frame_about_is_right_handed_and_orthonormal_for_any_axis() {
        for axis in [Vec3::x(), Vec3::y(), Vec3::z(), Vec3::new(1.0, -2.0, 0.5)] {
            let built = frame_about(&Point3::origin(), &axis).unwrap();
            let (x, y, z) = (x_axis(&built), y_axis(&built), z_axis(&built));

            assert!((x.cross(&y) - z).norm() < 1e-15, "not right-handed for {axis:?}");
            assert!(x.dot(&z).abs() < 1e-15 && (x.norm() - 1.0).abs() < 1e-15);
            assert!((z - axis.normalize()).norm() < 1e-15);
        }
    }

    #[test]
    fn a_zero_axis_is_rejected() {
        let error = frame_about(&Point3::origin(), &Vec3::zeros()).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Geometry);
    }

    #[test]
    fn placing_moves_the_origin_and_turns_the_axes() {
        let local = frame(&Point3::new(1.0, 0.0, 0.0), &Vec3::z(), &Vec3::x()).unwrap();
        let quarter_turn = Rotation3::from_axis_angle(&Vec3::z_axis(), FRAC_PI_2);
        let by = Sim3::from_parts(Translation3::new(0.0, 0.0, 5.0), quarter_turn, 2.0);

        let moved = placed(&local, &by);

        assert!((origin(&moved) - Point3::new(0.0, 2.0, 5.0)).norm() < 1e-15);
        assert!((x_axis(&moved) - Vec3::y()).norm() < 1e-15);
        assert!((z_axis(&moved) - Vec3::z()).norm() < 1e-15);
    }
}
