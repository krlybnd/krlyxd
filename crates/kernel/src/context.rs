//! What a plugin is given when it is called.

use crate::plugin::{Family, Registries, Registry};
use crate::topo::Body;
use krly3d_core::{Accuracy, Attempt, CallControl, Clock, Error, Id};
use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::{Mutex, PoisonError, Weak};

/// The injected dependencies of a call. Plugins hold no global state: whatever they
/// need beyond their request, including other plugins, comes from here.
/// Clones retain the same services, derived caches and optional cancellation control.
#[derive(Clone)]
pub struct Context {
    accuracy: Accuracy,
    clock: Arc<dyn Clock>,
    registries: Arc<Registries>,
    node_stages: Vec<Id>,
    edit_stages: Vec<Id>,
    derived: Arc<Derived>,
    control: Option<Arc<CallControl>>,
}

impl Context {
    pub fn new(accuracy: Accuracy, clock: Arc<dyn Clock>, registries: Registries) -> Self {
        Self {
            accuracy,
            clock,
            registries: Arc::new(registries),
            node_stages: Vec::new(),
            edit_stages: Vec::new(),
            derived: Arc::new(Derived::default()),
            control: None,
        }
    }

    /// A per-call view sharing the immutable plugin services and all derived caches.
    /// Attaching a control does not change the original context or another call's control.
    pub fn with_call_control(&self, control: Arc<CallControl>) -> Context {
        Self { control: Some(control), ..self.clone() }
    }

    /// Check a cooperative work boundary. Legacy contexts have no call control.
    pub fn check_control(&self) -> Result<(), Error> {
        self.control.as_ref().map_or(Ok(()), |control| control.check().map_err(Error::from))
    }

    /// Serialize a short cache/result publication with cancellation, checking the deadline.
    /// The closure must not re-enter publication or cancel this context's control.
    pub fn publish_control<T>(&self, publish: impl FnOnce() -> T) -> Result<T, Error> {
        match &self.control {
            Some(control) => control.publish(publish).map_err(Error::from),
            None => Ok(publish()),
        }
    }

    /// The same context with the two pipelines set: the stages that turn an operation
    /// into a body, and the stages an edit of the document passes through.
    pub fn with_stages(mut self, node_stages: Vec<Id>, edit_stages: Vec<Id>) -> Self {
        self.node_stages = node_stages;
        self.edit_stages = edit_stages;
        self
    }

    pub fn node_stages(&self) -> &[Id] {
        &self.node_stages
    }

    pub fn edit_stages(&self) -> &[Id] {
        &self.edit_stages
    }

    pub fn accuracy(&self) -> &Accuracy {
        &self.accuracy
    }

    pub fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }

    pub fn registry<F: Family>(&self) -> Option<&Registry<F>> {
        self.registries.get::<F>()
    }

    /// Asks the chain of family `F` registered under `id`.
    pub fn call<F: Family<Context = Self>>(&self, id: &Id, request: &F::Request) -> Attempt<F::Response> {
        if let Err(error) = self.check_control() {
            return Attempt::Failed(error);
        }
        match self.registry::<F>() {
            Some(registry) => registry.call(self, id, request),
            None => Attempt::declined(format!("no {} plugin is registered at all", F::NAME)),
        }
    }

    /// Something worked out from a body, such as a search index or a tree of boxes: made
    /// by `make` the first time it is asked for and remembered for as long as the body
    /// lives. A body is known by its `Arc`, which is all a probe is handed, and the model
    /// keeps one `Arc` per build key, so this is a memory per built node as well. One
    /// body may have derived data of several types; what fails to be made is not kept.
    pub fn derived<T: Send + Sync + 'static>(
        &self,
        body: &Arc<Body>,
        make: impl FnOnce() -> Attempt<T>,
    ) -> Attempt<Arc<T>> {
        if let Err(error) = self.check_control() {
            return Attempt::Failed(error);
        }
        let slot = (Arc::as_ptr(body).addr(), TypeId::of::<T>());
        if let Some(known) = self.derived.get::<T>(slot) {
            return Attempt::Done(known);
        }
        // Made outside the lock, so that making one thing may ask for another.
        match make() {
            Attempt::Done(made) => {
                // Both expired entries and a losing concurrent candidate can run
                // arbitrary destructors. Keep their ownership outside both locks.
                let made = Arc::new(made);
                match self.publish_control(|| self.derived.keep(slot, body, &made)) {
                    Ok((known, retired)) => {
                        drop(retired);
                        Attempt::Done(known)
                    }
                    Err(error) => Attempt::Failed(error),
                }
            }
            Attempt::Declined(reason) => Attempt::Declined(reason),
            Attempt::Failed(error) => Attempt::Failed(error),
        }
    }
}

