//! Finds the segments and numbers them longest first, so that ids are stable and the
//! strongest evidence is listed first.

use super::settle;
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::detect::detect;
use krlyvis_kernel::ports::stage::Stage;
use krlyvis_kernel::{Context, Plugin, Scene};

pub struct Detect {
    pub detector: Id,
}

impl Plugin<Stage> for Detect {
    fn id(&self) -> Id {
        Id::new("detect")
    }

    fn call(&self, cx: &Context, scene: &Scene) -> Attempt<Scene> {
        Attempt::from(self.run(cx, scene))
    }
}

impl Detect {
    fn run(&self, cx: &Context, scene: &Scene) -> Result<Scene, Error> {
        let mut next = scene.clone();
        if let Some(mut segments) =
            settle(&mut next.notes, "detect", detect(cx, &self.detector, &scene.image))?
        {
            segments.sort_by(|a, b| b.length().total_cmp(&a.length()));
            next.segments = segments;
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::Detect;
    use crate::detect::lsd::Lsd;
    use krly3d_core::Id;
    use krlyvis_kernel::ports::detect::Detect as Detectors;
    use krlyvis_kernel::ports::sample::Canvas;
    use krlyvis_kernel::ports::stage::{contract, Stage};
    use krlyvis_kernel::{Ask, Pixel, Scene, VisionBuilder};
    use std::sync::Arc;

    #[test]
    fn detect_passes_the_contract_of_stages_and_lists_the_longest_segment_first() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Detectors>(Lsd::default()).plugin::<Stage>(Detect { detector: Id::new("lsd") });
        let cx = builder.context();
        let mut canvas = Canvas::new((120, 80), 30.0);
        canvas.fill(
            &[
                Pixel::new(10.2, 10.3),
                Pixel::new(110.1, 12.7),
                Pixel::new(108.4, 40.2),
                Pixel::new(12.5, 60.6),
            ],
            220.0,
        );
        let scene = Scene::new(Arc::new(canvas.image().unwrap()), Ask::default());

        contract::check(&cx, "detect", &scene).unwrap();
        let found = Detect { detector: Id::new("lsd") }.run(&cx, &scene).unwrap();

        assert!(!found.segments.is_empty());
        assert!(found.segments.windows(2).all(|pair| pair[0].length() >= pair[1].length()));
    }
}
