//! The interpretations of shapes: one generic rule per reading and kind of shape.

pub mod mesh;

use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::interpret::{ToBody, ToMesh};
use krly3d_kernel::ports::shape::{Leaf, Shape};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, KernelBuilder, Plugin};
use std::sync::Arc;

pub fn register(builder: &mut KernelBuilder) {
    builder.plugin::<ToBody>(BrepBody).plugin::<ToMesh>(mesh::BrepMesh);
}

/// A B-rep leaf read as a body is that body.
pub struct BrepBody;

impl Plugin<ToBody> for BrepBody {
    fn id(&self) -> Id {
        Id::new("brep")
    }

    fn call(&self, _: &Context, shape: &Arc<dyn Shape>) -> Attempt<Arc<Body>> {
        match Arc::clone(shape).into_any().downcast::<Leaf>() {
            Ok(leaf) => Attempt::Done(Arc::clone(&leaf.body)),
            Err(_) => Attempt::declined("the brep body rule reads B-rep leaves"),
        }
    }
}
