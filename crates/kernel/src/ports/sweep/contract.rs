//! What every sweep rule must satisfy: the surface it returns contains the profile edge
//! at every stage of the motion.

use super::SweepRequest;
use crate::ports::surface::Surface;
use krly3d_core::Accuracy;

pub fn check(
    surface: &dyn Surface,
    request: &SweepRequest,
    parameters: &[f64],
    accuracy: &Accuracy,
) -> Result<(), String> {
    for fraction in [0.0, 0.25, 0.5, 1.0] {
        let carried = request.motion.at(fraction);
        for &t in parameters {
            let distance = surface.distance(&(carried * request.curve.point(t)));
            if distance.abs() > accuracy.length() {
                return Err(format!(
                    "the edge point t={t} is {distance} off the swept surface after {fraction} of the motion"
                ));
            }
        }
    }
    Ok(())
}
