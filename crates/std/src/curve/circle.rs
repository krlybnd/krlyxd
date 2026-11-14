//! The circle.

use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::frame::{origin, placed, x_axis, y_axis, z_axis};
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::{Iso3, Point3, Sim3, Vec3};
use krly3d_core::{text, Error};
use krly3d_kernel::ports::curve::Curve;
use std::any::Any;
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

/// The circle of the given radius in the xy plane of `frame`, parameterised by the angle
/// from the frame's x axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    frame: Iso3,
    radius: f64,
}

impl Circle {
    pub fn new(frame: Iso3, radius: f64) -> Result<Self, Error> {
        if radius.is_finite() && radius > 0.0 {
            Ok(Self { frame, radius })
        } else {
            Err(Error::input(format!("a circle needs a positive radius, got {radius}")))
        }
    }

    pub fn frame(&self) -> &Iso3 {
        &self.frame
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    pub fn center(&self) -> Point3 {
        origin(&self.frame)
    }

    pub fn axis(&self) -> Vec3 {
        z_axis(&self.frame)
    }
}

impl Curve for Circle {
    fn classes(&self) -> &'static [&'static str] {
        &["circle", "conic", "curve"]
    }

    fn point(&self, t: f64) -> Point3 {
        self.frame * Point3::new(self.radius * t.cos(), self.radius * t.sin(), 0.0)
    }

    fn tangent(&self, t: f64) -> Vec3 {
        self.frame * Vec3::new(-self.radius * t.sin(), self.radius * t.cos(), 0.0)
    }

    fn project(&self, point: &Point3) -> f64 {
        let local = self.frame.inverse_transform_point(point);
        local.y.atan2(local.x)
    }

    fn period(&self) -> Option<f64> {
        Some(TAU)
    }

    fn vector_area(&self, from: f64, to: f64) -> Vec3 {
        // p = c + e(θ) with |e| = r, so ∫ p × dp = c × (e(to) − e(from)) + r²·axis·(to − from).
        let chord = self.point(to) - self.point(from);
        let turn = self.axis() * (self.radius * self.radius * (to - from));
        (self.center().coords.cross(&chord) + turn) * 0.5
    }

    fn bounds(&self, from: f64, to: f64) -> Bounds {
        // Along a world axis the circle is centre + a·cos t + b·sin t, which peaks where
        // tan t = b/a. The peaks that fall inside the stretch join its two ends.
        let (x, y) = (x_axis(&self.frame), y_axis(&self.frame));
        let (low, high) = (from.min(to), from.max(to));
        let mut bounds = Bounds::of([self.point(low), self.point(high)]);
        for axis in 0..3 {
            let peak = y[axis].atan2(x[axis]);
            for candidate in [peak, peak + PI] {
                let turned = candidate + ((low - candidate) / TAU).ceil() * TAU;
                if turned <= high {
                    bounds = bounds.with(&self.point(turned));
                }
            }
        }
        bounds
    }

    fn length(&self, from: f64, to: f64) -> f64 {
        self.radius * (to - from).abs()
    }

    fn summary(&self) -> String {
        format!("circle r={} axis={}", text::num(self.radius), text::direction(&self.axis()))
    }

    fn transformed(&self, by: &Sim3) -> Arc<dyn Curve> {
        Arc::new(Self { frame: placed(&self.frame, by), radius: self.radius * by.scaling() })
    }

    fn mirrored(&self, mirror: &Mirror) -> Option<Arc<dyn Curve>> {
        Some(Arc::new(Self { frame: mirror.path_frame(&self.frame).ok()?, radius: self.radius }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Circle;
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Iso3, Point3, Vec3};
    use krly3d_core::{Accuracy, ErrorKind};
    use krly3d_kernel::ports::curve::{contract, Curve};
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn satisfies_the_curve_contract() {
        let placed = frame(&Point3::new(2.0, 1.0, -3.0), &Vec3::new(1.0, 1.0, 2.0), &Vec3::x()).unwrap();
        let circle = Circle::new(placed, 4.0).unwrap();

        contract::check(&circle, &[-2.5, 0.0, 0.8, 3.0], &Accuracy::for_scale(10.0).unwrap()).unwrap();
    }

    #[test]
    fn a_quarter_turn_from_the_x_axis_lies_on_the_y_axis() {
        let circle = Circle::new(Iso3::identity(), 2.0).unwrap();

        let point = circle.point(FRAC_PI_2);

        assert!((point - Point3::new(0.0, 2.0, 0.0)).norm() < 1e-15);
    }

    #[test]
    fn a_circle_without_radius_is_rejected() {
        let error = Circle::new(Iso3::identity(), 0.0).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Input);
    }
}