/// What has been worked out from the bodies that are still alive.
///
/// A body is known by where it lives in memory. Every entry holds a weak reference to
/// its body, which keeps that place from being given to another body while the entry
/// exists, so the same place always means the same body.
#[derive(Default)]
struct Derived {
    kept: Mutex<BTreeMap<Slot, Kept>>,
}

/// Where a body lives in memory, and the type of what was derived from it.
type Slot = (usize, TypeId);

struct Kept {
    body: Weak<Body>,
    data: Arc<dyn Any + Send + Sync>,
}

impl Derived {
    fn get<T: Send + Sync + 'static>(&self, slot: Slot) -> Option<Arc<T>> {
        let kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(&kept.get(&slot)?.data).downcast::<T>().ok()
    }

    /// Keeps `made` unless another caller was faster, and returns what is kept.
    fn keep<T: Send + Sync + 'static>(
        &self,
        slot: Slot,
        body: &Arc<Body>,
        made: &Arc<T>,
    ) -> (Arc<T>, Vec<Kept>) {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        // A body that is gone takes along what was derived from it.
        let expired: Vec<_> =
            kept.iter().filter(|(_, entry)| entry.body.strong_count() == 0).map(|(slot, _)| *slot).collect();
        let retired = expired.into_iter().filter_map(|slot| kept.remove(&slot)).collect();
        let entry = kept
            .entry(slot)
            .or_insert_with(|| Kept { body: Arc::downgrade(body), data: Arc::<T>::clone(made) });
        let known = Arc::clone(&entry.data).downcast::<T>().unwrap_or_else(|_| Arc::clone(made));
        (known, retired)
    }
}

#[cfg(test)]
mod tests {
    use super::Context;
    use crate::plugin::{Family, Plugin, Registries};
    use crate::topo::Body;
    use krly3d_core::{Accuracy, Attempt, CallControl, CallStop, Error, FakeClock, Id};
    use std::cell::Cell;
    use std::sync::Arc;

    fn context() -> Context {
        Context::new(Accuracy::default(), Arc::new(FakeClock::default()), Registries::default())
    }

    #[test]
    fn what_is_derived_from_a_body_is_made_once() {
        let (cx, body, made) = (context(), Arc::new(Body::default()), Cell::new(0));
        let count = || {
            made.set(made.get() + 1);
            Attempt::Done(made.get())
        };

        let (first, second) = (cx.derived(&body, count), cx.derived(&body, count));

        assert_eq!((first, second, made.get()), (Attempt::Done(Arc::new(1)), Attempt::Done(Arc::new(1)), 1));
    }

    #[test]
    fn controlled_views_share_services_pipelines_and_caches_but_isolate_stops() {
        let mut registries = Registries::default();
        registries.add::<Echo>(Hello);
        let cx = context_with(registries).with_stages(vec![Id::new("node")], vec![Id::new("edit")]);
        let control = Arc::new(CallControl::new());
        let controlled = cx.with_call_control(Arc::clone(&control));
        let other = controlled.with_call_control(Arc::new(CallControl::new()));
        let body = Arc::new(Body::default());
        assert!(Arc::ptr_eq(&cx.registries, &controlled.registries));
        assert_eq!(controlled.node_stages(), cx.node_stages());
        assert_eq!(controlled.edit_stages(), cx.edit_stages());
        assert_eq!(controlled.call::<Echo>(&Id::new("say"), &0), Attempt::Done("hello".into()));
        let made = controlled.derived(&body, || Attempt::Done(7_u32)).into_result().unwrap();
        let known = cx.derived(&body, || Attempt::Done(8_u32)).into_result().unwrap();
        assert!(Arc::ptr_eq(&made, &known));
        control.cancel();
        assert_eq!(controlled.check_control(), Err(Error::from(CallStop::Cancelled)));
        assert_eq!(
            controlled.call::<Echo>(&Id::new("say"), &0),
            Attempt::Failed(Error::from(CallStop::Cancelled))
        );
        assert_eq!(cx.check_control(), Ok(()));
        assert_eq!(other.check_control(), Ok(()));
        assert!(Arc::ptr_eq(&made, &other.derived(&body, || Attempt::Done(9_u32)).into_result().unwrap()));
    }

    #[test]
    fn cancellation_during_derivation_does_not_publish_a_cache_entry() {
        let cx = context();
        let control = Arc::new(CallControl::new());
        let controlled = cx.with_call_control(Arc::clone(&control));
        let body = Arc::new(Body::default());
        let stopped = controlled.derived(&body, || {
            control.cancel();
            Attempt::Done(7_u32)
        });
        assert_eq!(stopped, Attempt::Failed(Error::from(CallStop::Cancelled)));
        assert_eq!(cx.derived(&body, || Attempt::Done(8_u32)), Attempt::Done(Arc::new(8)));
        assert_eq!(controlled.publish_control(|| 1), Err(Error::from(CallStop::Cancelled)));
    }

