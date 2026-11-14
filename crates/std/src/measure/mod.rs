//! Measures: one number about a body.

pub mod area;
pub mod count;
pub mod volume;

use krly3d_core::{Attempt, Exactness};
use krly3d_kernel::ports::content::{content, ContentRequest, FaceContent};
use krly3d_kernel::ports::measure::{Measure, Measured};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, KernelBuilder};
use std::sync::Arc;

pub fn register(builder: &mut KernelBuilder) {
    builder.plugin::<Measure>(volume::Volume).plugin::<Measure>(area::Area);
    for count in count::Count::all() {
        builder.plugin::<Measure>(count);
    }
}

/// Adds up one quantity of the content of every face. The sum is as exact as its least
/// exact term, and a face nobody can measure is named in the reason.
fn total(cx: &Context, body: &Arc<Body>, share: fn(&FaceContent) -> f64) -> Attempt<Measured> {
    let mut sum = Measured { value: 0.0, exact: Exactness::Exact };
    for (face, data) in body.faces.iter().enumerate() {
        match content(cx, &ContentRequest { body: Arc::clone(body), face }) {
            Attempt::Done(found) => {
                sum.value += share(&found);
                sum.exact = sum.exact.and(found.exact);
            }
            Attempt::Declined(reason) => return Attempt::declined(format!("face `{}`: {reason}", data.name)),
            Attempt::Failed(error) => return Attempt::Failed(error),
        }
    }
    Attempt::Done(sum)
}
