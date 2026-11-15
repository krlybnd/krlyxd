//! What a plugin is given when it is called.

use crate::plugin::{Family, Registries, Registry};
use krly3d_core::{Attempt, CallControl, Clock, Error, Id};
use std::sync::Arc;

/// The injected dependencies of a call. Plugins hold no global state: whatever they
/// need beyond their request, including other plugins, comes from here.
#[derive(Clone)]
pub struct Context {
    clock: Arc<dyn Clock>,
    registries: Arc<Registries>,
    stages: Vec<Id>,
    control: Option<Arc<CallControl>>,
}

impl Context {
    pub fn new(clock: Arc<dyn Clock>, registries: Registries) -> Self {
        Self { clock, registries: Arc::new(registries), stages: Vec::new(), control: None }
    }

    /// A per-call view sharing the immutable providers without changing this context.
    pub fn with_call_control(&self, control: Arc<CallControl>) -> Self {
        Self { control: Some(control), ..self.clone() }
    }

    /// Check a cooperative work boundary. Legacy contexts have no control.
    pub fn check_control(&self) -> Result<(), Error> {
        self.control.as_ref().map_or(Ok(()), |control| control.check().map_err(Error::from))
    }

    /// The same context with the pipeline set: the stages an analysis passes through.
    pub fn with_stages(mut self, stages: Vec<Id>) -> Self {
        self.stages = stages;
        self
    }

    pub fn stages(&self) -> &[Id] {
        &self.stages
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
        let result = match self.registry::<F>() {
            Some(registry) => registry.call(self, id, request),
            None => Attempt::declined(format!("no {} plugin is registered at all", F::NAME)),
        };
        match self.check_control() {
            Ok(()) => result,
            Err(error) => Attempt::Failed(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Context;
    use crate::plugin::{Family, Registries};
    use krly3d_core::{Attempt, CallControl, CallStop, Error, FakeClock, Id};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Echo;

    impl Family for Echo {
        type Context = Context;
        const NAME: &'static str = "echo";
        type Request = ();
        type Response = usize;
    }

    struct Count(Arc<AtomicUsize>);

    impl crate::plugin::Plugin<Echo> for Count {
        fn id(&self) -> Id {
            Id::new("count")
        }

        fn call(&self, _: &Context, _: &()) -> Attempt<usize> {
            Attempt::Done(self.0.fetch_add(1, Ordering::Relaxed))
        }
    }

    #[test]
    fn controlled_views_share_providers_and_stages_but_isolate_stops() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registries = Registries::default();
        registries.add::<Echo>(Count(Arc::clone(&calls)));
        let base =
            Context::new(Arc::new(FakeClock::default()), registries).with_stages(vec![Id::new("stage")]);
        let control = Arc::new(CallControl::new());
        let stopped = base.with_call_control(Arc::clone(&control));
        let other = stopped.with_call_control(Arc::new(CallControl::new()));
        assert!(Arc::ptr_eq(&base.registries, &stopped.registries));
        assert!(Arc::ptr_eq(&base.clock, &stopped.clock));
        assert_eq!(stopped.stages(), base.stages());
        control.cancel();
        assert_eq!(stopped.check_control(), Err(Error::from(CallStop::Cancelled)));
        assert_eq!(
            stopped.call::<Echo>(&Id::new("count"), &()),
            Attempt::Failed(Error::from(CallStop::Cancelled))
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(base.call::<Echo>(&Id::new("count"), &()), Attempt::Done(0));
        assert_eq!(other.call::<Echo>(&Id::new("count"), &()), Attempt::Done(1));
        assert!(base.control.is_none());
    }

    struct StopBeforeReturn(Arc<CallControl>);

    impl crate::plugin::Plugin<Echo> for StopBeforeReturn {
        fn id(&self) -> Id {
            Id::new("stop")
        }

        fn call(&self, _: &Context, _: &()) -> Attempt<usize> {
            self.0.cancel();
            Attempt::Done(42)
        }
    }

    #[test]
    fn stop_inside_an_uninterruptible_provider_is_observed_after_it_returns() {
        let control = Arc::new(CallControl::new());
        let mut registries = Registries::default();
        registries.add::<Echo>(StopBeforeReturn(Arc::clone(&control)));
        let base = Context::new(Arc::new(FakeClock::default()), registries);
        let controlled = base.with_call_control(control);
        assert_eq!(
            controlled.call::<Echo>(&Id::new("stop"), &()),
            Attempt::Failed(Error::from(CallStop::Cancelled))
        );
        assert_eq!(base.check_control(), Ok(()));
    }
}
