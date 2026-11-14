//! Port of the `measure` family: one number about a body, with how exact it is.
//!
//! Measures are keyed by name (`volume`, `area`, `faces`). Queries, watchers,
//! expectations and parameter sweeps all read the same measures.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::topo::Body;
use krly3d_core::{Attempt, Exactness, Id};
use std::sync::Arc;

pub struct Measure;

impl Family for Measure {
    const NAME: &'static str = "measure";
    type Request = MeasureRequest;
    type Response = Measured;
    type Context = crate::context::Context;
}

pub struct MeasureRequest {
    pub body: Arc<Body>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measured {
    pub value: f64,
    pub exact: Exactness,
}

pub fn measure(cx: &Context, name: &str, body: &Arc<Body>) -> Attempt<Measured> {
    cx.call::<Measure>(&Id::new(name), &MeasureRequest { body: Arc::clone(body) })
}
