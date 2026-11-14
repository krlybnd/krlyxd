//! Key families: what a face is, how it lies and where, in words a search can match.

pub mod axis;
pub mod cell;
pub mod coaxial;
pub mod kind;
pub mod made;
pub mod name;
pub mod normal;
pub mod radius;

use krly3d_core::math::Vec3;
use krly3d_core::text;
use krly3d_kernel::ports::keys::Keys;
use krly3d_kernel::KernelBuilder;

pub fn register(builder: &mut KernelBuilder) {
    builder
        .plugin::<Keys>(name::NameKey)
        .plugin::<Keys>(kind::KindKey)
        .plugin::<Keys>(made::MadeKey)
        .plugin::<Keys>(normal::NormalKey)
        .plugin::<Keys>(radius::RadiusKey)
        .plugin::<Keys>(axis::AxisKey)
        .plugin::<Keys>(cell::CellKey)
        .plugin::<Keys>(coaxial::CoaxialKey);
}

/// A name as it stands in a key. A search is split at spaces and reads `^` as the start
/// of a weight, so a key can hold neither.
fn word(name: &str) -> String {
    name.replace(|c: char| c.is_whitespace() || c == '^', "_")
}

/// A direction as it stands in a key: `+z` along a world axis, else the components of
/// the unit vector with commas between them.
fn direction(vector: &Vec3) -> String {
    text::direction(vector).trim_matches(['(', ')']).replace(' ', ",")
}

#[cfg(test)]
pub(crate) mod testing {
    use krly3d_core::{Attempt, Id};
    use krly3d_kernel::ports::keys::{contract, FaceKeys, Keys, KeysRequest};
    use krly3d_kernel::topo::{anchor, body_bounds, Body};
    use krly3d_kernel::Context;
    use std::sync::Arc;

    /// The face called `name` as an index would present it to a key family.
    pub(crate) fn face(cx: &Context, body: &Arc<Body>, name: &str) -> KeysRequest {
        let (face, _) = body.face(name).unwrap();
        KeysRequest {
            body: Arc::clone(body),
            face,
            anchor: anchor(cx, body, face).into_result().unwrap(),
            bounds: body_bounds(cx, body).into_result().unwrap(),
        }
    }

    /// What the key family `prefix` says about a face, after checking the contract of
    /// the family on it; `None` when it has nothing to say.
    pub(crate) fn said(cx: &Context, prefix: &str, body: &Arc<Body>, name: &str) -> Option<FaceKeys> {
        let (prefix, request) = (Id::new(prefix), face(cx, body, name));
        contract::check(cx, &prefix, &request).unwrap();
        match cx.call::<Keys>(&prefix, &request) {
            Attempt::Done(found) => Some(found),
            Attempt::Declined(_) => None,
            Attempt::Failed(error) => panic!("{error}"),
        }
    }

    /// The keys alone, for a family that relates nothing.
    pub(crate) fn keys(cx: &Context, prefix: &str, body: &Arc<Body>, name: &str) -> Vec<String> {
        said(cx, prefix, body, name).map(|found| found.keys).unwrap_or_default()
    }
}
