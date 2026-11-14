//! Port of the `measure` family: true lengths of segments lying in one world plane.
//! Keyed by method.
//!
//! A pixel of a plane seen by a calibrated camera is one point of space up to the
//! distance of the plane, so all lengths in the plane are known up to one common scale;
//! one known length fixes it.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::ports::camera::Camera;
use crate::ports::detect::Segment;
use crate::scene::{Axis, Known};
use krly3d_core::{Attempt, Id};
use nalgebra::Vector2;

pub struct Measure;

impl Family for Measure {
    type Context = crate::context::Context;
    const NAME: &'static str = "measure";
    type Request = MeasureRequest;
    type Response = Vec<Measured>;
}

pub struct MeasureRequest {
    pub camera: Camera,
    /// The world axis square to the plane.
    pub normal: Axis,
    pub segments: Vec<Segment>,
    /// The ids of the segments to measure.
    pub which: Vec<usize>,
    /// Sets the unit; without it lengths are in multiples of the distance from the
    /// camera to the plane.
    pub known: Option<Known>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measured {
    pub segment: usize,
    pub length: f64,
    /// The ends in the plane, along its two axes in the order of [`Axis::others`],
    /// measured from the foot of the camera on the plane.
    pub from: Vector2<f64>,
    pub to: Vector2<f64>,
}

pub fn measure(cx: &Context, method: &Id, request: &MeasureRequest) -> Attempt<Vec<Measured>> {
    cx.call::<Measure>(method, request)
}
