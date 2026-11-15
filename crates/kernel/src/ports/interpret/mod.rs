//! Ports of the interpretations: what a shape is as a B-rep body, and as a mesh.
//!
//! Rules are keyed by shape class, most specific first, like `trim` and `content`: a rule
//! for `brep` reads a B-rep leaf, a rule for an operator reads that operator, and a rule for
//! `shape` stands at the end of the chain for whatever has no closed form, marked so.
//! These are the only places a shape is converted; a tool never converts by itself.

pub mod contract;
mod decline;

pub use decline::{MeshDeclineStats, MeshDeclines};

use crate::context::Context;
use crate::mesh::Mesh;
use crate::plugin::Family;
use crate::ports::pair;
use crate::ports::shape::{Leaf, Shape};
use crate::topo::Body;
use krly3d_core::Attempt;
use std::collections::VecDeque;
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
///
/// Each body in each context retains at most eight successful tolerance entries, keyed
/// by exact f64 bits, in FIFO insertion order. Hits do not refresh that order. Eviction
/// releases only the cache's Arc; existing readers keep their mesh. This is an entry
/// bound per body, not a byte budget or a total bound across bodies and readers.
pub fn mesh_of(cx: &Context, shape: &Arc<dyn Shape>, tolerance: f64) -> Attempt<Arc<Mesh>> {
    if let Err(error) = cx.check_control() {
        return Attempt::Failed(error);
    }
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
    match ask() {
        Attempt::Done(mesh) => match cx.publish_control(|| memo.put(tolerance, mesh)) {
            Ok((known, retired)) => {
                // Release evicted or duplicate candidates outside publication and cache locks.
                drop(retired);
                Attempt::Done(known)
            }
            Err(error) => Attempt::Failed(error),
        },
        Attempt::Declined(reason) => Attempt::Declined(reason),
        Attempt::Failed(error) => Attempt::Failed(error),
    }
}

/// The meshes made of one body, by the bits of their tolerance.
#[derive(Default)]
struct Meshes {
    made: Mutex<VecDeque<(u64, Arc<Mesh>)>>,
}

impl Meshes {
    const MAX_ENTRIES: usize = 8;

    fn get(&self, tolerance: f64) -> Option<Arc<Mesh>> {
        let made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        made.iter().find(|(bits, _)| *bits == tolerance.to_bits()).map(|(_, mesh)| Arc::clone(mesh))
    }

    fn put(&self, tolerance: f64, mesh: Arc<Mesh>) -> (Arc<Mesh>, Option<Arc<Mesh>>) {
        let mut made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        let key = tolerance.to_bits();
        // Concurrent misses may compute the same tolerance. Keep the resident winner
        // without consuming another slot or advancing its FIFO insertion order.
        if let Some((_, known)) = made.iter().find(|(bits, _)| *bits == key) {
            return (Arc::clone(known), Some(mesh));
        }
        let retired =
            if made.len() == Self::MAX_ENTRIES { made.pop_front().map(|(_, mesh)| mesh) } else { None };
        made.push_back((key, Arc::clone(&mesh)));
        (mesh, retired)
    }
}

#[cfg(test)]
mod tests {
    use super::{mesh_of, MeshRequest, Meshes, ToMesh};
    use crate::mesh::Mesh;
    use crate::ports::shape::Leaf;
    use crate::topo::Body;
    use crate::{Context, KernelBuilder, Plugin};
    use krly3d_core::{Attempt, CallControl, CallStop, Error, Id};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    struct Counting {
        calls: Arc<AtomicUsize>,
        gate: Option<(f64, Arc<Barrier>)>,
    }

    impl Plugin<ToMesh> for Counting {
        fn id(&self) -> Id {
            Id::new("brep")
        }

