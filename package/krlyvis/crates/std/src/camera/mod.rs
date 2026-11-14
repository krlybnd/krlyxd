//! Calibrations: the camera from the vanishing points. The `camera` chain asks for a
//! given focal length first, then assumes the centre in the middle of the image;
//! `orthocentre` also finds the centre, from three finite points.

pub mod centred;
pub mod given;
pub mod orthocentre;

use krly3d_core::{Error, Id};
use krlyvis_kernel::image::diagonal;
use krlyvis_kernel::ports::camera::{Calibrate, Camera};
use krlyvis_kernel::ports::vanish::Vanishing;
use krlyvis_kernel::{Pixel, VisionBuilder};
use nalgebra::{Matrix3, Vector3};

pub fn register(builder: &mut VisionBuilder) {
    builder
        .plugin::<Calibrate>(given::Given)
        .plugin::<Calibrate>(centred::Centred)
        .plugin::<Calibrate>(orthocentre::Orthocentre);
}

/// Focal lengths believed, in image diagonals: from about 136° down to about 3° of view
/// across the diagonal.
const FOCAL_RANGE: (f64, f64) = (0.2, 20.0);

fn plausible(focal: f64, size: (usize, usize)) -> bool {
    let diagonals = focal / diagonal(size);
    diagonals.is_finite() && (FOCAL_RANGE.0..=FOCAL_RANGE.1).contains(&diagonals)
}

/// The camera with the given focal length and centre, its axes taken from the first
/// three vanishing points: the vertical is the direction most along the image's y axis,
/// turned to point up; x is the strongest of the others, made square to the vertical;
/// y completes a right-handed frame, and the third point, if any, is checked against it.
fn orient(focal: f64, centre: Pixel, vanishing: &[Vanishing], method: &str) -> Result<Camera, Error> {
    let direction = |point: &Vector3<f64>| {
        Vector3::new((point.x - centre.x * point.z) / focal, (point.y - centre.y * point.z) / focal, point.z)
            .normalize()
    };
    let found: Vec<(usize, Vector3<f64>)> =
        vanishing.iter().take(3).enumerate().map(|(index, point)| (index, direction(&point.point))).collect();
    let [(_, first), (_, second), ..] = found[..] else {
        return Err(Error::input("two vanishing points are needed to orient a camera"));
    };
    let mut candidates: Vec<(Option<usize>, Vector3<f64>)> =
        found.iter().map(|&(index, direction)| (Some(index), direction)).collect();
    if found.len() == 2 {
        candidates.push((None, first.cross(&second).normalize()));
    }
    let (up_source, up) = candidates
        .iter()
        .copied()
        .max_by(|a, b| a.1.y.abs().total_cmp(&b.1.y.abs()))
        .ok_or_else(|| Error::input("no vertical direction"))?;
    let up = if up.y > 0.0 { -up } else { up };
    let others: Vec<&(usize, Vector3<f64>)> =
        found.iter().filter(|(index, _)| Some(*index) != up_source).collect();
    let (x_source, x) = **others.first().ok_or_else(|| Error::input("no horizontal direction"))?;
    let x = (x - up * up.dot(&x)).normalize();
    let y = up.cross(&x);
    Ok(Camera {
        focal,
        centre,
        rotation: Matrix3::from_columns(&[x, y, up]),
        axes: [Some(x_source), others.get(1).map(|(index, _)| *index), up_source],
        off_square: others
            .get(1)
            .map(|(_, found)| found.cross(&y).norm().atan2(found.dot(&y).abs()).to_degrees()),
        method: Id::new(method),
    })
}
