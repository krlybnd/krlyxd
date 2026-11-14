//! `made:` — the operation a face was made by, read off its name: a face called
//! `chamfer(front|top)` or `hole:side` is `made:chamfer` or `made:hole`.

use super::word;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::keys::{FaceKeys, Keys, KeysRequest};
use krly3d_kernel::{Context, Plugin};

pub struct MadeKey;

impl Plugin<Keys> for MadeKey {
    fn id(&self) -> Id {
        Id::new("made")
    }

    fn call(&self, _: &Context, request: &KeysRequest) -> Attempt<FaceKeys> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        // An operation names its faces `operation(…)` or `operation:…`.
        match face.name.split_once(['(', ':']) {
            Some((operation, _)) if !operation.is_empty() => Attempt::Done(FaceKeys {
                keys: vec![format!("made:{}", word(operation))],
                related: Vec::new(),
            }),
            _ => Attempt::declined(format!("the name `{}` states no operation", face.name)),
        }
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
    fn the_operation_is_what_stands_before_a_bracket_or_a_colon_in_the_name() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 4.0).with("d", 2.0).with("h", 1.0), &[]);
        let mut renamed = Body::clone(&block);
        renamed.faces[0].name = "chamfer(front|top)".to_string();
        renamed.faces[1].name = "hole:side".to_string();
        let renamed = Arc::new(renamed);

        assert_eq!(keys(&cx, "made", &renamed, "chamfer(front|top)"), ["made:chamfer"]);
        assert_eq!(keys(&cx, "made", &renamed, "hole:side"), ["made:hole"]);
    }

    #[test]
    fn a_plain_name_states_no_operation() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 4.0).with("d", 2.0).with("h", 1.0), &[]);

        assert!(keys(&cx, "made", &block, "top").is_empty());
    }
}
