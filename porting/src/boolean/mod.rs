//! Boolean operations as pluggable strategies.
//!
//! Open for extension, closed for modification: a new way of computing a
//! boolean is a new type implementing `BooleanStrategy`, appended to a `Chain`.
//! Nothing in the scene graph, the protocol or the existing strategies changes.
//!
//! * `Chain`  – tries strategies in order; the first that completes wins;
//! * `Named`  – decorator: gives the result's faces stable, inherited names;
//! * `Legacy` – adapter around the per-shape-pair handlers.

mod legacy;
#[cfg(test)]
mod tests;

pub use legacy::Legacy;

use crate::brep::{BrepEnvelope, GeometryError};
use crate::world_graph::ModifyingOperation;

/// Everything a strategy may look at. Tools are already placed in the target's frame.
pub struct BooleanRequest<'a> {
    pub id: &'a str,
    pub operation: ModifyingOperation,
    pub target: &'a BrepEnvelope,
    pub tools: &'a [BrepEnvelope],
    /// scene names of the tools, parallel to `tools`
    pub tool_names: &'a [String],
}

pub enum Attempt {
    Done(Box<BrepEnvelope>),
    /// "not a case I handle" – the next strategy should try
    Declined(String),
    /// "my case, but it went wrong"
    Failed(GeometryError),
}

pub trait BooleanStrategy: Send + Sync {
    fn name(&self) -> &'static str;
    fn apply(&self, request: &BooleanRequest) -> Attempt;
}

/// Strategies tried in order.
#[derive(Default)]
pub struct Chain {
    links: Vec<Box<dyn BooleanStrategy>>,
}

impl Chain {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn then(mut self, strategy: impl BooleanStrategy + 'static) -> Self {
        self.links.push(Box::new(strategy));
        self
    }
    pub fn names(&self) -> Vec<&'static str> {
        self.links.iter().map(|l| l.name()).collect()
    }
}

impl BooleanStrategy for Chain {
    fn name(&self) -> &'static str {
        "chain"
    }
    fn apply(&self, request: &BooleanRequest) -> Attempt {
        let mut failure = None;
        let mut reasons = Vec::new();
        for link in &self.links {
            match link.apply(request) {
                Attempt::Done(brep) => return Attempt::Done(brep),
                Attempt::Declined(why) => reasons.push(format!("{}: {why}", link.name())),
                Attempt::Failed(error) => {
                    reasons.push(format!("{}: {error:?}", link.name()));
                    failure.get_or_insert(error);
                }
            }
        }
        match failure {
            Some(error) => Attempt::Failed(error),
            None => Attempt::Declined(if reasons.is_empty() { "no strategy installed".into() } else { reasons.join("; ") }),
        }
    }
}

/// Decorator: whatever the inner strategy produced, its faces inherit the
/// names of the target's faces, then of the tools' faces (`tool:face`);
/// faces with no predecessor are named after the operation.
pub struct Named<S>(pub S);

impl<S: BooleanStrategy> BooleanStrategy for Named<S> {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn apply(&self, request: &BooleanRequest) -> Attempt {
        match self.0.apply(request) {
            Attempt::Done(mut brep) => {
                let mut inputs: Vec<(&BrepEnvelope, Option<&str>)> = vec![(request.target, None)];
                for (tool, name) in request.tools.iter().zip(request.tool_names) {
                    inputs.push((tool, Some(name.as_str())));
                }
                let created = match request.operation {
                    ModifyingOperation::Union => "union",
                    ModifyingOperation::Subtract => "cut",
                    ModifyingOperation::Intersect => "common",
                };
                crate::naming::inherit_from(&inputs, &mut brep, created);
                Attempt::Done(brep)
            }
            other => other,
        }
    }
}

/// The default: named results, legacy handlers as the only link for now.
pub fn standard() -> Named<Chain> {
    Named(Chain::new().then(Legacy))
}
