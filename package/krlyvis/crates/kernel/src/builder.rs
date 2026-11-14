//! Assembling a vision kernel from outside.

use crate::context::Context;
use crate::plugin::{Family, Plugin, Registries};
use crate::vision::Vision;
use krly3d_core::{Clock, Id, SystemClock};
use std::sync::Arc;

/// Collects the clock, every plugin and the order of the stages, then hands them over as
/// one `Context`. Nothing is registered by default: the kernel has no built-in capability.
pub struct VisionBuilder {
    clock: Arc<dyn Clock>,
    registries: Registries,
    stages: Vec<Id>,
}

impl Default for VisionBuilder {
    fn default() -> Self {
        Self {
            clock: Arc::new(SystemClock::default()),
            registries: Registries::default(),
            stages: Vec::new(),
        }
    }
}

impl VisionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clock(&mut self, clock: Arc<dyn Clock>) -> &mut Self {
        self.clock = clock;
        self
    }

    /// Registers a plugin of family `F` behind those already registered under its id.
    pub fn plugin<F: Family>(&mut self, plugin: impl Plugin<F>) -> &mut Self {
        self.registries.add(plugin);
        self
    }

    /// The stages an analysis passes through, in order.
    pub fn stages(&mut self, stages: &[&str]) -> &mut Self {
        self.stages = stages.iter().map(|stage| Id::new(stage)).collect();
        self
    }

    pub fn context(self) -> Context {
        Context::new(self.clock, self.registries).with_stages(self.stages)
    }

    pub fn build(self) -> Vision {
        Vision::new(self.context())
    }
}
