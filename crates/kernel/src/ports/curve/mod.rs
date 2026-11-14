//! Port of the `curve` family: a parameterised space curve.
//!
//! An object family: a curve carries its own data and answers several questions, so it
//! has its own trait instead of the one-call `Plugin`.

pub mod contract;

use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::{Point3, Sim3, Vec3};
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

pub trait Curve: Debug + Send + Sync + 'static {
    /// Class names from the most specific to the most general; the first is the kind.
    /// Pair rules are looked up in this order.
    fn classes(&self) -> &'static [&'static str];

    fn point(&self, t: f64) -> Point3;

    /// Derivative of `point` with respect to the parameter.
    fn tangent(&self, t: f64) -> Vec3;

    /// Parameter of the point of the curve closest to `point`.
    fn project(&self, point: &Point3) -> f64;

    /// Length of the parameter interval after which a closed curve repeats.
    fn period(&self) -> Option<f64>;

    /// Half the integral of `p × dp` from `from` to `to`: the vector area of the fan between
    /// the world origin and that stretch of the curve. Summed around the boundary of a
    /// face it gives the integral of the face's normal, whatever the surface.
    fn vector_area(&self, from: f64, to: f64) -> Vec3;

    /// The smallest box around the stretch of the curve between two parameters.
    fn bounds(&self, from: f64, to: f64) -> Bounds;

    /// Length of the stretch of the curve between two parameters.
    fn length(&self, from: f64, to: f64) -> f64;

    /// The curve in a few words with its sizes, e.g. `circle r=3 axis=+z`.
    fn summary(&self) -> String;

    /// The curve after the whole space is moved by `by`. Parameters are preserved:
    /// the moved curve at `t` is the moved point of this curve at `t`.
    fn transformed(&self, by: &Sim3) -> Arc<dyn Curve>;

    /// The curve reflected in `mirror`, with parameters preserved: the image at `t` is the
    /// reflected point of this curve at `t`. `None` when the kind cannot be reflected, in
    /// which case a mirror of a body using it is declined.
    fn mirrored(&self, _mirror: &Mirror) -> Option<Arc<dyn Curve>> {
        None
    }

    /// Gives pair rules access to the concrete type.
    fn as_any(&self) -> &dyn Any;
}
