//! Real solver stops, driven by an injected clock rather than wall-clock sleeps.

use super::{edges, layout, read_mesh, triangulate, Abort, BrepMesh, Work};
use crate::tool::testing::{context, made};
use krly3d_core::{Attempt, CallControl, CallStop, Clock, Error, FakeClock, Id};
use krly3d_kernel::mesh::Mesh;
use krly3d_kernel::ports::interpret::{mesh_of, MeshDeclineStats, MeshRequest, ToMesh};
use krly3d_kernel::ports::shape::Leaf;
use krly3d_kernel::schema::Params;
use krly3d_kernel::{Context, KernelBuilder, Plugin};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};

struct GateClock {
    time: FakeClock,
    armed: AtomicBool,
    checks: AtomicUsize,
    pause_at: usize,
    entered: mpsc::SyncSender<()>,
    release: Barrier,
}

impl GateClock {
    fn new(pause_at: usize) -> (Arc<Self>, mpsc::Receiver<()>) {
        let (entered, reached) = mpsc::sync_channel(1);
        (
            Arc::new(Self {
                time: FakeClock::default(),
                armed: AtomicBool::new(false),
                checks: AtomicUsize::new(0),
                pause_at,
                entered,
                release: Barrier::new(2),
            }),
            reached,
        )
    }

    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
}

impl Clock for GateClock {
    fn now(&self) -> Duration {
        if self.armed.load(Ordering::SeqCst)
            && self.checks.fetch_add(1, Ordering::SeqCst) + 1 == self.pause_at
        {
            self.entered.send(()).unwrap();
            self.release.wait();
        }
        self.time.now()
    }
}

struct RealMesh {
    calls: Arc<AtomicUsize>,
    clock: Arc<GateClock>,
}

impl Plugin<ToMesh> for RealMesh {
    fn id(&self) -> Id {
        Id::new("brep")
    }

    fn call(&self, cx: &Context, request: &MeshRequest) -> Attempt<Arc<Mesh>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        read_mesh(cx, request, |work, flat, surface, points, tolerance| {
            // Everything before this is the real edge/layout code. Arm only when
            // entering the real triangulator, well away from any publication lock.
            self.clock.arm();
            triangulate::triangles(work, flat, surface, points, tolerance)
        })
    }
}

#[test]
fn real_mid_triangulation_stops_are_typed_and_retry_without_poisoned_caches() {
    for stop in [CallStop::Cancelled, CallStop::DeadlineExceeded] {
        let geometry = context();
        let body = made(&geometry, "sphere", Params::new().with("r", 5.0), &[]);
        let original = body.as_ref().clone();
        let (clock, reached) = GateClock::new(20);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = KernelBuilder::new();
        builder.plugin::<ToMesh>(RealMesh { calls: Arc::clone(&calls), clock: Arc::clone(&clock) });
        let base = builder.context();
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
        let controlled = base.with_call_control(Arc::clone(&control));
        let shape = Leaf::of(Arc::clone(&body));
        let running = {
            let shape = Arc::clone(&shape);
            std::thread::spawn(move || mesh_of(&controlled, &shape, 0.01))
        };
        reached.recv_timeout(Duration::from_secs(10)).unwrap();
        let requested = Instant::now();
        match stop {
            CallStop::Cancelled => control.cancel(),
            CallStop::DeadlineExceeded => clock.time.advance(Duration::from_secs(1)),
        }
        clock.release.wait();
        assert_eq!(running.join().unwrap(), Attempt::Failed(Error::from(stop)));
        eprintln!(
            "real triangulation {stop}: request-to-join {:?} (includes scheduling and cleanup)",
            requested.elapsed()
        );
        // Cancel after the flag read can pass the paused check, then is caught at
        // the next Work::CHUNK boundary, before another clock read. Deadline is
        // caught by the paused check itself. Neither path runs a later clock read.
        assert_eq!(clock.checks.load(Ordering::SeqCst), 20);
        assert_eq!(body.vertices, original.vertices);
        assert_eq!(body.edges.len(), original.edges.len());
        assert_eq!(body.faces.len(), original.faces.len());
        assert_eq!(
            BrepMesh::decline_stats(&base, &body),
            Attempt::Done(MeshDeclineStats { hits: 0, misses: 1, entries: 0 })
        );
        // A new per-call control shares the same caches, with an independent stop.
        let retry = base.with_call_control(Arc::new(CallControl::new()));
        let mesh = mesh_of(&retry, &shape, 0.01).into_result().unwrap();
        let again = mesh_of(&base, &shape, 0.01).into_result().unwrap();
        assert!(Arc::ptr_eq(&mesh, &again));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(mesh.open_sides().is_empty());
        assert_eq!(
            BrepMesh::decline_stats(&base, &body),
            Attempt::Done(MeshDeclineStats { hits: 0, misses: 2, entries: 0 })
        );
    }
}

