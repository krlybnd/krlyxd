//! The kernel's spine: one object, assembled by a builder, with its
//! collaborators injected.
//!
//! * `Probe`   – how the kernel looks at geometry (strategy, injected);
//! * `Layer`   – a visitor that derives something from a body (names, keys,
//!               outlines, later PMI) and keeps it per node;
//! * `walk`    – the single traversal all layers share;
//! * `Kernel`  – owns the scene, refreshes the layers when a body changed.

pub mod probe;
pub mod visit;

use crate::boolean::BooleanStrategy;
use crate::brep::Accuracy;
use crate::math::Point3;
use crate::perception::probe::Hit;
use crate::world_graph::{ChangeSet, EditScope, GraphError, ModifyingOperation, WorldGraph};
use probe::{BoxTree, Probe};
use std::any::Any;
use std::collections::BTreeMap;
use visit::{walk, Visitor};

/// A derived, per-body layer. It is a visitor; the kernel decides when it runs.
pub trait Layer: Visitor + Send {
    fn name(&self) -> &'static str;
    /// forget what was derived for a node (it was removed or is being rebuilt)
    fn forget(&mut self, node: &str);
    fn visitor(&mut self) -> &mut dyn Visitor;
    fn as_any(&self) -> &dyn Any;
}

pub struct Kernel {
    pub(crate) world: WorldGraph,
    probe: Box<dyn Probe>,
    boolean: Box<dyn BooleanStrategy>,
    layers: Vec<Box<dyn Layer>>,
    /// shape revision each node's layers were last built from
    fresh: BTreeMap<String, u64>,
}

pub struct KernelBuilder {
    accuracy: Accuracy,
    probe: Option<Box<dyn Probe>>,
    boolean: Option<Box<dyn BooleanStrategy>>,
    layers: Vec<Box<dyn Layer>>,
}

impl Default for KernelBuilder {
    fn default() -> Self {
        Self { accuracy: Accuracy::STANDARD, probe: None, boolean: None, layers: Vec::new() }
    }
}

impl KernelBuilder {
    pub fn accuracy(mut self, accuracy: Accuracy) -> Self {
        self.accuracy = accuracy;
        self
    }
    /// Inject the probe strategy (default: the box tree).
    pub fn probe(mut self, probe: impl Probe + 'static) -> Self {
        self.probe = Some(Box::new(probe));
        self
    }
    /// Inject how booleans are computed (default: named results over the legacy handlers).
    pub fn boolean(mut self, strategy: impl BooleanStrategy + 'static) -> Self {
        self.boolean = Some(Box::new(strategy));
        self
    }
    /// Add a derived layer; layers run together in one traversal.
    pub fn layer(mut self, layer: impl Layer + 'static) -> Self {
        self.layers.push(Box::new(layer));
        self
    }
    pub fn build(self) -> Result<Kernel, GraphError> {
        Ok(Kernel {
            world: WorldGraph::new(self.accuracy)?,
            probe: self.probe.unwrap_or_else(|| Box::new(BoxTree::new())),
            boolean: self.boolean.unwrap_or_else(|| Box::new(crate::boolean::standard())),
            layers: self.layers,
            fresh: BTreeMap::new(),
        })
    }
}

impl Kernel {
    pub fn builder() -> KernelBuilder {
        KernelBuilder::default()
    }
    pub fn world(&self) -> &WorldGraph {
        &self.world
    }
    pub fn world_mut(&mut self) -> &mut WorldGraph {
        &mut self.world
    }
    pub fn probe_name(&self) -> &'static str {
        self.probe.name()
    }

    /// Boolean through the injected strategy.
    pub fn operate(
        &mut self,
        target: &str,
        operation: ModifyingOperation,
        tools: &[String],
        scope: EditScope,
    ) -> Result<ChangeSet, GraphError> {
        self.world.operate_with(target, operation, tools, scope, self.boolean.as_ref())
    }

    /// Ray in world coordinates against one body, through the injected probe.
    pub fn ray(&self, node: &str, origin: Point3, dir: Point3) -> Result<Vec<Hit>, GraphError> {
        let brep = self.world.placed_brep(node)?;
        Ok(self.probe.ray(&brep, origin, dir)?)
    }

    /// Bring the layers of one body up to date. Returns true if they were rebuilt.
    pub fn refresh(&mut self, node: &str) -> Result<bool, GraphError> {
        let revision = self.world.shape(node)?.revision;
        if self.fresh.get(node) == Some(&revision) {
            return Ok(false);
        }
        let brep = self.world.placed_brep(node)?;
        for layer in &mut self.layers {
            layer.forget(node);
        }
        let mut visitors: Vec<&mut dyn Visitor> = self.layers.iter_mut().map(|l| l.visitor()).collect();
        walk(node, &brep, &mut visitors)?;
        self.fresh.insert(node.to_string(), revision);
        Ok(true)
    }

    /// The layer of a given type, if it was injected.
    pub fn layer<T: Layer + 'static>(&self) -> Option<&T> {
        self.layers.iter().find_map(|l| l.as_any().downcast_ref::<T>())
    }
}

#[cfg(test)]
mod tests;
