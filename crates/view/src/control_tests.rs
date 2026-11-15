//! Deterministic stops in real render work; no wall-clock sleeps or large workloads.

#![allow(clippy::unwrap_used, clippy::panic)]

use crate::ports::render::{render, Frame, Image, Render, RenderRequest, View};
use crate::raycast::NAME;
use krly3d_core::math::Vec3;
use krly3d_core::{Attempt, CallControl, CallStop, Clock, Error, FakeClock, Id};
use krly3d_kernel::ports::probe::{Hit, Probe, ProbeRequest, RAYS};
use krly3d_kernel::ports::query::{ask, Query, QueryRequest};
use krly3d_kernel::schema::Params;
use krly3d_kernel::{Context, KernelBuilder, Plugin};
use krly3d_std::probe::{box_tree::BoxTree, exhaustive::Exhaustive};
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

struct GateClock {
    time: FakeClock,
    checks: AtomicUsize,
    pause_at: AtomicUsize,
    expire_at: AtomicUsize,
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl GateClock {
    fn new() -> (Arc<Self>, mpsc::Receiver<()>, mpsc::SyncSender<()>) {
        let (entered, reached) = mpsc::sync_channel(1);
        let (resume, release) = mpsc::sync_channel(1);
        let clock = Arc::new(Self {
            time: FakeClock::default(),
            checks: AtomicUsize::new(0),
            pause_at: AtomicUsize::new(usize::MAX),
            expire_at: AtomicUsize::new(usize::MAX),
            entered,
            release: Mutex::new(release),
        });
        (clock, reached, resume)
    }

    fn reset(&self, pause_at: usize) {
        self.checks.store(0, Ordering::SeqCst);
        self.pause_at.store(pause_at, Ordering::SeqCst);
    }

    fn control(self: &Arc<Self>) -> Arc<CallControl> {
        Arc::new(CallControl::with_deadline(self.clone(), Duration::from_secs(1)))
    }
}

impl Clock for GateClock {
    fn now(&self) -> Duration {
        let check = self.checks.fetch_add(1, Ordering::SeqCst) + 1;
        if check == self.pause_at.load(Ordering::SeqCst) {
            self.entered.send(()).unwrap();
            // A broken test fails instead of leaving a blocked worker behind.
            self.release.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
        }
        if check >= self.expire_at.load(Ordering::SeqCst) {
            Duration::from_secs(1)
        } else {
            self.time.now()
        }
    }
}

#[derive(Clone, Copy)]
enum Method {
    Tree,
    Exhaustive,
}

#[derive(Default)]
struct ProbeStats {
    calls: AtomicUsize,
    first_check: AtomicUsize,
    largest_batch: AtomicUsize,
}

impl ProbeStats {
    fn reset(&self) {
        self.calls.store(0, Ordering::SeqCst);
        self.largest_batch.store(0, Ordering::SeqCst);
    }
}

struct RealProbe {
    method: Method,
    clock: Arc<GateClock>,
    stats: Arc<ProbeStats>,
}

impl Plugin<Probe> for RealProbe {
    fn id(&self) -> Id {
        Id::new(RAYS)
    }

    fn call(&self, cx: &Context, request: &ProbeRequest) -> Attempt<Vec<Vec<Hit>>> {
        if self.stats.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.stats.first_check.store(self.clock.checks.load(Ordering::SeqCst), Ordering::SeqCst);
        }
        self.stats.largest_batch.fetch_max(request.rays.len(), Ordering::SeqCst);
        match self.method {
            Method::Tree => BoxTree.call(cx, request),
            Method::Exhaustive => Exhaustive.call(cx, request),
        }
    }
}

fn pool(threads: usize) -> ThreadPool {
    ThreadPoolBuilder::new().num_threads(threads).build().unwrap()
}

fn scene(method: Method, clock: &Arc<GateClock>, stats: &Arc<ProbeStats>) -> (Context, RenderRequest) {
    let geometry = crate::testing::context();
    let body = krly3d_std::testing::made(
        &geometry,
        "box",
        Params::new().with("w", 60.0).with("d", 40.0).with("h", 10.0),
        &[],
    );
    let mut builder = KernelBuilder::new();
    builder.accuracy(*geometry.accuracy());
    krly3d_std::trim::register(&mut builder);
    builder.plugin::<Probe>(RealProbe { method, clock: Arc::clone(clock), stats: Arc::clone(stats) });
    crate::register(&mut builder);
    let request = RenderRequest {
        bodies: vec![body],
        view: View { toward: Vec3::new(-1.0, 1.0, -1.0), up: Vec3::z(), frame: Frame::Fit },
        size: (192, 48),
        paint: Vec::new(),
    };
    (builder.context(), request)
}

