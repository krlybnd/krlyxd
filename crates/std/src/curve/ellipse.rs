//! The ellipse: `centre + a·cos t·x + b·sin t·y` in the xy plane of its frame.

use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::frame::{origin, placed, x_axis, y_axis, z_axis};
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::quadrature;
use krly3d_core::math::roots::harmonic;
use krly3d_core::math::{Iso3, Point3, Sim3, Vec3};
use krly3d_core::{text, Error};
use krly3d_kernel::ports::curve::Curve;
use std::any::Any;
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ellipse {
    frame: Iso3,
    /// Semi-axes along the frame's x and y axes.
    a: f64,
    b: f64,
}

impl Ellipse {
    pub fn new(frame: Iso3, a: f64, b: f64) -> Result<Self, Error> {
        if [a, b].iter().all(|axis| axis.is_finite() && *axis > 0.0) {
            Ok(Self { frame, a, b })
        } else {
            Err(Error::input(format!("an ellipse needs two positive semi-axes, got {a} and {b}")))
        }
    }

    pub fn frame(&self) -> &Iso3 {
        &self.frame
    }

    pub fn semi_axes(&self) -> (f64, f64) {
        (self.a, self.b)
    }

    pub fn center(&self) -> Point3 {
        origin(&self.frame)
    }

    pub fn axis(&self) -> Vec3 {
        z_axis(&self.frame)
    }
}

/// Pieces a quarter turn is split into for the length. The speed `√(a²sin²t + b²cos²t)`
/// is smooth, so eight Gauss points per piece leave an error far below round-off for any
/// ellipse whose axes differ by less than a factor of a hundred.
const PIECES_PER_QUARTER: usize = 16;

impl Curve for Ellipse {
    fn classes(&self) -> &'static [&'static str] {
        &["ellipse", "conic", "curve"]
    }

    fn point(&self, t: f64) -> Point3 {
        self.frame * Point3::new(self.a * t.cos(), self.b * t.sin(), 0.0)
    }

    fn tangent(&self, t: f64) -> Vec3 {
        self.frame * Vec3::new(-self.a * t.sin(), self.b * t.cos(), 0.0)
    }

    fn project(&self, point: &Point3) -> f64 {
        // The foot point makes (p − e(t)) square to e'(t):
        // (b² − a²)/2·sin 2t + a·x·sin t − b·y·cos t = 0, a trigonometric equation of
        // degree two, solved in closed form; the nearest of its roots is the foot.
        let local = self.frame.inverse_transform_point(point);
        let (x, y) = (local.x, local.y);
        let roots = harmonic([0.0, -self.b * y, self.a * x, 0.0, 0.5 * (self.b * self.b - self.a * self.a)]);
        let distance = |t: f64| (self.a * t.cos() - x).hypot(self.b * t.sin() - y);
        roots
            .as_slice()
            .iter()
            .copied()
            .min_by(|p, q| distance(*p).total_cmp(&distance(*q)))
            .unwrap_or_else(|| (y / self.b).atan2(x / self.a))
    }

    fn period(&self) -> Option<f64> {
        Some(TAU)
    }

    fn vector_area(&self, from: f64, to: f64) -> Vec3 {
        // p = c + e(t): ∫ p × dp = c × (e(to) − e(from)) + a·b·axis·(to − from).
        let chord = self.point(to) - self.point(from);
        let turn = self.axis() * (self.a * self.b * (to - from));
        (self.center().coords.cross(&chord) + turn) * 0.5
    }

    fn bounds(&self, from: f64, to: f64) -> Bounds {
        // Along a world axis the ellipse is centre + a·x·cos t + b·y·sin t, which peaks
        // where tan t = b·y / (a·x).
        let (x, y) = (x_axis(&self.frame), y_axis(&self.frame));
        let (low, high) = (from.min(to), from.max(to));
        let mut bounds = Bounds::of([self.point(low), self.point(high)]);
        for axis in 0..3 {
            let peak = (self.b * y[axis]).atan2(self.a * x[axis]);
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
        // An elliptic integral of the second kind, by Gauss–Legendre quadrature.
        let (low, high) = (from.min(to), from.max(to));
        let pieces = (((high - low) / (0.5 * PI)).ceil() as usize).max(1) * PIECES_PER_QUARTER;
        let speed = |t: f64| (self.a * t.sin()).hypot(self.b * t.cos());
        quadrature::nodes(low, high, pieces).map(|(t, weight)| weight * speed(t)).sum()
    }

    fn summary(&self) -> String {
        format!(
            "ellipse a={} b={} axis={}",
            text::num(self.a),
            text::num(self.b),
            text::direction(&self.axis())
        )
    }

    fn transformed(&self, by: &Sim3) -> Arc<dyn Curve> {
        Arc::new(Self { frame: placed(&self.frame, by), a: self.a * by.scaling(), b: self.b * by.scaling() })
    }

    fn mirrored(&self, mirror: &Mirror) -> Option<Arc<dyn Curve>> {
        Some(Arc::new(Self { frame: mirror.path_frame(&self.frame).ok()?, ..*self }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Ellipse;
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Iso3, Point3, Vec3};
    use krly3d_core::Accuracy;
    use krly3d_kernel::ports::curve::{contract, Curve};
    use std::f64::consts::{PI, TAU};

    #[test]
    fn satisfies_the_curve_contract() {
        let placed = frame(&Point3::new(2.0, 1.0, -3.0), &Vec3::new(1.0, 1.0, 2.0), &Vec3::x()).unwrap();
        let ellipse = Ellipse::new(placed, 5.0, 2.0).unwrap();

        contract::check(&ellipse, &[-2.5, 0.0, 0.8, 3.0, 5.9], &Accuracy::for_scale(10.0).unwrap()).unwrap();
    }

    #[test]
    fn the_foot_of_a_point_inside_or_outside_is_the_nearest_point() {
        let ellipse = Ellipse::new(Iso3::identity(), 5.0, 2.0).unwrap();
        for point in [Point3::new(1.0, 0.5, 0.0), Point3::new(-7.0, 3.0, 1.0), Point3::new(0.2, -0.1, 0.0)] {
            let foot = ellipse.point(ellipse.project(&point));
            let nearest = (0..20_000)
                .map(|step| ellipse.point(TAU * f64::from(step) / 20_000.0))
                .map(|candidate| (candidate - point).norm())
                .fold(f64::INFINITY, f64::min);
            assert!((foot - point).norm() <= nearest + 1e-9, "{point:?}");
        }
    }

    #[test]
    fn a_whole_ellipse_has_the_perimeter_of_the_series_and_the_area_pi_a_b() {
        let ellipse = Ellipse::new(Iso3::identity(), 5.0, 3.0).unwrap();
        // Perimeter by the Gauss–Kummer series in h = ((a − b)/(a + b))².
        let h: f64 = (2.0 / 8.0_f64).powi(2);
        let terms =
            [1.0, 1.0 / 4.0, 1.0 / 64.0, 1.0 / 256.0, 25.0 / 16_384.0, 49.0 / 65_536.0, 441.0 / 1_048_576.0];
        let series = PI * 8.0 * terms.iter().zip(0..).map(|(term, power)| term * h.powi(power)).sum::<f64>();

        assert!((ellipse.length(0.0, TAU) - series).abs() < 1e-9);
        assert!((ellipse.vector_area(0.0, TAU).z - 15.0 * PI).abs() < 1e-12);
    }
}
