//! Deterministic stops injected after real native provider operations, without sleeps.
#![allow(clippy::unwrap_used)]

use super::{
    build_planar_retained_controlled, build_region_in_context, build_region_retained,
    build_region_retained_controlled, publish_retained, RetainedPlanar,
};
use krly2d_api::Region;
use krly3d_brep::planar::{PlanarCall, PlanarInput, PROTOCOL};
use krly3d_brep::{Diagnostic, Id};
use krly3d_core::Attempt;
use krly3d_kernel::plugin::Family;
use krly3d_kernel::ports::meet::Meet;
use krly3d_kernel::ports::stage::{EditStage, NodeRun, NodeStage};
use krly3d_kernel::ports::sweep::Sweep;
use krly3d_kernel::ports::tool::Tool;
use krly3d_kernel::ports::trim::Trim;
use krly3d_kernel::schema::Schema;
use krly3d_kernel::{Context, KernelBuilder, Plugin};
use krlyxd::{CallControl, Clock, FakeClock, LengthUnit};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Weak};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Profile = 0,
    Sweep,
    Meet,
    Trim,
    Node,
    Commit,
}

enum Stop {
    Deadline(Arc<FakeClock>),
    Cancel(Weak<CallControl>),
}

struct Observation {
    phase: Phase,
    after: usize,
    stop: Stop,
    counts: [AtomicUsize; 6],
}

impl Observation {
    fn new(phase: Phase, after: usize, stop: Stop) -> Arc<Self> {
        Arc::new(Self { phase, after, stop, counts: std::array::from_fn(|_| AtomicUsize::new(0)) })
    }

    fn count(&self, phase: Phase) -> usize {
        self.counts[phase as usize].load(Ordering::Relaxed)
    }

    fn completed(&self, phase: Phase) {
        let count = self.counts[phase as usize].fetch_add(1, Ordering::Relaxed) + 1;
        if phase == self.phase && count == self.after {
            match &self.stop {
                Stop::Deadline(clock) => clock.advance(Duration::from_secs(1)),
                Stop::Cancel(control) => control.upgrade().unwrap().cancel(),
            }
        }
    }
}

struct Observed<P> {
    provider: P,
    phase: Phase,
    observation: Arc<Observation>,
}

impl<F, P> Plugin<F> for Observed<P>
where
    F: Family<Context = Context>,
    P: Plugin<F>,
{
    fn id(&self) -> Id {
        self.provider.id()
    }

    fn schema(&self) -> Option<Schema> {
        self.provider.schema()
    }

    fn call(&self, cx: &Context, request: &F::Request) -> Attempt<F::Response> {
        let result = self.provider.call(cx, request);
        if matches!(&result, Attempt::Done(_)) {
            self.observation.completed(self.phase);
        }
        result
    }
}

fn services(observation: &Arc<Observation>) -> Context {
    let mut builder = KernelBuilder::new();
    builder
        .plugin::<Tool>(Observed {
            provider: krly3d_std::tool::profile::Profile,
            phase: Phase::Profile,
            observation: Arc::clone(observation),
        })
        .plugin::<Sweep>(Observed {
            provider: krly3d_std::sweep::line_shift::LineShift,
            phase: Phase::Sweep,
            observation: Arc::clone(observation),
        })
        .plugin::<Meet>(Observed {
            provider: krly3d_std::meet::line_surface::LineSurface,
            phase: Phase::Meet,
            observation: Arc::clone(observation),
        })
        .plugin::<Trim>(Observed {
            provider: krly3d_std::trim::plane::PlaneTrim,
            phase: Phase::Trim,
            observation: Arc::clone(observation),
        })
        .plugin::<NodeStage>(Observed {
            provider: krly3d_std::stage::run::Run,
            phase: Phase::Node,
            observation: Arc::clone(observation),
        })
        .plugin::<EditStage>(Observed {
            provider: krly3d_std::stage::commit::Commit,
            phase: Phase::Commit,
            observation: Arc::clone(observation),
        });
    krly3d_std::tool::register(&mut builder);
    krly3d_std::sweep::register(&mut builder);
    krly3d_std::meet::register(&mut builder);
    krly3d_std::trim::register(&mut builder);
    krly3d_std::stage::register(&mut builder);
    krly3d_std::measure::register(&mut builder);
    builder.context()
}

fn polygons() -> Vec<Vec<[f64; 2]>> {
    vec![
        vec![[10., 20.], [20., 20.], [20., 28.], [10., 28.]],
        vec![[12., 22.], [14., 22.], [14., 24.], [12., 24.]],
        vec![[16., 24.], [19., 24.], [19., 27.], [16., 27.]],
    ]
}

fn region() -> Arc<Region> {
    Arc::new(Region::from_polygons(polygons(), LengthUnit::Model).unwrap())
}

