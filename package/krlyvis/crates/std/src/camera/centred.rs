//! The centre in the middle of the image and the focal length from two vanishing points
//! of square directions.
//!
//! Directions `K⁻¹a` and `K⁻¹b` are square when
//! `(a - c a_w)·(b - c b_w) + f² a_w b_w = 0`, which gives `f²` in closed form. Of the
//! pairs among the three strongest points the one with the most support that gives a
//! plausible focal length is used.

use super::{orient, plausible};
use krly3d_core::{Attempt, Id};
use krlyvis_kernel::image::centre;
use krlyvis_kernel::ports::camera::{Calibrate, Camera, CameraRequest};
use krlyvis_kernel::{Context, Pixel, Plugin};
use nalgebra::Vector3;

pub struct Centred;

impl Plugin<Calibrate> for Centred {
    fn id(&self) -> Id {
        Id::new("camera")
    }

    fn call(&self, _: &Context, request: &CameraRequest) -> Attempt<Camera> {
        let middle = centre(request.size);
        let points = &request.vanishing[..request.vanishing.len().min(3)];
        let mut best: Option<(f64, f64)> = None;
        for (index, first) in points.iter().enumerate() {
            for second in &points[index + 1..] {
                let Some(focal) = focal(&first.point, &second.point, middle) else { continue };
                let score = first.support * second.support;
                if plausible(focal, request.size) && best.is_none_or(|(most, _)| score > most) {
                    best = Some((score, focal));
                }
            }
        }
        match best {
            Some((_, focal)) => Attempt::from(orient(focal, middle, &request.vanishing, "centred")),
            None => Attempt::declined(
                "no two vanishing points give a plausible focal length with the centre in the middle of the image",
            ),
        }
    }
}

/// The focal length for which two homogeneous vanishing points belong to square
/// directions, if there is one.
fn focal(a: &Vector3<f64>, b: &Vector3<f64>, centre: Pixel) -> Option<f64> {
    let across =
        (a.x - centre.x * a.z) * (b.x - centre.x * b.z) + (a.y - centre.y * a.z) * (b.y - centre.y * b.z);
    let squared = -across / (a.z * b.z);
    (squared.is_finite() && squared > 0.0).then(|| squared.sqrt())
}

#[cfg(test)]
mod tests {
    use super::Centred;
    use krlyvis_kernel::ports::camera::{contract, Calibrate};
    use krlyvis_kernel::VisionBuilder;

    #[test]
    fn centred_passes_the_contract_of_calibrations() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Calibrate>(Centred);

        contract::check(&builder.context(), "camera", None).unwrap();
    }
}
