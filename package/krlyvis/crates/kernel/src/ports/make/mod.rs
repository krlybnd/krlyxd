//! Port of the `make` family: what a statement of the vis layer makes from its
//! parameters and the data of the nodes it reads. Keyed by the statement's tool
//! (`picture`, `outline`, `frame`); a plugin's schema is the method a client calls.

use crate::context::Context;
use crate::datum::Datum;
use crate::plugin::Family;
use krly3d_core::schema::Params;
use krly3d_core::{Attempt, Id};

pub struct Make;

#[derive(Clone, Debug)]
pub struct MakeRequest {
    pub params: Params,
    pub inputs: Vec<Datum>,
}

impl Family for Make {
    type Context = crate::context::Context;
    const NAME: &'static str = "make";
    type Request = MakeRequest;
    type Response = Datum;
}

pub fn make(cx: &Context, tool: &Id, request: &MakeRequest) -> Attempt<Datum> {
    cx.call::<Make>(tool, request)
}