    #[test]
    fn every_body_and_every_type_of_data_is_remembered_apart() {
        let (cx, one, other) = (context(), Arc::new(Body::default()), Arc::new(Body::default()));

        let of_one = cx.derived(&one, || Attempt::Done(1_u32));
        let of_other = cx.derived(&other, || Attempt::Done(2_u32));
        let text_of_one = cx.derived(&one, || Attempt::Done("one"));

        assert_eq!((of_one, of_other), (Attempt::Done(Arc::new(1)), Attempt::Done(Arc::new(2))));
        assert_eq!(text_of_one, Attempt::Done(Arc::new("one")));
    }

    struct DropProbe {
        cache: std::sync::Weak<super::Derived>,
        control: Option<Arc<CallControl>>,
    }

    impl Drop for DropProbe {
        fn drop(&mut self) {
            let Some(control) = self.control.take() else { return };
            if let Some(cache) = self.cache.upgrade() {
                assert!(cache.kept.try_lock().is_ok(), "destructor ran under the cache lock");
            }
            let (sent, received) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                control.cancel();
                sent.send(()).unwrap();
            });
            received.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
            worker.join().unwrap();
        }
    }

    #[test]
    fn retired_and_losing_cache_values_drop_outside_publication_locks() {
        for expired in [true, false] {
            let base = context();
            let control = Arc::new(CallControl::new());
            let cx = base.with_call_control(Arc::clone(&control));
            let body = Arc::new(Body::default());
            let probe =
                || DropProbe { cache: Arc::downgrade(&cx.derived), control: Some(Arc::clone(&control)) };
            if expired {
                let made = cx.derived(&body, || Attempt::Done(probe())).into_result().unwrap();
                drop(made);
                drop(body);
                let fresh = Arc::new(Body::default());
                assert_eq!(cx.derived(&fresh, || Attempt::Done(9_u32)), Attempt::Done(Arc::new(9)));
            } else {
                // A recursive derivation wins after the outer cache miss, so the
                // outer value is discarded instead of stored.
                let _ = cx
                    .derived(&body, || {
                        let _ = base.derived(&body, || {
                            Attempt::Done(DropProbe { cache: Arc::downgrade(&cx.derived), control: None })
                        });
                        Attempt::Done(probe())
                    })
                    .into_result()
                    .unwrap();
            }
            assert_eq!(control.check(), Err(CallStop::Cancelled));
        }
    }

    #[test]
    fn what_could_not_be_made_is_asked_for_again() {
        let (cx, body) = (context(), Arc::new(Body::default()));

        let failed: Attempt<Arc<u32>> = cx.derived(&body, || Attempt::Failed(Error::geometry("no luck")));
        let declined: Attempt<Arc<u32>> = cx.derived(&body, || Attempt::declined("not now"));
        let made = cx.derived(&body, || Attempt::Done(7_u32));

        assert_eq!(failed, Attempt::Failed(Error::geometry("no luck")));
        assert_eq!(declined, Attempt::declined("not now"));
        assert_eq!(made, Attempt::Done(Arc::new(7)));
    }

    #[test]
    fn what_was_derived_from_a_body_is_dropped_once_the_body_is_gone() {
        let (cx, gone, kept) = (context(), Arc::new(Body::default()), Arc::new(Body::default()));
        let Attempt::Done(of_gone) = cx.derived(&gone, || Attempt::Done(1_u32)) else { panic!() };

        drop(gone);
        let made_later = cx.derived(&kept, || Attempt::Done(2_u32));

        assert_eq!(made_later, Attempt::Done(Arc::new(2)));
        assert_eq!(Arc::strong_count(&of_gone), 1);
    }

    struct Echo;

    impl Family for Echo {
        const NAME: &'static str = "echo";
        type Request = i32;
        type Response = String;
        type Context = Context;
    }

    struct Hello;

    impl Plugin<Echo> for Hello {
        fn id(&self) -> Id {
            Id::new("say")
        }

        fn call(&self, _: &Context, _: &i32) -> Attempt<String> {
            Attempt::Done("hello".to_string())
        }
    }

    fn context_with(registries: Registries) -> Context {
        Context::new(Accuracy::default(), Arc::new(FakeClock::default()), registries)
    }

    #[test]
    fn a_context_reaches_the_registry_of_a_family_by_its_marker_type() {
        let mut registries = Registries::default();
        registries.add::<Echo>(Hello);

        assert_eq!(
            context_with(registries).call::<Echo>(&Id::new("say"), &0),
            Attempt::Done("hello".to_string())
        );
    }

    #[test]
    fn a_family_without_any_plugin_declines() {
        let answer = context_with(Registries::default()).call::<Echo>(&Id::new("say"), &0);

        assert_eq!(answer, Attempt::declined("no echo plugin is registered at all"));
    }
}
