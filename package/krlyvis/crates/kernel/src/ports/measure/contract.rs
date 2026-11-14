//! What every measuring method must satisfy: segments drawn by a known camera on a wall
//! of known size, with one of its lengths given, measure as they were drawn.

use super::{measure, MeasureRequest};
use crate::context::Context;
use crate::ports::detect::Segment;
use crate::ports::sample;
use crate::scene::{Axis, Known};
use krly3d_core::{Attempt, Id};
use nalgebra::{Vector2, Vector3};

const CLOSE: f64 = 1e-9;

pub fn check(cx: &Context, method: &str) -> Result<(), String> {
    let camera = sample::camera(35.0, 15.0);
    let eye = Vector3::new(-5.0, -14.0, 7.0);
    // A wall 12 wide and 6 high in the plane y = 0: its edges and a diagonal.
    let ends = [
        ([0.0, 0.0, 0.0], [12.0, 0.0, 0.0]),
        ([0.0, 0.0, 6.0], [12.0, 0.0, 6.0]),
        ([0.0, 0.0, 0.0], [0.0, 0.0, 6.0]),
        ([12.0, 0.0, 0.0], [12.0, 0.0, 6.0]),
        ([0.0, 0.0, 0.0], [12.0, 0.0, 6.0]),
    ];
    let mut segments = Vec::new();
    for (from, to) in ends {
        let seen = |point: [f64; 3]| sample::project(&camera, &eye, &Vector3::from(point));
        let (Some(from), Some(to)) = (seen(from), seen(to)) else {
            return Err("the sample wall is behind the sample camera".to_string());
        };
        segments.push(Segment { from, to, width: 1.0, significance: 10.0 });
    }
    let request = MeasureRequest {
        camera,
        normal: Axis::Y,
        segments,
        which: (0..ends.len()).collect(),
        known: Some(Known { segment: 0, length: 12.0 }),
    };
    let found = match measure(cx, &Id::new(method), &request) {
        Attempt::Done(found) => found,
        other => return Err(format!("`{method}` did not measure: {other:?}")),
    };
    if found.len() != ends.len() {
        return Err(format!("`{method}` measured {} of {} segments", found.len(), ends.len()));
    }
    for (measured, (from, to)) in found.iter().zip(ends) {
        let span = Vector3::from(to) - Vector3::from(from);
        // The plane square to y is spanned by z and x, in that order.
        let expected = Vector2::new(span.z, span.x);
        let length_off = (measured.length - span.norm()).abs() > CLOSE * span.norm();
        if length_off || (measured.to - measured.from - expected).norm() > CLOSE * span.norm() {
            return Err(format!("`{method}` measured {measured:?} for the span {span}"));
        }
    }
    Ok(())
}
