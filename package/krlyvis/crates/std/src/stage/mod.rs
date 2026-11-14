//! The stages of the standard analysis, in order: find segments, find their vanishing
//! points, work out the camera, measure in the plane asked for, and find how high and how
//! far the vertical segments standing on the ground are.

pub mod calibrate;
pub mod depth;
pub mod detect;
pub mod measure;
pub mod vanish;

use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::stage::Stage;
use krlyvis_kernel::VisionBuilder;

pub fn register(builder: &mut VisionBuilder) {
    builder
        .plugin::<Stage>(detect::Detect { detector: Id::new("lsd") })
        .plugin::<Stage>(vanish::Vanish { method: Id::new("vote") })
        .plugin::<Stage>(calibrate::Calibrate)
        .plugin::<Stage>(measure::Measure { method: Id::new("plane") })
        .plugin::<Stage>(depth::Depth { method: Id::new("ground") })
        .stages(&["detect", "vanish", "camera", "measure", "depth"]);
}

/// A family's answer as a stage takes it: a declined request becomes a note and leaves
/// the scene as it was, a failure stops the analysis.
fn settle<T>(notes: &mut Vec<String>, stage: &str, attempt: Attempt<T>) -> Result<Option<T>, Error> {
    match attempt {
        Attempt::Done(value) => Ok(Some(value)),
        Attempt::Declined(reason) => {
            notes.push(format!("{stage}: {reason}"));
            Ok(None)
        }
        Attempt::Failed(error) => Err(error),
    }
}
