//! Section rules: the curves two surfaces have in common, in closed form.

pub mod cylinder_cylinder;
pub mod plane_cylinder;
pub mod plane_plane;
pub mod plane_sphere;
pub mod sphere_cylinder;
pub mod sphere_sphere;

use crate::curve::{Circle, Line};
use krly3d_core::math::frame::frame_about;
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::{Accuracy, Attempt, Error, Exactness};
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::ports::section::{Cut, Section};
use krly3d_kernel::KernelBuilder;
use std::sync::Arc;

pub fn register(builder: &mut KernelBuilder) {
    builder
        .plugin::<Section>(plane_plane::PlanePlane)
        .plugin::<Section>(plane_sphere::PlaneSphere)
        .plugin::<Section>(plane_cylinder::PlaneCylinder)
        .plugin::<Section>(sphere_sphere::SphereSphere)
        .plugin::<Section>(sphere_cylinder::SphereCylinder)
        .plugin::<Section>(cylinder_cylinder::CylinderCylinder);
}

/// How two circles in one plane lie to each other, their centres `apart`. The same
/// figure decides sphere against sphere and cylinder against parallel cylinder.
enum Lens {
    Apart,
    Same,
    Touching,
    /// Crossing at `along` from the first centre towards the second and `half` to
    /// either side of the line of centres.
    Crossing {
        along: f64,
        half: f64,
    },
}

fn lens(apart: f64, first: f64, second: f64, accuracy: &Accuracy) -> Lens {
    if accuracy.is_zero_length(apart) {
        return if accuracy.is_zero_length(first - second) { Lens::Same } else { Lens::Apart };
    }
    let (reach, gap) = (first + second - apart, apart - (first - second).abs());
    if accuracy.is_zero_length(reach) || accuracy.is_zero_length(gap) {
        return Lens::Touching;
    }
    if reach < 0.0 || gap < 0.0 {
        return Lens::Apart;
    }
    let along = (apart * apart + first * first - second * second) / (2.0 * apart);
    Lens::Crossing { along, half: (first * first - along * along).max(0.0).sqrt() }
}

/// The answer of a rule whose surfaces touch along curves without crossing.
fn touching(curves: Result<Vec<Arc<dyn Curve>>, Error>) -> Attempt<Cut> {
    curves.map(|curves| Cut::Touching { curves, exact: Exactness::Exact }).into()
}

fn curves(curves: Result<Vec<Arc<dyn Curve>>, Error>) -> Attempt<Cut> {
    curves.map(|curves| Cut::Curves { curves, exact: Exactness::Exact }).into()
}

fn none() -> Attempt<Cut> {
    Attempt::Done(Cut::Curves { curves: Vec::new(), exact: Exactness::Exact })
}

fn line(point: Point3, direction: Vec3) -> Result<Arc<dyn Curve>, Error> {
    Line::new(point, direction).map(|line| Arc::new(line) as Arc<dyn Curve>)
}

fn circle(center: Point3, axis: Vec3, radius: f64) -> Result<Arc<dyn Curve>, Error> {
    let placed = frame_about(&center, &axis)?;
    Circle::new(placed, radius).map(|circle| Arc::new(circle) as Arc<dyn Curve>)
}

#[cfg(test)]
pub(crate) mod testing {
    use crate::standard;
    use krly3d_core::{Accuracy, Attempt};
    use krly3d_kernel::ports::section::{contract, section, Cut, SectionRequest};
    use krly3d_kernel::ports::surface::Surface;
    use std::sync::Arc;

    /// Asks the standard registry and, on success, checks the section contract.
    pub(crate) fn cut(first: impl Surface, second: impl Surface) -> Attempt<Cut> {
        let mut builder = standard();
        builder.accuracy(Accuracy::for_scale(10.0).unwrap());
        let cx = builder.context();
        let request = SectionRequest { first: Arc::new(first), second: Arc::new(second) };
        let attempt = section(&cx, &request);
        if let Attempt::Done(found) = &attempt {
            contract::check(found, &request, &[-3.0, 0.0, 0.7, 2.5], cx.accuracy()).unwrap();
        }
        attempt
    }

    /// The summaries of the curves of a cut, sorted.
    pub(crate) fn summaries(attempt: Attempt<Cut>) -> Vec<String> {
        match attempt {
            Attempt::Done(Cut::Curves { curves, .. }) => {
                let mut texts: Vec<String> = curves.iter().map(|curve| curve.summary()).collect();
                texts.sort();
                texts
            }
            Attempt::Done(Cut::Touching { curves, .. }) => {
                let mut texts: Vec<String> =
                    curves.iter().map(|curve| format!("touching {}", curve.summary())).collect();
                texts.sort();
                texts
            }
            other => panic!("expected curves, got {other:?}"),
        }
    }
}
