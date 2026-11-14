//! Content rules: area and volume share of a face, one rule per kind of surface.
//!
//! All of them use the same identity. With `V` the integral of the outward normal over
//! the face (a boundary integral that any face has in closed form for line and circle
//! edges), `p·n` reduces on each analytic surface to a constant along `V` plus a multiple
//! of the area, so only the area needs a formula of its own.

pub mod cone;
pub mod cylinder;
pub mod ellipsoid;
pub mod nurbs;
pub mod plane;
pub mod sphere;
pub mod torus;

use krly3d_kernel::ports::content::Content;
use krly3d_kernel::topo::Use;
use krly3d_kernel::KernelBuilder;

pub fn register(builder: &mut KernelBuilder) {
    builder
        .plugin::<Content>(plane::PlaneContent)
        .plugin::<Content>(cylinder::CylinderContent)
        .plugin::<Content>(cone::ConeContent)
        .plugin::<Content>(sphere::SphereContent)
        .plugin::<Content>(torus::TorusContent)
        .plugin::<Content>(ellipsoid::EllipsoidContent)
        .plugin::<Content>(nurbs::NurbsContent);
}

/// `+1` for a use that runs along its edge, `−1` for one that runs against it.
fn direction(used: &Use) -> f64 {
    if used.forward {
        1.0
    } else {
        -1.0
    }
}

/// `+1` when the normal of the surface is the outward normal of the face, else `−1`.
fn side(outward: bool) -> f64 {
    if outward {
        1.0
    } else {
        -1.0
    }
}
