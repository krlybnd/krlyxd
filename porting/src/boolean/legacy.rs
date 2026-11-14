//! Adapter: the existing per-shape-pair handlers behind the strategy interface.

use super::{Attempt, BooleanRequest, BooleanStrategy};
use crate::operations::modifying::boolean::{multi_tool_boolean, BooleanOp, MultiToolOutcome};
use crate::world_graph::ModifyingOperation;

pub struct Legacy;

impl BooleanStrategy for Legacy {
    fn name(&self) -> &'static str {
        "legacy-handlers"
    }
    fn apply(&self, request: &BooleanRequest) -> Attempt {
        let op = match request.operation {
            ModifyingOperation::Union => BooleanOp::Union,
            ModifyingOperation::Subtract => BooleanOp::Subtraction,
            ModifyingOperation::Intersect => BooleanOp::Intersection,
        };
        let MultiToolOutcome { result, .. } = multi_tool_boolean(request.target, request.tools, op, request.id.to_string());
        match result {
            Ok(done) => Attempt::Done(Box::new(done.brep)),
            Err(error) => Attempt::Failed(error),
        }
    }
}
