//! Port of the `probe` family: what a ray hits.
//!
//! "What does a ray hit" is the first of the questions any representation of a shape
//! must answer for the perception layer; whatever answers it can stand behind the same
//! queries. Rays are asked in batches so that an implementation can share work between
//! them. Plugins register under the name of the question, `rays`: an accelerated probe
//! first, the reference one behind it.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::ports::trim::{place, Place, TrimRequest};
use crate::topo::{Body, FaceId};
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::{Attempt, Id};
use std::sync::Arc;

pub struct Probe;

impl Family for Probe {
    const NAME: &'static str = "probe";
    type Request = ProbeRequest;
    type Response = Vec<Vec<Hit>>;
    type Context = crate::context::Context;
}

/// The id every ray probe registers under.
pub const RAYS: &str = "rays";

pub struct ProbeRequest {
    pub body: Arc<Body>,
    pub rays: Vec<Ray>,
}

/// The line `origin + t·direction`; hits are reported for every `t`, behind the origin too.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Point3,
    pub direction: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub t: f64,
    pub face: FaceId,
    pub point: Point3,
    /// Whether the ray passes from outside the material to inside it here.
    pub entering: bool,
    /// Whether the hit lies on an edge of the face rather than within it.
    pub on_edge: bool,
}

/// The hits of each ray on the body, ordered along the ray. A ray that only touches the
/// body, along an edge or tangent to a face, does not hit it.
pub fn cast(cx: &Context, body: &Arc<Body>, rays: Vec<Ray>) -> Attempt<Vec<Vec<Hit>>> {
    cx.call::<Probe>(&Id::new(RAYS), &ProbeRequest { body: Arc::clone(body), rays })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Inside,
    Outside,
    /// On a face of the body, within the length tolerance.
    Boundary,
}

/// Directions with no two components equal, built from the golden ratio, so that a ray
/// along one of them is unlikely to run along an edge or through a corner of a model
/// drawn on a grid.
const DIRECTIONS: [[f64; 3]; 6] = {
    const A: f64 = 0.618_033_988_749_895;
    const B: f64 = A * A;
    [[1.0, A, B], [-A, 1.0, B], [B, -A, 1.0], [-1.0, B, -A], [A, -1.0, -B], [-B, -A, -1.0]]
};

/// On which side of the boundary of a solid a point lies.
///
/// A ray from the point decides: an odd number of hits ahead means inside, and so does a
/// first hit that leaves the material. Both must agree and no hit may lie on an edge;
/// otherwise the next direction is tried, and if none is clean the question is declined
/// rather than guessed.
pub fn side(cx: &Context, body: &Arc<Body>, point: &Point3) -> Attempt<Side> {
    for (face, data) in body.faces.iter().enumerate() {
        if cx.accuracy().is_zero_length(data.surface.distance(point)) {
            match place(cx, &TrimRequest { body: Arc::clone(body), face, point: *point }) {
                Attempt::Done(Place::Outside) => {}
                Attempt::Done(_) => return Attempt::Done(Side::Boundary),
                Attempt::Declined(reason) => return Attempt::Declined(reason),
                Attempt::Failed(error) => return Attempt::Failed(error),
            }
        }
    }
    for direction in DIRECTIONS {
        let ray = Ray { origin: *point, direction: Vec3::from(direction) };
        let hits = match cast(cx, body, vec![ray]) {
            Attempt::Done(mut per_ray) => per_ray.pop().unwrap_or_default(),
            Attempt::Declined(reason) => return Attempt::Declined(reason),
            Attempt::Failed(error) => return Attempt::Failed(error),
        };
        let ahead: Vec<&Hit> = hits.iter().filter(|hit| hit.t > 0.0).collect();
        let odd = ahead.len() % 2 == 1;
        let leaving_first = ahead.first().is_some_and(|hit| !hit.entering);
        if odd == leaving_first && !ahead.iter().any(|hit| hit.on_edge) {
            return Attempt::Done(if odd { Side::Inside } else { Side::Outside });
        }
    }
    Attempt::declined("every ray tried from the point passes through an edge")
}
