//! What every measure must satisfy: the number follows the body when the body is moved
//! and scaled.

use super::{measure, Measured};
use crate::context::Context;
use crate::ports::sample;
use crate::topo::Body;
use krly3d_core::Attempt;
use std::sync::Arc;

/// `dimension` is the power of length the measure has: 3 for a volume, 2 for an area,
/// 0 for a count. Moving the body away from the origin and scaling it must change the
/// value by exactly that power of the scale, which catches any dependence on position.
pub fn check(cx: &Context, name: &str, body: &Body, stated: &Measured, dimension: i32) -> Result<(), String> {
    let by = sample::similarity();
    let moved = Arc::new(body.transformed(&by));
    let expected = stated.value * by.scaling().powi(dimension);
    match measure(cx, name, &moved) {
        Attempt::Done(Measured { value, .. }) => {
            let slack = cx.accuracy().relative() * (1.0 + expected.abs());
            if (value - expected).abs() <= slack {
                Ok(())
            } else {
                Err(format!("`{name}` reads {value} on the moved body, expected {expected}"))
            }
        }
        other => Err(format!("`{name}` does not handle the moved body: {other:?}")),
    }
}
