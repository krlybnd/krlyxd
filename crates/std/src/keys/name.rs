//! `name:` — the name of the face, so that a search can ask for one face outright.

use super::word;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::keys::{FaceKeys, Keys, KeysRequest};
use krly3d_kernel::{Context, Plugin};

pub struct NameKey;

impl Plugin<Keys> for NameKey {
    fn id(&self) -> Id {
        Id::new("name")
    }

    fn call(&self, _: &Context, request: &KeysRequest) -> Attempt<FaceKeys> {
        request
            .target()
            .map(|face| FaceKeys { keys: vec![format!("name:{}", word(&face.name))], related: Vec::new() })
            .into()
    }
}

#[cfg(test)]
mod tests {
    use crate::keys::testing::keys;
    use crate::tool::testing::{context, made};
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::topo::Body;
    use std::sync::Arc;

    #[test]
    fn a_face_is_keyed_by_its_name() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 4.0).with("d", 2.0).with("h", 1.0), &[]);

        assert_eq!(keys(&cx, "name", &block, "top"), ["name:top"]);
    }

    #[test]
    fn a_name_with_a_space_in_it_is_keyed_as_one_word() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 4.0).with("d", 2.0).with("h", 1.0), &[]);
        let mut renamed = Body::clone(&block);
        renamed.faces[0].name = "datum face".to_string();

        assert_eq!(keys(&cx, "name", &Arc::new(renamed), "datum face"), ["name:datum_face"]);
    }
}
