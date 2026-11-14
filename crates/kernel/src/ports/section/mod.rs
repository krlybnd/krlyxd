//! Port of the `section` family: the curves two surfaces have in common.
//!
//! Rules are keyed `a|b` by surface class, e.g. `plane|sphere`. The answer is about the
//! whole surfaces; which part of a curve lies on two faces is the business of whoever asks.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::ports::curve::Curve;
use crate::ports::pair;
use crate::ports::surface::Surface;
use krly3d_core::{Attempt, Exactness};
use std::sync::Arc;

pub struct Section;

impl Family for Section {
    const NAME: &'static str = "section";
    type Request = SectionRequest;
    type Response = Cut;
    type Context = crate::context::Context;
}

pub struct SectionRequest {
    pub first: Arc<dyn Surface>,
    pub second: Arc<dyn Surface>,
}

#[derive(Clone, Debug)]
pub enum Cut {
    /// The whole curves along which the surfaces cross; none when they do not meet.
    Curves { curves: Vec<Arc<dyn Curve>>, exact: Exactness },
    /// The whole curves along which the surfaces touch without crossing, as a blend
    /// surface touches the faces it joins.
    Touching { curves: Vec<Arc<dyn Curve>>, exact: Exactness },
    /// The two surfaces are the same set of points.
    Same,
}

/// Asks the section rules for the classes of the two surfaces, in the order given and
/// then the other way round, since which surface comes first makes no difference.
/// Surfaces that touch in a single point have no curve in common.
pub fn section(cx: &Context, request: &SectionRequest) -> Attempt<Cut> {
    match pair::ask::<Section>(cx, request.first.classes(), request.second.classes(), request) {
        Attempt::Declined(reason) => {
            let swapped =
                SectionRequest { first: Arc::clone(&request.second), second: Arc::clone(&request.first) };
            match pair::ask::<Section>(cx, swapped.first.classes(), swapped.second.classes(), &swapped) {
                Attempt::Declined(other) => Attempt::Declined(format!("{reason}; {other}")),
                settled => settled,
            }
        }
        settled => settled,
    }
}
