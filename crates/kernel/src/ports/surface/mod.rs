//! Port of the `surface` family: a parameterised surface that also knows its equation.
//!
//! An object family, like `curve`.

pub mod contract;

use crate::ports::curve::Curve;
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::quadric::Quadric;
use krly3d_core::math::{Point3, Sim3, Uv, Vec3};
use krly3d_core::Exactness;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// A piece of a curve run from one parameter to another, which may be the smaller one.
#[derive(Clone, Debug)]
pub struct Stretch {
    pub curve: Arc<dyn Curve>,
    pub from: f64,
    pub to: f64,
}

impl Stretch {
    /// The point `fraction` of the way from the start (0) to the end (1).
    pub fn point(&self, fraction: f64) -> Point3 {
        self.curve.point(self.from + (self.to - self.from) * fraction)
    }

    /// Direction of travel at `fraction` of the way.
    pub fn travel(&self, fraction: f64) -> Vec3 {
        self.curve.tangent(self.from + (self.to - self.from) * fraction) * (self.to - self.from).signum()
    }
}

pub trait Surface: Debug + Send + Sync + 'static {
    /// Class names from the most specific to the most general; the first is the kind.
    /// Pair rules are looked up in this order.
    fn classes(&self) -> &'static [&'static str];

    fn point(&self, uv: Uv) -> Point3;

    /// Unit normal at `uv`. For a closed surface it points away from the enclosed side.
    fn normal(&self, uv: Uv) -> Vec3;

    /// Parameters of the point of the surface closest to `point`.
    fn project(&self, point: &Point3) -> Uv;

    /// Signed distance from the surface, positive on the side the normal points to.
    fn distance(&self, point: &Point3) -> f64 {
        let uv = self.project(point);
        (point - self.point(uv)).dot(&self.normal(uv))
    }

    /// Appends to `hits` the parameters `t` at which `origin + t·direction` meets the
    /// surface, with multiplicity, and says how exact they are. This is the first question
    /// any representation must answer, so it is asked directly and without allocation
    /// instead of through a rule.
    fn line(&self, origin: &Point3, direction: &Vec3, hits: &mut Vec<f64>) -> Exactness;

    /// The surface as a quadric, positive on the side of the normal, when it is one.
    fn quadric(&self) -> Option<Quadric> {
        None
    }

    /// Period of `u` and of `v`, for the parameters that close on themselves.
    fn periods(&self) -> [Option<f64>; 2];

    /// The values of `v` at which the whole curve of constant `v` is one point, as at the
    /// poles of a sphere or the apex of a cone. A face that reaches such a value is laid
    /// out flat up to it.
    fn poles(&self) -> Vec<f64> {
        Vec::new()
    }

    /// Euler characteristic of the complete surface when it is closed (2 for a sphere,
    /// 0 for a torus); `None` for a surface that needs a boundary to be a face.
    fn euler_characteristic(&self) -> Option<i32> {
        None
    }

    /// A path on the surface from one of its points to another, as stretches of curves
    /// simple enough to intersect in closed form. Empty when the two points coincide.
    fn walk(&self, from: &Point3, to: &Point3) -> Vec<Stretch>;

    /// The points where the surface, taken whole, reaches farthest along `direction` or
    /// against it, for a surface that has such points. Together with the boundary of a
    /// face they decide how far the face reaches.
    fn extremes(&self, direction: &Vec3) -> Vec<Point3>;

    /// The surface in a few words with its sizes, e.g. `cylinder r=3 axis=+z`.
    fn summary(&self) -> String;

    /// The surface after the whole space is moved by `by`.
    fn transformed(&self, by: &Sim3) -> Arc<dyn Surface>;

    /// The surface reflected in `mirror`: the reflected point set. Its parameters and the
    /// sense of its normal may differ from this surface's; whoever keeps a face on it reads
    /// them again. `None` when the kind cannot be reflected.
    fn mirrored(&self, _mirror: &Mirror) -> Option<Arc<dyn Surface>> {
        None
    }

    /// Gives pair rules access to the concrete type.
    fn as_any(&self) -> &dyn Any;
}
