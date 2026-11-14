//! Plain mathematics on top of nalgebra: aliases, frames, polynomial roots, quadric algebra.

pub mod bounds;
pub mod elliptic;
pub mod frame;
pub mod mirror;
pub mod nurbs;
pub mod quadrature;
pub mod quadric;

pub mod roots;

pub type Vec3 = nalgebra::Vector3<f64>;
pub type Point3 = nalgebra::Point3<f64>;
/// Surface parameters `(u, v)`.
pub type Uv = nalgebra::Point2<f64>;
pub type Mat4 = nalgebra::Matrix4<f64>;
/// Rigid placement mapping local to world coordinates. The rotation is kept as a matrix,
/// so an axis-aligned frame stays exact.
pub type Iso3 = nalgebra::IsometryMatrix3<f64>;
/// Rigid placement with a uniform scale.
pub type Sim3 = nalgebra::SimilarityMatrix3<f64>;
