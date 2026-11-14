//! What every shape must satisfy. The tests of a shape plugin call `check` on instances.

use super::Shape;
use crate::context::Context;
use krly3d_core::math::Point3;

/// At each sample the distance is a number, the points inside lie in the stated box, and
/// the distance changes no faster than the point moves: `|d(p) − d(q)| ≤ |p − q|`, which
/// is what lets a marcher step by it.
pub fn check(cx: &Context, shape: &dyn Shape, samples: &[Point3]) -> Result<(), String> {
    let bounds = shape.bounds(cx).into_result().map_err(|error| error.to_string())?;
    let slack = cx.accuracy().length();
    let mut known = Vec::with_capacity(samples.len());
    for point in samples {
        let signed =
            shape.distance(cx, point).into_result().map_err(|error| format!("at {point:?}: {error}"))?;
        if !signed.distance.is_finite() {
            return Err(format!("the distance at {point:?} is {}", signed.distance));
        }
        if signed.distance < -slack && !bounds.grown(slack).contains(point) {
            return Err(format!("{point:?} is inside but outside the stated box {bounds:?}"));
        }
        known.push((point, signed.distance));
    }
    for (index, (p, dp)) in known.iter().enumerate() {
        for (q, dq) in &known[index + 1..] {
            let (change, moved) = ((dp - dq).abs(), (*p - *q).norm());
            if change > moved + slack {
                return Err(format!(
                    "the distance changes by {change} between {p:?} and {q:?}, {moved} apart"
                ));
            }
        }
    }
    Ok(())
}