        fn call(&self, _: &Context, request: &MeshRequest) -> Attempt<Arc<Mesh>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some((tolerance, barrier)) = &self.gate {
                if tolerance.to_bits() == request.tolerance.to_bits() {
                    barrier.wait();
                }
            }
            Attempt::Done(Arc::new(Mesh::default()))
        }
    }

    fn context(gate: Option<(f64, Arc<Barrier>)>) -> (Context, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = KernelBuilder::new();
        builder.plugin::<ToMesh>(Counting { calls: Arc::clone(&calls), gate });
        (builder.context(), calls)
    }

    #[test]
    fn success_cache_preserves_exact_tolerance_body_and_context_identity() {
        let (cx, calls) = context(None);
        let body = Arc::new(Body::default());
        let shape = Leaf::of(Arc::clone(&body));
        let first = mesh_of(&cx, &shape, 0.05).into_result().unwrap();
        let shared = mesh_of(&cx.clone(), &Leaf::of(Arc::clone(&body)), 0.05).into_result().unwrap();
        assert!(Arc::ptr_eq(&first, &shared));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let adjacent = mesh_of(&cx, &shape, f64::from_bits(0.05_f64.to_bits() + 1)).into_result().unwrap();
        assert!(!Arc::ptr_eq(&first, &adjacent));
        let equal_body = Leaf::of(Arc::new(body.as_ref().clone()));
        let independent = mesh_of(&cx, &equal_body, 0.05).into_result().unwrap();
        assert!(!Arc::ptr_eq(&first, &independent));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        let (other_context, other_calls) = context(None);
        let other = mesh_of(&other_context, &shape, 0.05).into_result().unwrap();
        assert!(!Arc::ptr_eq(&first, &other));
        assert_eq!(other_calls.load(Ordering::SeqCst), 1);

        let control = Arc::new(CallControl::new());
        let controlled = cx.with_call_control(Arc::clone(&control));
        control.cancel();
        assert_eq!(mesh_of(&controlled, &shape, 0.05), Attempt::Failed(Error::from(CallStop::Cancelled)));
        assert!(Arc::ptr_eq(&first, &mesh_of(&cx, &shape, 0.05).into_result().unwrap()));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn success_cache_evicts_fifo_without_invalidating_existing_readers() {
        let (cx, calls) = context(None);
        let shape = Leaf::of(Arc::new(Body::default()));
        let reader = mesh_of(&cx, &shape, 1.0).into_result().unwrap();
        let weak = Arc::downgrade(&reader);
        for tolerance in 2..=Meshes::MAX_ENTRIES {
            drop(mesh_of(&cx, &shape, tolerance as f64).into_result().unwrap());
        }
        assert!(Arc::ptr_eq(&reader, &mesh_of(&cx, &shape, 1.0).into_result().unwrap()));
        assert_eq!(calls.load(Ordering::SeqCst), Meshes::MAX_ENTRIES);
        drop(mesh_of(&cx, &shape, 9.0).into_result().unwrap());
        assert_eq!(Arc::strong_count(&reader), 1, "a hit must not refresh FIFO order");
        let rebuilt = mesh_of(&cx, &shape, 1.0).into_result().unwrap();
        assert!(!Arc::ptr_eq(&reader, &rebuilt));
        assert_eq!(*reader, *rebuilt);
        assert_eq!(calls.load(Ordering::SeqCst), Meshes::MAX_ENTRIES + 2);
        drop(reader);
        assert!(weak.upgrade().is_none(), "evicted meshes die when their last reader releases them");
    }

    #[test]
    fn each_body_keeps_its_own_eight_successes_in_one_context() {
        let (cx, calls) = context(None);
        let shapes = [Leaf::of(Arc::new(Body::default())), Leaf::of(Arc::new(Body::default()))];
        let residents: Vec<Vec<_>> = shapes
            .iter()
            .map(|shape| {
                (1..=Meshes::MAX_ENTRIES)
                    .map(|tolerance| {
                        Arc::downgrade(&mesh_of(&cx, shape, tolerance as f64).into_result().unwrap())
                    })
                    .collect()
            })
            .collect();
        assert!(residents.iter().flatten().all(|mesh| mesh.upgrade().is_some()));
        assert_eq!(calls.load(Ordering::SeqCst), 2 * Meshes::MAX_ENTRIES);
        drop(mesh_of(&cx, &shapes[0], 9.0).into_result().unwrap());
        assert!(residents[0][0].upgrade().is_none());
        assert!(residents[0].iter().skip(1).all(|mesh| mesh.upgrade().is_some()));
        assert!(residents[1].iter().all(|mesh| mesh.upgrade().is_some()));
    }

    #[test]
    fn concurrent_successes_share_one_tolerance_slot_without_evicting_other_entries() {
        let workers = 4;
        let (cx, calls) = context(Some((0.5, Arc::new(Barrier::new(workers)))));
        let shape = Leaf::of(Arc::new(Body::default()));
        let residents: Vec<_> = (1..Meshes::MAX_ENTRIES)
            .map(|tolerance| Arc::downgrade(&mesh_of(&cx, &shape, tolerance as f64).into_result().unwrap()))
            .collect();
        let threads: Vec<_> = (0..workers)
            .map(|_| {
                let (cx, shape) = (cx.clone(), Arc::clone(&shape));
                std::thread::spawn(move || mesh_of(&cx, &shape, 0.5).into_result().unwrap())
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|thread| thread.join().unwrap()).collect();
        assert!(results.iter().all(|mesh| Arc::ptr_eq(mesh, &results[0])));
        assert!(residents.iter().all(|mesh| mesh.upgrade().is_some()));
        assert!(Arc::ptr_eq(&results[0], &mesh_of(&cx, &shape, 0.5).into_result().unwrap()));
        assert_eq!(calls.load(Ordering::SeqCst), Meshes::MAX_ENTRIES - 1 + workers);
        drop(mesh_of(&cx, &shape, 9.0).into_result().unwrap());
        assert!(residents[0].upgrade().is_none());
        assert!(residents.iter().skip(1).all(|mesh| mesh.upgrade().is_some()));
    }

    #[test]
    fn cancelled_success_cannot_evict_a_resident_mesh() {
        struct CancelSuccess(Arc<CallControl>);

        impl Plugin<ToMesh> for CancelSuccess {
            fn id(&self) -> Id {
                Id::new("brep")
            }

            fn call(&self, _: &Context, request: &MeshRequest) -> Attempt<Arc<Mesh>> {
                if request.tolerance == 9.0 {
                    self.0.cancel();
                }
                Attempt::Done(Arc::new(Mesh::default()))
            }
        }

        let control = Arc::new(CallControl::new());
        let mut builder = KernelBuilder::new();
        builder.plugin::<ToMesh>(CancelSuccess(Arc::clone(&control)));
        let cx = builder.context();
        let shape = Leaf::of(Arc::new(Body::default()));
        let residents: Vec<_> = (1..=Meshes::MAX_ENTRIES)
            .map(|tolerance| Arc::downgrade(&mesh_of(&cx, &shape, tolerance as f64).into_result().unwrap()))
            .collect();
        let controlled = cx.with_call_control(control);
        assert_eq!(mesh_of(&controlled, &shape, 9.0), Attempt::Failed(Error::from(CallStop::Cancelled)));
        assert!(residents.iter().all(|mesh| mesh.upgrade().is_some()));
        drop(mesh_of(&cx, &shape, 9.0).into_result().unwrap());
        assert!(residents[0].upgrade().is_none(), "retry must compute and publish the ninth tolerance");
    }
}
