//! Finds the height and the distance of every vertical segment standing on the ground,
//! once the camera is known.

use super::settle;
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::depth::{depth, DepthRequest, Unit};
use krlyvis_kernel::ports::stage::Stage;
use krlyvis_kernel::{Context, Plugin, Scene};

pub struct Depth {
    pub method: Id,
}

impl Plugin<Stage> for Depth {
    fn id(&self) -> Id {
        Id::new("depth")
    }

    fn call(&self, cx: &Context, scene: &Scene) -> Attempt<Scene> {
        Attempt::from(self.run(cx, scene))
    }
}

impl Depth {
    fn run(&self, cx: &Context, scene: &Scene) -> Result<Scene, Error> {
        let mut next = scene.clone();
        // Without a camera there is no ground to stand on; the camera stage said why.
        let Some(camera) = &scene.camera else { return Ok(next) };
        let request = DepthRequest {
            camera: camera.clone(),
            segments: scene.segments.clone(),
            eye: scene.ask.eye,
            known: scene.ask.known,
        };
        if let Some(found) = settle(&mut next.notes, "depth", depth(cx, &self.method, &request))? {
            let unmatched = match (found.unit, scene.ask.known, scene.ask.plane) {
                (Unit::CameraHeight, Some(known), None) => Some(known.segment),
                _ => None,
            };
            if let Some(segment) = unmatched {
                next.notes.push(format!(
                    "depth: s{segment} does not stand on the ground, so it cannot set the unit of depths"
                ));
            }
            next.depth = Some(found);
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::Depth;
    use krly3d_core::Id;
    use krlyvis_kernel::ports::stage::{contract, Stage};
    use krlyvis_kernel::{Ask, Gray, Scene, VisionBuilder};
    use std::sync::Arc;

    #[test]
    fn depth_passes_the_contract_of_stages_and_waits_for_a_camera() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Stage>(Depth { method: Id::new("ground") });
        let cx = builder.context();
        let scene = Scene::new(Arc::new(Gray::new(4, 4, vec![0.0; 16]).unwrap()), Ask::default());

        contract::check(&cx, "depth", &scene).unwrap();
        let next = Depth { method: Id::new("ground") }.run(&cx, &scene).unwrap();

        assert_eq!(next.depth, None);
        assert!(next.notes.is_empty());
    }
}
