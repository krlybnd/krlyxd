//! What every detector must satisfy: each edge of a drawn polygon is found where it was
//! drawn, and an image of noise yields at most the one false segment a detector may be
//! allowed by chance.

use super::{detect, Segment};
use crate::context::Context;
use crate::image::Pixel;
use crate::ports::sample::{self, Canvas};
use krly3d_core::{Attempt, Id};
use std::sync::Arc;

/// How far, in pixels, the ends of a found segment may lie from the drawn edge.
pub const OFF_LINE: f64 = 0.5;
/// The share of a drawn edge one found segment must cover.
pub const COVERED: f64 = 0.8;

/// A quadrilateral with no edge along a pixel axis.
pub const QUADRILATERAL: [(f64, f64); 4] = [(60.3, 50.7), (250.2, 80.1), (230.6, 200.4), (40.1, 170.9)];

pub fn check(cx: &Context, detector: &str) -> Result<(), String> {
    edges_are_found(cx, detector)?;
    noise_is_quiet(cx, detector)
}

fn edges_are_found(cx: &Context, detector: &str) -> Result<(), String> {
    let corners: Vec<Pixel> = QUADRILATERAL.iter().map(|&(x, y)| Pixel::new(x, y)).collect();
    let mut canvas = Canvas::new((300, 250), 40.0);
    canvas.fill(&corners, 200.0);
    let image = Arc::new(canvas.image().map_err(|error| error.to_string())?);
    let found = run(cx, detector, &image)?;
    for (index, &start) in corners.iter().enumerate() {
        let end = corners[(index + 1) % corners.len()];
        if !found.iter().any(|segment| covers(segment, start, end)) {
            return Err(format!("`{detector}` missed the edge {start} - {end}; found {found:?}"));
        }
    }
    Ok(())
}

fn noise_is_quiet(cx: &Context, detector: &str) -> Result<(), String> {
    let image = Arc::new(sample::noise((256, 256), 7).map_err(|error| error.to_string())?);
    let found = run(cx, detector, &image)?;
    if found.len() <= 1 {
        Ok(())
    } else {
        Err(format!("`{detector}` found {} segments in noise", found.len()))
    }
}

fn run(cx: &Context, detector: &str, image: &Arc<crate::image::Gray>) -> Result<Vec<Segment>, String> {
    match detect(cx, &Id::new(detector), image) {
        Attempt::Done(found) => Ok(found),
        other => Err(format!("`{detector}` did not run: {other:?}")),
    }
}

/// Whether a segment lies on the edge from `start` to `end` and covers enough of it.
fn covers(segment: &Segment, start: Pixel, end: Pixel) -> bool {
    let length = (end - start).norm();
    let along = (end - start) / length;
    let across = |point: Pixel| (point - start).perp(&along).abs();
    let position = |point: Pixel| (point - start).dot(&along);
    let (a, b) = (position(segment.from), position(segment.to));
    let overlap = a.max(b).min(length) - a.min(b).max(0.0);
    across(segment.from) <= OFF_LINE && across(segment.to) <= OFF_LINE && overlap >= COVERED * length
}
