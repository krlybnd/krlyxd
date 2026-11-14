//! Port of the `depth` family: how far away things stand and how high they are. Keyed
//! by method.
//!
//! One picture fixes depth only together with something known about the world. The
//! methods here take the ground as a plane square to the vertical: a pixel below the
//! horizon that lies on the ground is then one point of space, at a distance in units of
//! the camera's height above the ground.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::ports::camera::Camera;
use crate::ports::detect::Segment;
use crate::scene::Known;
use krly3d_core::{Attempt, Id};
use nalgebra::Vector2;

pub struct Depth;

impl Family for Depth {
    type Context = crate::context::Context;
    const NAME: &'static str = "depth";
    type Request = DepthRequest;
    type Response = Depths;
}

pub struct DepthRequest {
    pub camera: Camera,
    pub segments: Vec<Segment>,
    /// The camera's height above the ground, when known; it sets the unit.
    pub eye: Option<f64>,
    /// A segment of known length; when it stands on the ground it sets the unit.
    pub known: Option<Known>,
}

/// What set the unit of the depths.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    /// The camera's height above the ground, as given.
    Eye(f64),
    /// The height of a standing segment, as given.
    Known(Known),
    /// Nothing: lengths are in multiples of the camera's height above the ground.
    CameraHeight,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Depths {
    pub unit: Unit,
    /// The vertical segments whose lower end is below the horizon, by segment id.
    pub standing: Vec<Standing>,
}

/// A vertical segment taken to stand on the ground with its lower end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Standing {
    pub segment: usize,
    /// From the ground to the upper end.
    pub height: f64,
    /// Along the ground, from the point below the camera to the foot.
    pub distance: f64,
    /// The foot on the ground along the world axes x and y, from the point below the camera.
    pub foot: Vector2<f64>,
}

pub fn depth(cx: &Context, method: &Id, request: &DepthRequest) -> Attempt<Depths> {
    cx.call::<Depth>(method, request)
}
