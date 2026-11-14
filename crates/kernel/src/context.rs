//! What a plugin is given when it is called.

use crate::plugin::{Family, Registries, Registry};
use crate::topo::Body;
use krly3d_core::{Accuracy, Attempt, Clock, Id};
use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::{Mutex, PoisonError, Weak};

/// The injected dependencies of a call. Plugins hold no global state: whatever they
/// need beyond their request, including other plugins, comes from here.
pub struct Context {
    accuracy: Accuracy,
    clock: Arc<dyn Clock>,
    registries: Registries,
    node_stages: Vec<Id>,
    edit_stages: Vec<Id>,
    derived: Derived,
}

impl Context {
    pub fn new(accuracy: Accuracy, clock: Arc<dyn Clock>, registries: Registries) -> Self {
        Self {
            accuracy,
            clock,
            registries,
            node_stages: Vec::new(),
            edit_stages: Vec::new(),
            derived: Derived::default(),
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
        let slot = (Arc::as_ptr(body).addr(), TypeId::of::<T>());
        if let Some(known) = self.derived.get::<T>(slot) {
            return Attempt::Done(known);
        }
        // Made outside the lock, so that making one thing may ask for another.
        make().map(|made| self.derived.keep(slot, body, Arc::new(made)))
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
    fn keep<T: Send + Sync + 'static>(&self, slot: Slot, body: &Arc<Body>, made: Arc<T>) -> Arc<T> {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        // A body that is gone takes along what was derived from it.
        kept.retain(|_, entry| entry.body.strong_count() > 0);
        let entry = kept
            .entry(slot)
            .or_insert_with(|| Kept { body: Arc::downgrade(body), data: Arc::<T>::clone(&made) });
        Arc::clone(&entry.data).downcast::<T>().unwrap_or(made)
    }
}

#[cfg(test)]
mod tests {
    use super::Context;
    use crate::plugin::{Family, Plugin, Registries};
    use crate::topo::Body;
    use krly3d_core::{Accuracy, Attempt, Error, FakeClock, Id};
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
    fn every_body_and_every_type_of_data_is_remembered_apart() {
        let (cx, one, other) = (context(), Arc::new(Body::default()), Arc::new(Body::default()));

        let of_one = cx.derived(&one, || Attempt::Done(1_u32));
        let of_other = cx.derived(&other, || Attempt::Done(2_u32));
        let text_of_one = cx.derived(&one, || Attempt::Done("one"));

        assert_eq!((of_one, of_other), (Attempt::Done(Arc::new(1)), Attempt::Done(Arc::new(2))));
        assert_eq!(text_of_one, Attempt::Done(Arc::new("one")));
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