fn call() -> PlanarCall {
    PlanarCall {
        protocol: PROTOCOL.into(),
        request_id: "controlled".into(),
        input: PlanarInput {
            polygons: polygons(),
            height: 3.,
            length_unit: "model".into(),
            source: "vector:unverified".into(),
        },
    }
}

fn build_in(
    services: Context,
    control: &Arc<CallControl>,
    region: Arc<Region>,
) -> Result<RetainedPlanar, Diagnostic> {
    build_region_in_context(region, 3., "controlled".into(), "vector:unverified".into(), control, services)
}

fn assert_stopped(result: Result<RetainedPlanar, Diagnostic>, code: &str) {
    assert!(result.is_err(), "stopped computation must not return a retained owner or snapshot");
    let error = result.err().unwrap();
    assert_eq!(error.code.as_str(), code);
}

#[test]
fn public_entry_points_reject_pre_cancelled_and_expired_calls_before_input_validation() {
    for cancelled in [false, true] {
        let clock = Arc::new(FakeClock::default());
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
        if cancelled {
            control.cancel();
        } else {
            clock.advance(Duration::from_secs(1));
        }
        let code = if cancelled { "cancelled" } else { "deadline_exceeded" };
        assert_stopped(build_planar_retained_controlled(call(), Arc::clone(&control)), code);
        let mut invalid = call();
        invalid.protocol = "invalid".into();
        assert_stopped(build_planar_retained_controlled(invalid, Arc::clone(&control)), code);
        assert_stopped(
            build_region_retained_controlled(
                region(),
                3.,
                "controlled".into(),
                "vector".into(),
                Arc::clone(&control),
            ),
            code,
        );
    }
}

#[test]
fn cancellation_and_fake_deadlines_stop_inside_each_topology_loop_without_a_result_and_allow_retry() {
    // The full input requires 3 profiles, 12 walls, 66 boundary pairs and
    // 4 containment queries. Each stop occurs strictly inside that work.
    for (phase, after) in [(Phase::Profile, 2), (Phase::Sweep, 2), (Phase::Meet, 7), (Phase::Trim, 3)] {
        for cancelled in [false, true] {
            let input = region();
            let clock = Arc::new(FakeClock::default());
            let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
            let stop = if cancelled { Stop::Cancel(Arc::downgrade(&control)) } else { Stop::Deadline(clock) };
            let observation = Observation::new(phase, after, stop);
            let services = services(&observation);
            let code = if cancelled { "cancelled" } else { "deadline_exceeded" };
            assert_stopped(build_in(services.clone(), &control, Arc::clone(&input)), code);
            assert_eq!(observation.count(phase), after, "{phase:?}: later iterations must not run");
            assert_eq!(observation.count(Phase::Node), 0, "topology stop must prevent document construction");
            assert_eq!(observation.count(Phase::Commit), 0);
            assert_eq!(Arc::strong_count(&input), 1, "failure must release its region, owner and snapshot");
            // Reuse the exact services/caches and input. The injected stop fires
            // only once; neither a cancelled context nor a failed result is kept.
            let retry = build_in(services, &Arc::new(CallControl::new()), Arc::clone(&input)).unwrap();
            assert!(Arc::ptr_eq(&retry.region, &input));
            assert!(retry.engine.owns(&retry.snapshot));
            assert_eq!(observation.count(Phase::Node), 2);
            assert!(retry.engine.kernel().body("part@1").unwrap().solid);
        }
    }
}

#[test]
fn cancellation_and_deadline_after_actual_profile_construction_prevent_extrusion() {
    for cancelled in [false, true] {
        let clock = Arc::new(FakeClock::default());
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
        let stop = if cancelled { Stop::Cancel(Arc::downgrade(&control)) } else { Stop::Deadline(clock) };
        let observation = Observation::new(Phase::Node, 1, stop);
        let services = services(&observation);
        assert_stopped(
            build_in(services.clone(), &control, region()),
            if cancelled { "cancelled" } else { "deadline_exceeded" },
        );
        assert_eq!(observation.count(Phase::Profile), 4, "3 validation profiles plus the actual profile");
        assert_eq!(observation.count(Phase::Node), 1, "the extrusion operation must not run");
        assert_eq!(observation.count(Phase::Commit), 0);
        let retry = build_in(services, &Arc::new(CallControl::new()), region()).unwrap();
        assert!(retry.engine.kernel().body("part@1").unwrap().solid);
        assert_eq!(observation.count(Phase::Node), 3);
    }
}

