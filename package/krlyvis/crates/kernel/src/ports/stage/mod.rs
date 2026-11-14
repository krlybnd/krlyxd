//! Port of the `stage` family: one step of the analysis.
//!
//! The pipeline is the ordered list of stage names kept by the context. New behaviour,
//! such as finding circles or repeated elements, is a new stage, never a change to an
//! existing one.

pub mod contract;

use crate::plugin::Family;
use crate::scene::Scene;

pub struct Stage;

impl Family for Stage {
    type Context = crate::context::Context;
    const NAME: &'static str = "stage";
    type Request = Scene;
    type Response = Scene;
}
