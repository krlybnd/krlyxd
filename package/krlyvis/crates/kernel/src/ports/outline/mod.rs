//! Port of the `outline` family: where the subject of a picture is, as polygons.
//!
//! The answer is vectors, not pixels: the boundary of the subject as closed polygons in
//! pixel coordinates (x to the right, y down, corners of pixels at whole numbers), the
//! outer boundary first. Whatever turns pixels into the subject — a threshold against the
//! background, a seed, a learned mask one day — is a plugin.

pub mod contract;

use crate::context::Context;
use crate::image::{Gray, Pixel};
use crate::plugin::Family;
use krly3d_core::{Attempt, Id};
use std::sync::Arc;

pub struct Outline;

#[derive(Clone, Debug)]
pub struct OutlineRequest {
    pub image: Arc<Gray>,
    /// The part of the picture to look in, as [left, top, right, bottom] pixels; all of it
    /// when absent.
    pub region: Option<[usize; 4]>,
    /// A pixel of the subject; the largest blob is taken when absent.
    pub seed: Option<Pixel>,
    /// Brightness that separates subject and background; found from the picture when absent.
    pub threshold: Option<f64>,
    /// How far, in pixels, the polygon may cut corners of the traced boundary.
    pub simplify: f64,
}

/// The subject's boundary and what was decided on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// Closed polygons in pixel coordinates, the outer boundary first, without the first
    /// point repeated at the end.
    pub polygons: Vec<Vec<Pixel>>,
    /// Pixels of the subject.
    pub pixels: usize,
    pub threshold: f64,
    /// Whether the subject is darker than its background.
    pub dark: bool,
}

impl Family for Outline {
    type Context = crate::context::Context;
    const NAME: &'static str = "outline";
    type Request = OutlineRequest;
    type Response = Shape;
}

pub fn outline(cx: &Context, method: &str, request: &OutlineRequest) -> Attempt<Shape> {
    cx.call::<Outline>(&Id::new(method), request)
}
