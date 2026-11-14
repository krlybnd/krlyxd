//! A focal length the caller knows, with the centre in the middle of the image.

use super::orient;
use krly3d_core::{Attempt, Id};
use krlyvis_kernel::image::centre;
use krlyvis_kernel::ports::camera::{Calibrate, Camera, CameraRequest};
use krlyvis_kernel::{Context, Plugin};

pub struct Given;

impl Plugin<Calibrate> for Given {
    fn id(&self) -> Id {
        Id::new("camera")
    }

    fn call(&self, _: &Context, request: &CameraRequest) -> Attempt<Camera> {
        match request.focal {
            Some(focal) if focal.is_finite() && focal > 0.0 => {
                Attempt::from(orient(focal, centre(request.size), &request.vanishing, "given"))
            }
            Some(focal) => Attempt::declined(format!("the focal length {focal} given is not positive")),
            None => Attempt::declined("no focal length was given"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Given;
    use krlyvis_kernel::ports::camera::{contract, Calibrate};
    use krlyvis_kernel::VisionBuilder;

    #[test]
    fn given_passes_the_contract_of_calibrations() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Calibrate>(Given);

        contract::check(&builder.context(), "camera", Some(700.0)).unwrap();
    }

    #[test]
    fn without_a_focal_length_it_declines() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Calibrate>(Given);

        assert!(contract::check(&builder.context(), "camera", None).is_err());
    }
}
