//! The straight line.

use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::{Point3, Sim3, Vec3};
use krly3d_core::{text, Error};
use krly3d_kernel::ports::curve::Curve;
use std::any::Any;
use std::sync::Arc;

/// `origin + t·direction`. The direction need not be a unit vector, so a moved or scaled
/// line keeps the parameters of its points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    origin: Point3,
    direction: Vec3,
}

impl Line {
    pub fn new(origin: Point3, direction: Vec3) -> Result<Self, Error> {
        let length = direction.norm();
        if length.is_finite() && length > 0.0 && origin.coords.norm().is_finite() {
            Ok(Self { origin, direction })
        } else {
            Err(Error::input(format!(
                "a line needs a finite origin and a direction, got {origin:?} and {direction:?}"
            )))
        }
    }

    pub fn origin(&self) -> Point3 {
        self.origin
    }

    pub fn direction(&self) -> Vec3 {
        self.direction
    }
}

impl Curve for Line {
    fn classes(&self) -> &'static [&'static str] {
        &["line", "curve"]
    }

    fn point(&self, t: f64) -> Point3 {
        self.origin + self.direction * t
    }

    fn tangent(&self, _: f64) -> Vec3 {
        self.direction
    }

    fn project(&self, point: &Point3) -> f64 {
        (point - self.origin).dot(&self.direction) / self.direction.norm_squared()
    }

    fn period(&self) -> Option<f64> {
        None
    }

    fn vector_area(&self, from: f64, to: f64) -> Vec3 {
        self.point(from).coords.cross(&self.point(to).coords) * 0.5
    }

    fn bounds(&self, from: f64, to: f64) -> Bounds {
        Bounds::of([self.point(from), self.point(to)])
    }

    fn length(&self, from: f64, to: f64) -> f64 {
        self.direction.norm() * (to - from).abs()
    }

    fn summary(&self) -> String {
        format!("line along {}", text::direction(&self.direction))
    }

    fn transformed(&self, by: &Sim3) -> Arc<dyn Curve> {
        Arc::new(Self { origin: by * self.origin, direction: by * self.direction })
    }

    fn mirrored(&self, mirror: &Mirror) -> Option<Arc<dyn Curve>> {
        Some(Arc::new(Self { origin: mirror.point(&self.origin), direction: mirror.vector(&self.direction) }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Line;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::{Accuracy, ErrorKind};
    use krly3d_kernel::ports::curve::{contract, Curve};

    #[test]
    fn satisfies_the_curve_contract() {
        let line = Line::new(Point3::new(1.0, -2.0, 0.5), Vec3::new(3.0, 1.0, -2.0)).unwrap();

        contract::check(&line, &[-4.0, 0.0, 0.3, 7.5], &Accuracy::for_scale(10.0).unwrap()).unwrap();
    }

    #[test]
    fn projecting_gives_the_parameter_of_the_foot_point() {
        let line = Line::new(Point3::new(0.0, 0.0, 1.0), Vec3::new(2.0, 0.0, 0.0)).unwrap();

        let t = line.project(&Point3::new(5.0, 9.0, -3.0));

        assert_eq!(t, 2.5);
    }

    #[test]
    fn a_line_without_direction_is_rejected() {
        let error = Line::new(Point3::origin(), Vec3::zeros()).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Input);
    }
}