#[test]
fn real_probe_and_paint_stops_are_typed_and_retry_to_identical_images() {
    for method in [Method::Tree, Method::Exhaustive] {
        for in_paint in [false, true] {
            for stop in [CallStop::Cancelled, CallStop::DeadlineExceeded] {
                let (clock, reached, resume) = GateClock::new();
                let stats = Arc::new(ProbeStats::default());
                let (base, request) = scene(method, &clock, &stats);
                let pool = pool(1);
                // Warm valid shared data, then measure checkpoint positions on this
                // exact workload. The second run pauses in work, never in publication.
                let original = pool.install(|| render(&base, NAME, &request).into_result().unwrap());
                clock.reset(usize::MAX);
                stats.reset();
                let reference = base.with_call_control(clock.control());
                let image = pool.install(|| render(&reference, NAME, &request).into_result().unwrap());
                assert_eq!(image, original);
                let total = clock.checks.load(Ordering::SeqCst);
                // Painting publishes once, and Raycast guards its final attempt once.
                // Immediately before those two publications is its last pixel batch.
                let pause = if in_paint { total - 2 } else { stats.first_check.load(Ordering::SeqCst) + 32 };
                assert!(pause < total - 1);
                assert!(stats.largest_batch.load(Ordering::SeqCst) <= 64);
                clock.reset(pause);
                stats.reset();
                let control = clock.control();
                let controlled = base.with_call_control(Arc::clone(&control));
                let result = std::thread::scope(|scope| {
                    let running = scope.spawn(|| pool.install(|| render(&controlled, NAME, &request)));
                    let entered = reached.recv_timeout(Duration::from_secs(10));
                    if entered.is_ok() {
                        match stop {
                            CallStop::Cancelled => control.cancel(),
                            CallStop::DeadlineExceeded => clock.time.advance(Duration::from_secs(1)),
                        }
                    }
                    resume.send(()).unwrap();
                    let result = running.join().unwrap();
                    entered.unwrap();
                    result
                });
                assert_eq!(result, Attempt::Failed(Error::from(stop)));
                // Cancellation can pass the paused clock read, then the next check
                // sees its flag. An expired deadline is rechecked once by the final
                // attempt guard; no further render work is performed.
                let final_checks = usize::from(stop == CallStop::DeadlineExceeded);
                assert_eq!(clock.checks.load(Ordering::SeqCst), pause + final_checks);
                assert!(stats.calls.load(Ordering::SeqCst) > 0);
                let retry = base.with_call_control(Arc::new(CallControl::new()));
                let image = pool.install(|| render(&retry, NAME, &request).into_result().unwrap());
                assert_eq!(image, original, "stopping must not poison shared derived data");
            }
        }
    }
}

#[test]
fn parallel_result_collection_keeps_rows_and_normal_pixels_unchanged() {
    let (clock, _, _) = GateClock::new();
    let stats = Arc::new(ProbeStats::default());
    let (base, request) = scene(Method::Tree, &clock, &stats);
    let one = pool(1).install(|| render(&base, NAME, &request).into_result().unwrap());
    let controlled = base.with_call_control(Arc::new(CallControl::new()));
    let many = pool(4).install(|| render(&controlled, NAME, &request).into_result().unwrap());
    assert_eq!(many, one);
    assert!(one.seen.iter().flatten().count() > 0);
    assert!(one.seen.iter().any(Option::is_none));
}

#[test]
fn final_renderer_publication_rechecks_the_deadline() {
    let (clock, _, _) = GateClock::new();
    let stats = Arc::new(ProbeStats::default());
    let (base, request) = scene(Method::Tree, &clock, &stats);
    let pool = pool(1);
    pool.install(|| render(&base, NAME, &request).into_result().unwrap());
    let controlled = base.with_call_control(clock.control());
    pool.install(|| render(&controlled, NAME, &request).into_result().unwrap());
    let total = clock.checks.load(Ordering::SeqCst);
    clock.reset(usize::MAX);
    clock.expire_at.store(total, Ordering::SeqCst);
    let stopped = pool.install(|| render(&controlled, NAME, &request));
    assert_eq!(stopped, Attempt::Failed(Error::from(CallStop::DeadlineExceeded)));
}

