//! Works out the camera from the vanishing points, through the chain the caller named.

use super::settle;
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::camera::{calibrate, CameraRequest};
use krlyvis_kernel::ports::stage::Stage;
use krlyvis_kernel::{Context, Plugin, Scene};

pub struct Calibrate;

impl Plugin<Stage> for Calibrate {
    fn id(&self) -> Id {
        Id::new("camera")
    }

    fn call(&self, cx: &Context, scene: &Scene) -> Attempt<Scene> {
        Attempt::from(run(cx, scene))
    }
}

fn run(cx: &Context, scene: &Scene) -> Result<Scene, Error> {
    let mut next = scene.clone();
    if scene.vanishing.len() < 2 {
        next.notes.push(format!("camera: two vanishing points are needed, {} found", scene.vanishing.len()));
        return Ok(next);
    }
    let request = CameraRequest {
        vanishing: scene.vanishing.clone(),
        size: scene.image.size(),
        focal: scene.ask.focal,
    };
    next.camera = settle(&mut next.notes, "camera", calibrate(cx, &scene.ask.camera, &request))?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::{run, Calibrate};
    use krlyvis_kernel::ports::stage::{contract, Stage};
    use krlyvis_kernel::{Ask, Gray, Scene, VisionBuilder};
    use std::sync::Arc;

    #[test]
    fn camera_passes_the_contract_of_stages_and_notes_missing_points() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Stage>(Calibrate);
        let cx = builder.context();
        let scene = Scene::new(Arc::new(Gray::new(4, 4, vec![0.0; 16]).unwrap()), Ask::default());

        contract::check(&cx, "camera", &scene).unwrap();
        let next = run(&cx, &scene).unwrap();

        assert_eq!(next.notes, ["camera: two vanishing points are needed, 0 found"]);
        assert_eq!(next.camera, None);
    }
}
