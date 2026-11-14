//! `n:` — the direction a flat face looks in: `n:+z` for a face that looks up.

use super::direction;
use crate::surface::Plane;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::keys::{FaceKeys, Keys, KeysRequest};
use krly3d_kernel::{Context, Plugin};

pub struct NormalKey;

impl Plugin<Keys> for NormalKey {
    fn id(&self) -> Id {
        Id::new("n")
    }

    fn call(&self, _: &Context, request: &KeysRequest) -> Attempt<FaceKeys> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        if !face.surface.as_any().is::<Plane>() {
            return Attempt::declined("only a flat face looks in one direction");
        }
        // The outward normal: the plane's own normal may point into the material.
        Attempt::Done(FaceKeys {
            keys: vec![format!("n:{}", direction(&request.anchor.normal))],
            related: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::keys::testing::keys;
    use crate::tool::testing::{context, made};
    use krly3d_kernel::schema::Params;

    #[test]
    fn a_flat_face_is_keyed_by_the_direction_it_looks_in() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 4.0).with("d", 2.0).with("h", 1.0), &[]);

        assert_eq!(keys(&cx, "n", &block, "top"), ["n:+z"]);
        assert_eq!(keys(&cx, "n", &block, "bottom"), ["n:-z"]);
        assert_eq!(keys(&cx, "n", &block, "front"), ["n:-y"]);
    }

    #[test]
    fn a_direction_off_the_axes_is_written_without_spaces() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 4.0).with("d", 2.0).with("h", 1.0), &[]);
        let turned = made(&cx, "transform", Params::new().with("turn", 45.0), &[block]);

        assert_eq!(keys(&cx, "n", &turned, "right"), ["n:0.707107,0.707107,0"]);
    }

    #[test]
    fn a_curved_face_has_no_direction() {
        let cx = context();
        let can = made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[]);

        assert!(keys(&cx, "n", &can, "side").is_empty());
    }
}
