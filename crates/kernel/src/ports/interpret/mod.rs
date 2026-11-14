//! Ports of the interpretations: what a shape is as a B-rep body, and as a mesh.
//!
//! Rules are keyed by shape class, most specific first, like `trim` and `content`: a rule
//! for `brep` reads a B-rep leaf, a rule for an operator reads that operator, and a rule for
//! `shape` stands at the end of the chain for whatever has no closed form, marked so.
//! These are the only places a shape is converted; a tool never converts by itself.

pub mod contract;

use crate::context::Context;
use crate::mesh::Mesh;
use crate::plugin::Family;
use crate::ports::pair;
use crate::ports::shape::{Leaf, Shape};
use crate::topo::Body;
use krly3d_core::Attempt;
use std::sync::{Arc, Mutex, PoisonError};

/// The B-rep reading of a shape.
pub struct ToBody;

impl Family for ToBody {
    const NAME: &'static str = "body reading";
    type Request = Arc<dyn Shape>;
    type Response = Arc<Body>;
    type Context = Context;
}

/// The mesh reading of a shape: triangles within `tolerance` of it.
pub struct ToMesh;

impl Family for ToMesh {
    const NAME: &'static str = "mesh reading";
    type Request = MeshRequest;
    type Response = Arc<Mesh>;
    type Context = Context;
}

pub struct MeshRequest {
    pub shape: Arc<dyn Shape>,
    /// How far a triangle may lie from the shape.
    pub tolerance: f64,
}

/// The shape as a B-rep body.
pub fn body_of(cx: &Context, shape: &Arc<dyn Shape>) -> Attempt<Arc<Body>> {
    pair::ask_class::<ToBody>(cx, shape.classes(), shape)
}

/// The shape as a mesh within `tolerance`. The mesh of a B-rep leaf is remembered with its
/// body, per tolerance, so that comparing a model again after an edit meshes only the
/// nodes the edit changed.
pub fn mesh_of(cx: &Context, shape: &Arc<dyn Shape>, tolerance: f64) -> Attempt<Arc<Mesh>> {
    let ask = || {
        pair::ask_class::<ToMesh>(cx, shape.classes(), &MeshRequest { shape: Arc::clone(shape), tolerance })
    };
    let Some(leaf) = shape.as_any().downcast_ref::<Leaf>() else { return ask() };
    let memo = match cx.derived::<Meshes>(&leaf.body, || Attempt::Done(Meshes::default())) {
        Attempt::Done(memo) => memo,
        Attempt::Declined(reason) => return Attempt::Declined(reason),
        Attempt::Failed(error) => return Attempt::Failed(error),
    };
    if let Some(known) = memo.get(tolerance) {
        return Attempt::Done(known);
    }
    ask().map(|mesh| {
        memo.put(tolerance, Arc::clone(&mesh));
        mesh
    })
}

/// The meshes made of one body, by the bits of their tolerance.
#[derive(Default)]
struct Meshes {
    made: Mutex<Vec<(u64, Arc<Mesh>)>>,
}

impl Meshes {
    fn get(&self, tolerance: f64) -> Option<Arc<Mesh>> {
        let made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        made.iter().find(|(bits, _)| *bits == tolerance.to_bits()).map(|(_, mesh)| Arc::clone(mesh))
    }

    fn put(&self, tolerance: f64, mesh: Arc<Mesh>) {
        self.made.lock().unwrap_or_else(PoisonError::into_inner).push((tolerance.to_bits(), mesh));
    }
}
