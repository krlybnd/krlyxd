//! Finds the vanishing points of the segments.

use super::settle;
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::stage::Stage;
use krlyvis_kernel::ports::vanish::{vanish, VanishRequest};
use krlyvis_kernel::{Context, Plugin, Scene};

pub struct Vanish {
    pub method: Id,
}

impl Plugin<Stage> for Vanish {
    fn id(&self) -> Id {
        Id::new("vanish")
    }

    fn call(&self, cx: &Context, scene: &Scene) -> Attempt<Scene> {
        Attempt::from(self.run(cx, scene))
    }
}

impl Vanish {
    fn run(&self, cx: &Context, scene: &Scene) -> Result<Scene, Error> {
        let mut next = scene.clone();
        if scene.segments.len() < 2 {
            next.notes.push(format!("vanish: {} segments are too few", scene.segments.len()));
            return Ok(next);
        }
        let request = VanishRequest { segments: scene.segments.clone(), size: scene.image.size() };
        if let Some(points) = settle(&mut next.notes, "vanish", vanish(cx, &self.method, &request))? {
            next.vanishing = points;
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::Vanish;
    use krly3d_core::Id;
    use krlyvis_kernel::ports::stage::{contract, Stage};
    use krlyvis_kernel::{Ask, Gray, Scene, VisionBuilder};
    use std::sync::Arc;

    #[test]
    fn vanish_passes_the_contract_of_stages_and_notes_too_few_segments() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Stage>(Vanish { method: Id::new("vote") });
        let cx = builder.context();
        let scene = Scene::new(Arc::new(Gray::new(4, 4, vec![0.0; 16]).unwrap()), Ask::default());

        contract::check(&cx, "vanish", &scene).unwrap();
        let next = Vanish { method: Id::new("vote") }.run(&cx, &scene).unwrap();

        assert_eq!(next.notes, ["vanish: 0 segments are too few"]);
    }
}