#[test]
fn render_query_stops_inside_png_encoding_after_real_raycasting() {
    for stop in [CallStop::Cancelled, CallStop::DeadlineExceeded] {
        let session = crate::testing::session();
        let base = session.kernel().context();
        let (document, model) = session.kernel().snapshot();
        let id = Id::new("render");
        let params = Params::new().with("width", 96_usize).with("height", 72_usize);
        let admitted = base.registry::<Query>().unwrap().schema(&id).unwrap().admit(&id, &params, 0).unwrap();
        let query =
            QueryRequest { params: admitted, document: Arc::clone(&document), model: Arc::clone(&model) };
        let (_, bodies) = crate::nodes(&query).unwrap();
        let (_, view) = crate::view(base, &query.params).unwrap();
        let request = RenderRequest { bodies, view, size: (96, 72), paint: Vec::new() };
        let pool = pool(1);
        let expected = pool.install(|| ask(base, &id, &params, &document, &model).into_result().unwrap());
        let (clock, reached, resume) = GateClock::new();
        let controlled = base.with_call_control(clock.control());
        pool.install(|| render(&controlled, NAME, &request).into_result().unwrap());
        let render_checks = clock.checks.load(Ordering::SeqCst);
        // Query dispatch, Shot entry and paint admission add three checks before
        // rendering; pictured/PNG entry and the first row add four after it.
        // Pause at the next row, after PNG processing has actually copied pixels.
        clock.reset(render_checks + 8);
        let control = clock.control();
        let controlled = base.with_call_control(Arc::clone(&control));
        let result = std::thread::scope(|scope| {
            let running = scope.spawn(|| pool.install(|| ask(&controlled, &id, &params, &document, &model)));
            let entered = reached.recv_timeout(Duration::from_secs(10));
            if entered.is_ok() {
                match stop {
                    CallStop::Cancelled => control.cancel(),
                    CallStop::DeadlineExceeded => clock.time.advance(Duration::from_secs(1)),
                }
            }
            resume.send(()).unwrap();
            let result = running.join().unwrap();
            entered.unwrap();
            result
        });
        assert_eq!(result, Attempt::Failed(Error::from(stop)));
        // Picture and the query port both guard their final attempt.
        let final_checks = 2 * usize::from(stop == CallStop::DeadlineExceeded);
        assert_eq!(clock.checks.load(Ordering::SeqCst), render_checks + 8 + final_checks);
        let retry = base.with_call_control(Arc::new(CallControl::new()));
        let answer = pool.install(|| ask(&retry, &id, &params, &document, &model).into_result().unwrap());
        assert_eq!(answer, expected);
    }
}

#[test]
fn already_stopped_render_refuses_even_an_enormous_allocation() {
    let (clock, _, _) = GateClock::new();
    let (base, mut request) = scene(Method::Tree, &clock, &Arc::new(ProbeStats::default()));
    request.size = (usize::MAX, usize::MAX);
    let control = Arc::new(CallControl::new());
    control.cancel();
    let controlled = base.with_call_control(control);
    // Call the plugin directly to exercise its own guard before allocation/validation.
    let stopped: Attempt<Image> = crate::raycast::Raycast.call(&controlled, &request);
    assert_eq!(stopped, Attempt::Failed(Error::from(CallStop::Cancelled)));
}

#[test]
fn cancellation_wins_over_a_probe_decline_without_renderer_fallback() {
    struct Stops(Arc<CallControl>);
    impl Plugin<Probe> for Stops {
        fn id(&self) -> Id {
            Id::new(RAYS)
        }
        fn call(&self, _: &Context, _: &ProbeRequest) -> Attempt<Vec<Vec<Hit>>> {
            self.0.cancel();
            Attempt::declined("a probe that cannot answer while the call is stopped")
        }
    }
    struct Fallback(Arc<AtomicUsize>);
    impl Plugin<Render> for Fallback {
        fn id(&self) -> Id {
            Id::new(NAME)
        }
        fn call(&self, _: &Context, _: &RenderRequest) -> Attempt<Image> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Attempt::declined("unexpected renderer fallback")
        }
    }
    let (clock, _, _) = GateClock::new();
    let (_, request) = scene(Method::Tree, &clock, &Arc::new(ProbeStats::default()));
    let control = Arc::new(CallControl::new());
    let fallbacks = Arc::new(AtomicUsize::new(0));
    let mut builder = KernelBuilder::new();
    krly3d_std::trim::register(&mut builder);
    builder.plugin::<Probe>(Stops(Arc::clone(&control)));
    crate::register(&mut builder);
    builder.plugin::<Render>(Fallback(Arc::clone(&fallbacks)));
    let controlled = builder.context().with_call_control(control);
    let result = pool(1).install(|| render(&controlled, NAME, &request));
    assert_eq!(result, Attempt::Failed(Error::from(CallStop::Cancelled)));
    assert_eq!(fallbacks.load(Ordering::SeqCst), 0);
}
