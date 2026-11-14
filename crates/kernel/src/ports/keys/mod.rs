//! Port of the `keys` family: what can be said about one face in words a search can match.
//!
//! A key is a short text such as `kind:cylinder`, `r:3` or `n:+z`. Every key family is a
//! plugin registered under the prefix of its keys and answers for one face at a time, so
//! an index asks them all without knowing which exist. A family that has nothing to say
//! about a face declines, which leaves the face to the next plugin under the same
//! prefix: a new kind of surface brings its own radius or axis without editing a rule.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::topo::{Anchor, Body, Face, FaceId};
use krly3d_core::math::bounds::Bounds;
use krly3d_core::{Attempt, Error};
use std::sync::Arc;

pub struct Keys;

impl Family for Keys {
    const NAME: &'static str = "keys";
    type Request = KeysRequest;
    type Response = FaceKeys;
    type Context = crate::context::Context;
}

/// One face as the traversal of an index presents it. What every family would otherwise
/// work out again for itself is worked out once and handed along.
pub struct KeysRequest {
    pub body: Arc<Body>,
    pub face: FaceId,
    /// A point of the face and the outward normal there.
    pub anchor: Anchor,
    /// The box around the whole body.
    pub bounds: Bounds,
}

impl KeysRequest {
    pub fn target(&self) -> Result<&Face, Error> {
        self.body
            .faces
            .get(self.face)
            .ok_or_else(|| Error::not_found(format!("the body has no face number {}", self.face)))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaceKeys {
    /// Each key reads `prefix:value`, with the prefix of the family that states it.
    pub keys: Vec<String>,
    /// The faces the family relates this one to.
    pub related: Vec<FaceId>,
}

/// What every registered key family says about the face, in the order of the prefixes.
pub fn keys(cx: &Context, request: &KeysRequest) -> Attempt<FaceKeys> {
    let Some(registry) = cx.registry::<Keys>() else {
        return Attempt::declined(format!("no {} plugin is registered at all", Keys::NAME));
    };
    let mut all = FaceKeys::default();
    for prefix in registry.ids() {
        match registry.call(cx, prefix, request) {
            Attempt::Done(found) => {
                all.keys.extend(found.keys);
                all.related.extend(found.related);
            }
            // The family has nothing to say about this face.
            Attempt::Declined(_) => {}
            Attempt::Failed(error) => return Attempt::Failed(error),
        }
    }
    Attempt::Done(all)
}
