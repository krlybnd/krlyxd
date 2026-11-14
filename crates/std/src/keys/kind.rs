//! `kind:` — the kind of surface the face lies on: `kind:plane`, `kind:cylinder`, …

use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::keys::{FaceKeys, Keys, KeysRequest};
use krly3d_kernel::{Context, Plugin};

pub struct KindKey;

impl Plugin<Keys> for KindKey {
    fn id(&self) -> Id {
        Id::new("kind")
    }

    fn call(&self, _: &Context, request: &KeysRequest) -> Attempt<FaceKeys> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        // The first class of a surface is its kind.
        match face.surface.classes().first() {
            Some(kind) => Attempt::Done(FaceKeys { keys: vec![format!("kind:{kind}")], related: Vec::new() }),
            None => Attempt::declined("the surface states no class"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::keys::testing::keys;
    use crate::tool::testing::{context, made};
    use krly3d_kernel::schema::Params;

    #[test]
    fn a_face_is_keyed_by_the_kind_of_its_surface() {
        let cx = context();
        let can = made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[]);
        let ring = made(&cx, "torus", Params::new().with("ring", 6.0).with("tube", 2.0), &[]);

        assert_eq!(keys(&cx, "kind", &can, "top"), ["kind:plane"]);
        assert_eq!(keys(&cx, "kind", &can, "side"), ["kind:cylinder"]);
        assert_eq!(keys(&cx, "kind", &ring, "skin"), ["kind:torus"]);
    }
}