#[test]
fn nested_sweep_checkpoints_stop_native_extrusion_before_it_finishes() {
    for cancelled in [false, true] {
        let clock = Arc::new(FakeClock::default());
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
        let stop = if cancelled { Stop::Cancel(Arc::downgrade(&control)) } else { Stop::Deadline(clock) };
        // Twelve topology walls are complete; expire/cancel after the second
        // side sweep of the actual extrusion, which needs twelve more sweeps.
        let observation = Observation::new(Phase::Sweep, 14, stop);
        assert_stopped(
            build_in(services(&observation), &control, region()),
            if cancelled { "cancelled" } else { "deadline_exceeded" },
        );
        assert_eq!(observation.count(Phase::Sweep), 14);
        assert_eq!(observation.count(Phase::Profile), 4);
        assert_eq!(observation.count(Phase::Node), 1, "only the actual profile operation completed");
        assert_eq!(observation.count(Phase::Commit), 0);
    }
}

#[test]
fn a_stop_after_native_commit_still_rejects_the_provisional_result() {
    for cancelled in [false, true] {
        let clock = Arc::new(FakeClock::default());
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
        let stop = if cancelled { Stop::Cancel(Arc::downgrade(&control)) } else { Stop::Deadline(clock) };
        let observation = Observation::new(Phase::Commit, 1, stop);
        assert_stopped(
            build_in(services(&observation), &control, region()),
            if cancelled { "cancelled" } else { "deadline_exceeded" },
        );
        assert_eq!(observation.count(Phase::Node), 2);
        assert_eq!(observation.count(Phase::Commit), 1);
    }
}

/// Counts checkpoints without using elapsed wall time, expiring at a selected one.
/// Clock::now also runs under CallControl's final publication gate, so it must
/// never call cancel/publish. Advancing FakeClock is safe there.
#[derive(Default)]
struct CheckpointClock {
    clock: FakeClock,
    calls: AtomicUsize,
    expire_at: AtomicUsize,
}

impl Clock for CheckpointClock {
    fn now(&self) -> Duration {
        let count = self.calls.fetch_add(1, Ordering::Relaxed) + 1;
        if count == self.expire_at.load(Ordering::Relaxed) {
            self.clock.advance(Duration::from_secs(1));
        }
        self.clock.now()
    }
}

#[test]
fn deadline_at_the_final_publication_guard_returns_no_owner_or_snapshot() {
    // Calibrate with the same deterministic native work rather than depending
    // on a fragile hard-coded total checkpoint count.
    let clock = Arc::new(CheckpointClock::default());
    let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
    let input = region();
    let ready = build_region_retained_controlled(
        Arc::clone(&input),
        3.,
        "controlled".into(),
        "vector".into(),
        control,
    )
    .unwrap();
    let final_check = clock.calls.load(Ordering::Relaxed);
    drop(ready);
    let clock = Arc::new(CheckpointClock::default());
    clock.expire_at.store(final_check, Ordering::Relaxed);
    let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
    assert_stopped(
        build_region_retained_controlled(
            Arc::clone(&input),
            3.,
            "controlled".into(),
            "vector".into(),
            control,
        ),
        "deadline_exceeded",
    );
    assert_eq!(clock.calls.load(Ordering::Relaxed), final_check);
    assert_eq!(Arc::strong_count(&input), 1);
    assert!(build_region_retained(input, 3., "retry".into(), "vector".into()).is_ok());
}

struct CancelOnDrop {
    control: Weak<CallControl>,
    dropped: Arc<AtomicUsize>,
}

impl Plugin<NodeStage> for CancelOnDrop {
    fn id(&self) -> Id {
        Id::new("unused-drop-observer")
    }

    fn call(&self, _: &Context, request: &NodeRun) -> Attempt<NodeRun> {
        Attempt::Done(request.clone())
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.control.upgrade().unwrap().cancel();
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn rejected_candidates_drop_callback_services_outside_the_publication_gate() {
    let (sent, received) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        for cancelled in [false, true] {
            let clock = Arc::new(FakeClock::default());
            let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
            let dropped = Arc::new(AtomicUsize::new(0));
            let mut builder = krly3d_std::standard();
            builder.plugin::<NodeStage>(CancelOnDrop {
                control: Arc::downgrade(&control),
                dropped: Arc::clone(&dropped),
            });
            let retained = build_in(builder.context(), &Arc::new(CallControl::new()), region()).unwrap();
            if cancelled {
                control.cancel();
            } else {
                clock.advance(Duration::from_secs(1));
            }
            assert_stopped(
                publish_retained(retained, &control),
                if cancelled { "cancelled" } else { "deadline_exceeded" },
            );
            assert_eq!(dropped.load(Ordering::Relaxed), 1, "rejected retained services must be released");
        }
        sent.send(()).unwrap();
    });
    // Only a deadlock watchdog; all work/stopping decisions above use FakeClock
    // or synchronous cancellation. A regression must fail instead of hanging CI.
    let finished = received.recv_timeout(Duration::from_secs(10));
    assert!(finished.is_ok(), "a destructor re-entered a held publication gate");
    worker.join().unwrap();
}
