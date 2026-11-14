//! Port of the `shape` family: the foundation every reading of a model starts from.
//!
//! An object family: a shape answers how far a point is from it, with a sign (negative
//! inside), and how far it reaches. A B-rep body is one kind of shape ([`Leaf`]); fields,
//! subdivision cages and the operators that combine shapes are others, each a plugin of its
//! own. What a shape is *as* a body, a mesh or a field is asked of the `interpret` families,
//! once, generically; no tool makes its geometry twice.

pub mod contract;

use crate::context::Context;
use crate::ports::probe::{side, Side};
use crate::ports::trim::{place, Place, TrimRequest};
use crate::topo::{body_bounds, Body};
use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::Point3;
use krly3d_core::{Attempt, Exactness};
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// A distance from a shape: negative inside, positive outside, zero on the boundary; and
/// how far it can be trusted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Signed {
    pub distance: f64,
    pub exact: Exactness,
}

pub trait Shape: Debug + Send + Sync + 'static {
    /// Class names from the most specific to the most general; the first is the kind.
    /// Interpretation rules are looked up in this order.
    fn classes(&self) -> &'static [&'static str];

    /// The signed distance of `point` from the shape. It never exceeds the true distance:
    /// a field may answer less, never more, so that marching by it never steps across.
    fn distance(&self, cx: &Context, point: &Point3) -> Attempt<Signed>;

    /// A box the whole shape lies in.
    fn bounds(&self, cx: &Context) -> Attempt<Bounds>;

    /// The shapes an operator combines; none for a leaf.
    fn children(&self) -> &[Arc<dyn Shape>] {
        &[]
    }

    /// Gives interpretation rules access to the concrete type.
    fn as_any(&self) -> &dyn Any;

    /// The same, keeping the shared ownership, so that a rule can hand on what it holds.
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
}

/// A B-rep body as a shape: what every tool that builds a body makes.
#[derive(Clone, Debug)]
pub struct Leaf {
    pub body: Arc<Body>,
}

impl Leaf {
    pub fn of(body: Arc<Body>) -> Arc<dyn Shape> {
        Arc::new(Self { body })
    }
}

impl Shape for Leaf {
    fn classes(&self) -> &'static [&'static str] {
        &["brep", "shape"]
    }

    /// The nearest point of a face is the foot on its surface when the face holds that
    /// foot, and otherwise lies on its boundary; the sign comes from the probe.
    fn distance(&self, cx: &Context, point: &Point3) -> Attempt<Signed> {
        let body = &self.body;
        let mut nearest = f64::INFINITY;
        for (index, face) in body.faces.iter().enumerate() {
            let foot = face.surface.point(face.surface.project(point));
            let inside = match place(cx, &TrimRequest { body: Arc::clone(body), face: index, point: foot }) {
                Attempt::Done(found) => found != Place::Outside,
                Attempt::Declined(reason) => return Attempt::Declined(reason),
                Attempt::Failed(error) => return Attempt::Failed(error),
            };
            if inside {
                nearest = nearest.min((foot - point).norm());
                continue;
            }
            for used in face.uses() {
                let edge = &body.edges[used.edge];
                nearest = nearest.min((edge.curve.point(edge.nearest(point)) - point).norm());
            }
        }
        if !body.solid {
            return Attempt::Done(Signed { distance: nearest, exact: exactness(body, cx) });
        }
        let sign = match side(cx, body, point) {
            Attempt::Done(Side::Inside) => -1.0,
            Attempt::Done(Side::Outside) => 1.0,
            Attempt::Done(Side::Boundary) => 0.0,
            Attempt::Declined(reason) => return Attempt::Declined(reason),
            Attempt::Failed(error) => return Attempt::Failed(error),
        };
        Attempt::Done(Signed { distance: sign * nearest, exact: exactness(body, cx) })
    }

    fn bounds(&self, cx: &Context) -> Attempt<Bounds> {
        body_bounds(cx, &self.body)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

/// Feet on analytic surfaces and points on lines and circles are closed forms; on the
/// others they are found numerically.
fn exactness(body: &Body, cx: &Context) -> Exactness {
    const CLOSED: [&str; 5] = ["plane", "sphere", "cylinder", "cone", "torus"];
    let analytic = body.faces.iter().all(|face| CLOSED.contains(&face.surface.classes()[0]))
        && body.edges.iter().all(|edge| ["line", "circle"].contains(&edge.curve.classes()[0]));
    if analytic {
        Exactness::Exact
    } else {
        Exactness::Numeric { tolerance: cx.accuracy().length() }
    }
}
