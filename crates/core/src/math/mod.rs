//! Plain mathematics on top of nalgebra: aliases, frames, polynomial roots, quadric algebra.

pub mod bounds;
pub mod elliptic;
pub mod frame;
pub mod mirror;
pub mod nurbs;
pub mod quadrature;
pub mod quadric;

pub mod roots;

pub use krlyxd::math::{Iso3, Mat4, Point3, Sim3, Uv, Vec3};
