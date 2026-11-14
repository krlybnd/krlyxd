//! Port of the `meet` family: where a curve meets a surface.
//!
//! Rules are keyed `curve|surface`, e.g. `circle|quadric`. Ray hits, the end points of
//! section curves and the splitting points of a boolean all come from here.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::ports::curve::Curve;
use crate::ports::pair;
use crate::ports::surface::Surface;
use krly3d_core::{Attempt, Exactness};
use std::sync::Arc;

pub struct Meet;

impl Family for Meet {
    const NAME: &'static str = "meet";
    type Request = MeetRequest;
    type Response = Meeting;
    type Context = crate::context::Context;
}

pub struct MeetRequest {
    pub curve: Arc<dyn Curve>,
    pub surface: Arc<dyn Surface>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Meeting {
    /// Parameters of the curve at which it meets the surface: ascending, with
    /// multiplicity, and within one period for a closed curve.
    At { parameters: Vec<f64>, exact: Exactness },
    /// The whole curve lies in the surface.
    Within,
}

/// Asks the meet rules for the classes of the curve and of the surface.
pub fn meet(cx: &Context, request: &MeetRequest) -> Attempt<Meeting> {
    pair::ask::<Meet>(cx, request.curve.classes(), request.surface.classes(), request)
}
