//! What a plugin is given when it is called.

use crate::plugin::{Family, Registries, Registry};
use krly3d_core::{Attempt, Clock, Id};
use std::sync::Arc;

/// The injected dependencies of a call. Plugins hold no global state: whatever they
/// need beyond their request, including other plugins, comes from here.
pub struct Context {
    clock: Arc<dyn Clock>,
    registries: Registries,
    stages: Vec<Id>,
}

impl Context {
    pub fn new(clock: Arc<dyn Clock>, registries: Registries) -> Self {
        Self { clock, registries, stages: Vec::new() }
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
        match self.registry::<F>() {
            Some(registry) => registry.call(self, id, request),
            None => Attempt::declined(format!("no {} plugin is registered at all", F::NAME)),
        }
    }
}
