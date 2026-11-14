//! Assembling a kernel from outside.

use crate::api::Session;
use crate::context::Context;
use crate::kernel::Kernel;
use crate::plugin::{Family, Plugin, Registries};
use krly3d_core::{Accuracy, Clock, Id, SystemClock};
use std::sync::Arc;

/// Collects the tolerances, the clock and every plugin, then hands them over as one
/// `Context`. Nothing is registered by default: the kernel has no built-in capability.
pub struct KernelBuilder {
    accuracy: Accuracy,
    clock: Arc<dyn Clock>,
    registries: Registries,
    node_stages: Vec<Id>,
    edit_stages: Vec<Id>,
}

impl Default for KernelBuilder {
    fn default() -> Self {
        Self {
            accuracy: Accuracy::default(),
            clock: Arc::new(SystemClock::default()),
            registries: Registries::default(),
            node_stages: Vec::new(),
            edit_stages: Vec::new(),
        }
    }
}

impl KernelBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn accuracy(&mut self, accuracy: Accuracy) -> &mut Self {
        self.accuracy = accuracy;
        self
    }

    pub fn clock(&mut self, clock: Arc<dyn Clock>) -> &mut Self {
        self.clock = clock;
        self
    }

    /// Registers a plugin of family `F` behind those already registered under its id.
    pub fn plugin<F: Family<Context = Context>>(&mut self, plugin: impl Plugin<F>) -> &mut Self {
        self.registries.add(plugin);
        self
    }

    /// The stages that turn one operation into a body, in order.
    pub fn node_stages(&mut self, stages: &[&str]) -> &mut Self {
        self.node_stages = stages.iter().map(|stage| Id::new(stage)).collect();
        self
    }

    /// The stages an edit of the document passes through, in order.
    pub fn edit_stages(&mut self, stages: &[&str]) -> &mut Self {
        self.edit_stages = stages.iter().map(|stage| Id::new(stage)).collect();
        self
    }

    pub fn context(self) -> Context {
        Context::new(self.accuracy, self.clock, self.registries)
            .with_stages(self.node_stages, self.edit_stages)
    }

    pub fn build(self) -> Kernel {
        Kernel::new(self.context())
    }

    /// The kernel behind its one door; see `api`.
    pub fn session(self) -> Session {
        Session::new(self.build())
    }
}
