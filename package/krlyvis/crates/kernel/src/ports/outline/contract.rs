//! What every outline plugin must satisfy: a drawn quadrilateral on a plain background
//! comes back as a polygon whose corners lie within half a pixel of the drawn ones, and a
//! blank picture is declined rather than outlined.

use super::{outline, OutlineRequest};
use crate::context::Context;
use crate::image::Pixel;
use crate::ports::sample::Canvas;
use krly3d_core::Attempt;
use std::sync::Arc;

/// How far the corners of the polygon may lie from the drawn ones.
pub const OFF_CORNER: f64 = 0.5;

/// An axis-aligned rectangle, so that the drawn corners are corners of pixels.
pub const RECTANGLE: [(f64, f64); 4] = [(40.0, 30.0), (210.0, 30.0), (210.0, 150.0), (40.0, 150.0)];

pub fn check(cx: &Context, method: &str) -> Result<(), String> {
    corners_are_found(cx, method)?;
    blank_is_declined(cx, method)
}

fn request(image: crate::image::Gray) -> OutlineRequest {
    OutlineRequest { image: Arc::new(image), region: None, seed: None, threshold: None, simplify: 1.0 }
}

fn corners_are_found(cx: &Context, method: &str) -> Result<(), String> {
    let corners: Vec<Pixel> = RECTANGLE.iter().map(|&(x, y)| Pixel::new(x, y)).collect();
    let mut canvas = Canvas::new((260, 190), 230.0);
    canvas.fill(&corners, 40.0);
    let image = canvas.image().map_err(|error| error.to_string())?;
    let shape = outline(cx, method, &request(image)).into_result().map_err(|error| error.to_string())?;
    let polygon = shape.polygons.first().ok_or(format!("`{method}` found no polygon"))?;
    if polygon.len() != corners.len() {
        return Err(format!("`{method}` gave {} corners for a rectangle: {polygon:?}", polygon.len()));
    }
    for corner in &corners {
        if !polygon.iter().any(|point| (point - corner).norm() <= OFF_CORNER) {
            return Err(format!("`{method}` missed the corner {corner}; gave {polygon:?}"));
        }
    }
    Ok(())
}

fn blank_is_declined(cx: &Context, method: &str) -> Result<(), String> {
    let image = Canvas::new((80, 60), 128.0).image().map_err(|error| error.to_string())?;
    match outline(cx, method, &request(image)) {
        Attempt::Declined(_) => Ok(()),
        other => Err(format!("`{method}` outlined a blank picture: {other:?}")),
    }
}
