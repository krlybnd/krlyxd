//! Measures, in the plane asked for, the segments running along its two axes and the
//! segment of known length.

use super::settle;
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::camera::Camera;
use krlyvis_kernel::ports::measure::{measure, MeasureRequest};
use krlyvis_kernel::ports::stage::Stage;
use krlyvis_kernel::{Axis, Context, Plugin, Scene};

pub struct Measure {
    pub method: Id,
}

impl Plugin<Stage> for Measure {
    fn id(&self) -> Id {
        Id::new("measure")
    }

    fn call(&self, cx: &Context, scene: &Scene) -> Attempt<Scene> {
        Attempt::from(self.run(cx, scene))
    }
}

impl Measure {
    fn run(&self, cx: &Context, scene: &Scene) -> Result<Scene, Error> {
        let mut next = scene.clone();
        let Some(normal) = scene.ask.plane else { return Ok(next) };
        if let Some(known) = scene.ask.known {
            if known.segment >= scene.segments.len() {
                return Err(Error::input(format!(
                    "s{} is not a segment: {} were found",
                    known.segment,
                    scene.segments.len()
                )));
            }
        }
        let Some(camera) = &scene.camera else {
            next.notes.push("measure: there is no camera to measure with".to_string());
            return Ok(next);
        };
        let which = along_plane(scene, camera, normal);
        let request = MeasureRequest {
            camera: camera.clone(),
            normal,
            segments: scene.segments.clone(),
            which: which.clone(),
            known: scene.ask.known,
        };
        if let Some(measured) = settle(&mut next.notes, "measure", measure(cx, &self.method, &request))? {
            let missed = which.len() - measured.len();
            if missed > 0 {
                next.notes.push(format!("measure: {missed} segments reach past the horizon of the plane"));
            }
            next.measured = measured;
        }
        Ok(next)
    }
}

/// The segments running towards the vanishing points of the plane's two axes, and the
/// known one, by id.
fn along_plane(scene: &Scene, camera: &Camera, normal: Axis) -> Vec<usize> {
    let mut which: Vec<usize> = normal
        .others()
        .iter()
        .filter_map(|axis| camera.axes[axis.index()])
        .filter_map(|point| scene.vanishing.get(point))
        .flat_map(|point| point.members.iter().copied())
        .chain(scene.ask.known.map(|known| known.segment))
        .collect();
    which.sort_unstable();
    which.dedup();
    which
}

#[cfg(test)]
mod tests {
    use super::Measure;
    use krly3d_core::{ErrorKind, Id};
    use krlyvis_kernel::ports::stage::{contract, Stage};
    use krlyvis_kernel::{Ask, Axis, Gray, Known, Scene, VisionBuilder};
    use std::sync::Arc;

    fn scene(ask: Ask) -> Scene {
        Scene::new(Arc::new(Gray::new(4, 4, vec![0.0; 16]).unwrap()), ask)
    }

    #[test]
    fn measure_passes_the_contract_of_stages_and_notes_a_missing_camera() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Stage>(Measure { method: Id::new("plane") });
        let cx = builder.context();
        let ask = Ask { plane: Some(Axis::Y), ..Ask::default() };

        contract::check(&cx, "measure", &scene(ask.clone())).unwrap();
        let next = Measure { method: Id::new("plane") }.run(&cx, &scene(ask)).unwrap();

        assert_eq!(next.notes, ["measure: there is no camera to measure with"]);
    }

    #[test]
    fn a_known_segment_that_was_not_found_is_an_input_error() {
        let cx = VisionBuilder::new().context();
        let ask =
            Ask { plane: Some(Axis::Y), known: Some(Known { segment: 3, length: 1.0 }), ..Ask::default() };

        let error = Measure { method: Id::new("plane") }.run(&cx, &scene(ask)).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Input);
    }
}
