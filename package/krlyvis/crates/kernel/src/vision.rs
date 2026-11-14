//! The one door of krlyvis.

use crate::context::Context;
use crate::image::Gray;
use crate::ports::read::read;
use crate::ports::stage::Stage;
use crate::scene::{Ask, Scene};
use krly3d_core::Error;
use std::sync::Arc;

/// Reads images and analyses them through the stages of its context.
pub struct Vision {
    cx: Context,
}

impl Vision {
    pub fn new(cx: Context) -> Self {
        Self { cx }
    }

    pub fn context(&self) -> &Context {
        &self.cx
    }

    /// Decodes an image of the named format.
    pub fn read(&self, format: &str, bytes: Vec<u8>) -> Result<Gray, Error> {
        read(&self.cx, format, bytes).into_result()
    }

    /// Runs every stage over the image, in order. A stage that cannot do its part says
    /// why in the notes of the scene; only a failure stops the analysis.
    pub fn analyse(&self, image: Arc<Gray>, ask: Ask) -> Result<Scene, Error> {
        let mut scene = Scene::new(image, ask);
        for stage in self.cx.stages() {
            let started = self.cx.clock().now();
            scene = self.cx.call::<Stage>(stage, &scene).into_result()?;
            let took = self.cx.clock().now().saturating_sub(started);
            scene.timings.push((stage.clone(), took));
        }
        Ok(scene)
    }
}

#[cfg(test)]
mod tests {
    use crate::builder::VisionBuilder;
    use crate::context::Context;
    use crate::image::Gray;
    use crate::plugin::Plugin;
    use crate::ports::stage::Stage;
    use crate::scene::{Ask, Scene};
    use krly3d_core::{Attempt, ErrorKind, FakeClock, Id};
    use std::sync::Arc;
    use std::time::Duration;

    /// Leaves a note and moves the clock, so that order and timing can be seen.
    struct Mark {
        name: &'static str,
        clock: Arc<FakeClock>,
    }

    impl Plugin<Stage> for Mark {
        fn id(&self) -> Id {
            Id::new(self.name)
        }

        fn call(&self, _: &Context, scene: &Scene) -> Attempt<Scene> {
            self.clock.advance(Duration::from_millis(3));
            let mut next = scene.clone();
            next.notes.push(self.name.to_string());
            Attempt::Done(next)
        }
    }

    fn image() -> Arc<Gray> {
        Arc::new(Gray::new(2, 2, vec![0.0; 4]).unwrap())
    }

    #[test]
    fn stages_run_in_the_order_given_and_are_timed() {
        let clock = Arc::new(FakeClock::default());
        let mut builder = VisionBuilder::new();
        builder
            .clock(clock.clone())
            .plugin::<Stage>(Mark { name: "first", clock: clock.clone() })
            .plugin::<Stage>(Mark { name: "second", clock: clock.clone() })
            .stages(&["second", "first"]);

        let scene = builder.build().analyse(image(), Ask::default()).unwrap();

        assert_eq!(scene.notes, ["second", "first"]);
        assert_eq!(
            scene.timings,
            [(Id::new("second"), Duration::from_millis(3)), (Id::new("first"), Duration::from_millis(3))]
        );
    }

    #[test]
    fn a_stage_nobody_registered_stops_the_analysis() {
        let mut builder = VisionBuilder::new();
        builder.stages(&["missing"]);

        let error = builder.build().analyse(image(), Ask::default()).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Unsupported);
    }
}