#[test]
fn edge_recursion_and_layout_inner_loops_observe_mid_work_cancellation() {
    let base = context();
    let body = made(&base, "cylinder", Params::new().with("r", 5.0).with("h", 8.0), &[]);
    let tolerance = 0.0001;
    let mut sampled_points = body.vertices.clone();
    let sampled = edges::sample(&mut Work::new(&base), &body, tolerance, &mut sampled_points).unwrap();
    let face = body.faces.iter().position(|face| face.surface.classes().contains(&"cylinder")).unwrap();
    for in_layout in [false, true] {
        let (clock, reached) = GateClock::new(4);
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
        let controlled = base.with_call_control(Arc::clone(&control));
        let (body, sampled, mut points) = (Arc::clone(&body), sampled.clone(), sampled_points.clone());
        clock.arm();
        let running = std::thread::spawn(move || {
            let mut work = Work::new(&controlled);
            if in_layout {
                layout::lay_out(&mut work, &body, face, &sampled, &mut points, tolerance).map(|_| ())
            } else {
                points = body.vertices.clone();
                edges::sample(&mut work, &body, tolerance, &mut points).map(|_| ())
            }
        });
        reached.recv_timeout(Duration::from_secs(10)).unwrap();
        control.cancel();
        clock.release.wait();
        assert!(
            matches!(running.join().unwrap(), Err(Abort::Failed(error)) if error == Error::from(CallStop::Cancelled))
        );
        assert_eq!(clock.checks.load(Ordering::SeqCst), 4);
    }
}

#[test]
fn cancellation_before_budget_publication_does_not_memoize_the_decline() {
    struct Budget {
        control: Arc<CallControl>,
        calls: Arc<AtomicUsize>,
    }
    impl Plugin<ToMesh> for Budget {
        fn id(&self) -> Id {
            Id::new("brep")
        }
        fn call(&self, cx: &Context, request: &MeshRequest) -> Attempt<Arc<Mesh>> {
            read_mesh(cx, request, |_, _, _, _, _| {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    self.control.cancel();
                }
                Err(Abort::Budget("deterministic budget".into()))
            })
        }
    }
    let geometry = context();
    let body = made(&geometry, "box", Params::new().with("w", 2.0).with("d", 3.0).with("h", 4.0), &[]);
    let control = Arc::new(CallControl::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = KernelBuilder::new();
    builder.plugin::<ToMesh>(Budget { control: Arc::clone(&control), calls: Arc::clone(&calls) });
    let base = builder.context();
    let controlled = base.with_call_control(control);
    let shape = Leaf::of(Arc::clone(&body));
    assert_eq!(mesh_of(&controlled, &shape, 0.1), Attempt::Failed(Error::from(CallStop::Cancelled)));
    assert_eq!(
        BrepMesh::decline_stats(&base, &body),
        Attempt::Done(MeshDeclineStats { hits: 0, misses: 1, entries: 0 })
    );
    let retry = mesh_of(&base, &shape, 0.1);
    assert!(matches!(retry, Attempt::Declined(_)));
    assert_eq!(mesh_of(&base, &shape, 0.1), retry);
    let equal = made(&geometry, "box", Params::new().with("w", 2.0).with("d", 3.0).with("h", 4.0), &[]);
    assert!(!Arc::ptr_eq(&body, &equal));
    assert_eq!(mesh_of(&base, &Leaf::of(Arc::clone(&equal)), 0.1), retry);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        BrepMesh::decline_stats(&base, &body),
        Attempt::Done(MeshDeclineStats { hits: 2, misses: 2, entries: 1 })
    );
    assert_eq!(BrepMesh::decline_stats(&base, &equal), BrepMesh::decline_stats(&base, &body));
}
