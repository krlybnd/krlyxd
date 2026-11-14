//! What every calibration must satisfy: the exact vanishing points of a known camera
//! give back its focal length, its centre and its axes; the horizontal axes may come
//! back reversed, the vertical one must point up.

use super::{calibrate, Camera, CameraRequest};
use crate::context::Context;
use crate::ports::sample;
use crate::ports::vanish::Vanishing;
use crate::scene::Axis;
use krly3d_core::{Attempt, Id};
use nalgebra::Matrix3;

/// Relative error allowed on the focal length and absolute error on unit directions.
const CLOSE: f64 = 1e-9;

/// `focal` is passed on as the focal length the caller knows.
pub fn check(cx: &Context, method: &str, focal: Option<f64>) -> Result<(), String> {
    let truth = sample::camera(35.0, 15.0);
    let vanishing = Axis::ALL
        .iter()
        .map(|&axis| Vanishing {
            point: truth.vanishing(axis),
            members: Vec::new(),
            support: 300.0 - 100.0 * axis.index() as f64,
        })
        .collect();
    let request = CameraRequest { vanishing, size: sample::SIZE, focal };
    match calibrate(cx, &Id::new(method), &request) {
        Attempt::Done(found) => agrees(&found, &truth).map_err(|reason| format!("`{method}`: {reason}")),
        other => Err(format!("`{method}` did not calibrate: {other:?}")),
    }
}

fn agrees(found: &Camera, truth: &Camera) -> Result<(), String> {
    if (found.focal - truth.focal).abs() > CLOSE * truth.focal {
        return Err(format!("focal length {} instead of {}", found.focal, truth.focal));
    }
    if (found.centre - truth.centre).norm() > CLOSE * truth.focal {
        return Err(format!("centre {} instead of {}", found.centre, truth.centre));
    }
    if (found.rotation.transpose() * found.rotation - Matrix3::identity()).norm() > CLOSE
        || found.rotation.determinant() < 0.0
    {
        return Err(format!("{} is not a rotation", found.rotation));
    }
    for axis in Axis::ALL {
        let (a, b) = (found.axis(axis), truth.axis(axis));
        let matches = (a - b).norm() <= CLOSE || (axis != Axis::Z && (a + b).norm() <= CLOSE);
        if !matches {
            return Err(format!("axis {} is {a} instead of {b}", axis.letter()));
        }
    }
    if found.off_square.is_none_or(|degrees| degrees > CLOSE) {
        return Err(format!("the third point is {:?} degrees off square", found.off_square));
    }
    if found.axes != [Some(0), Some(1), Some(2)] {
        return Err(format!("axes taken from vanishing points {:?}", found.axes));
    }
    Ok(())
}
