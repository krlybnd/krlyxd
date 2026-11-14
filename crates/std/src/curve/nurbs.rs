//! A NURBS curve: the curve of a loft between sections, and its iso-curves.
//!
//! Points and derivatives are exact evaluations of the rational form. What has no closed
//! form is answered numerically and is accurate to round-off: the foot point (Newton's
//! method from the nearest of a few samples per knot span), lengths and vector areas
//! (Gauss–Legendre on every knot span), and the extremes of the box (Newton on the
//! derivative of each coordinate). A closed curve repeats with the length of its domain.

use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::nurbs;
use krly3d_core::math::quadrature;
use krly3d_core::math::{Point3, Sim3, Vec3};
use krly3d_core::Error;
use krly3d_kernel::ports::curve::Curve;
use std::any::Any;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct Nurbs {
    curve: nurbs::Curve,
    closed: bool,
}

/// Samples per knot span from which a foot point or an extreme is refined.
const SAMPLES: usize = 24;
/// Newton steps for a foot point or an extreme; it converges in a handful from a sample.
const NEWTON: usize = 40;
const PIECES: usize = 8;

impl Nurbs {
    pub fn new(curve: nurbs::Curve) -> Result<Self, Error> {
        let (start, end) = curve.domain();
        let gap = (curve.point(end) - curve.point(start)).norm();
        let size: f64 = curve
            .points
            .windows(2)
            .map(|pair| (pair[1].xyz() / pair[1].w - pair[0].xyz() / pair[0].w).norm())
            .sum();
        // Closed when the ends meet to the round-off of the control polygon's size.
        let closed = gap <= f64::EPSILON * 64.0 * size.max(1.0);
        Ok(Self { curve, closed })
    }

    pub fn data(&self) -> &nurbs::Curve {
        &self.curve
    }

    /// `t` taken into the domain of a closed curve; an open curve goes on beyond its ends
    /// along the polynomials of its first and last spans.
    fn inside(&self, t: f64) -> f64 {
        let (start, end) = self.curve.domain();
        if self.closed {
            start + (t - start).rem_euclid(end - start)
        } else {
            t
        }
    }

    /// The distinct knots inside `[from, to]` with both ends: the pieces on which the curve
    /// is smooth.
    fn breaks(&self, from: f64, to: f64) -> Vec<f64> {
        let mut breaks = vec![from];
        if self.closed {
            // Over more than one turn the knots repeat each turn.
            let (start, end) = self.curve.domain();
            let period = end - start;
            let first = ((from - start) / period).floor() as i64;
            let last = ((to - start) / period).ceil() as i64;
            for turn in first..=last {
                for knot in &self.curve.basis.knots {
                    let shifted = knot + period * turn as f64;
                    if shifted > from && shifted < to {
                        breaks.push(shifted);
                    }
                }
            }
        } else {
            breaks.extend(self.curve.basis.knots.iter().copied().filter(|knot| *knot > from && *knot < to));
        }
        breaks.push(to);
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        breaks
    }

    /// `∫ integrand dt` from `from` to `to`, piece by piece.
    fn integral<T: std::ops::Add<Output = T> + std::ops::Mul<f64, Output = T> + Copy>(
        &self,
        from: f64,
        to: f64,
        zero: T,
        integrand: impl Fn(f64) -> T,
    ) -> T {
        let (low, high, sign) = if from <= to { (from, to, 1.0) } else { (to, from, -1.0) };
        let mut total = zero;
        for span in self.breaks(low, high).windows(2) {
            for (t, weight) in quadrature::nodes(span[0], span[1], PIECES) {
                total = total + integrand(t) * weight;
            }
        }
        total * sign
    }

    fn derivatives(&self, t: f64, order: usize) -> Vec<Vec3> {
        self.curve.derivatives(self.inside(t), order)
    }
}

impl Curve for Nurbs {
    fn classes(&self) -> &'static [&'static str] {
        &["nurbs", "curve"]
    }

    fn point(&self, t: f64) -> Point3 {
        Point3::from(self.derivatives(t, 0)[0])
    }

    fn tangent(&self, t: f64) -> Vec3 {
        self.derivatives(t, 1)[1]
    }

    fn project(&self, point: &Point3) -> f64 {
        let (start, end) = self.curve.domain();
        let distance = |t: f64| (self.point(t) - point).norm();
        let mut best = start;
        for span in self.breaks(start, end).windows(2) {
            for sample in 0..=SAMPLES {
                let t = span[0] + (span[1] - span[0]) * sample as f64 / SAMPLES as f64;
                if distance(t) < distance(best) {
                    best = t;
                }
            }
        }
        // Newton on f(t) = C'(t)·(C(t) − p), keeping a step only while it brings the foot nearer.
        let mut t = best;
        for _ in 0..NEWTON {
            let [c, d1, d2] = [0, 1, 2].map(|k| self.derivatives(t, 2)[k]);
            let gap = c - point.coords;
            let slope = d2.dot(&gap) + d1.norm_squared();
            if slope <= 0.0 {
                break;
            }
            let (start, end) = self.curve.domain();
            let next = (t - d1.dot(&gap) / slope).clamp(start, end);
            if distance(next) > distance(t) || next == t {
                break;
            }
            t = next;
        }
        t
    }

    fn period(&self) -> Option<f64> {
        let (start, end) = self.curve.domain();
        self.closed.then_some(end - start)
    }

    fn vector_area(&self, from: f64, to: f64) -> Vec3 {
        self.integral(from, to, Vec3::zeros(), |t| {
            let ders = self.derivatives(t, 1);
            ders[0].cross(&ders[1]) * 0.5
        })
    }

    fn bounds(&self, from: f64, to: f64) -> Bounds {
        // Every sample is in the box; where a coordinate peaks between samples, the peak is
        // found by golden-section search within the two samples around it.
        let (low, high) = (from.min(to), from.max(to));
        let mut bounds = Bounds::of([self.point(low), self.point(high)]);
        for span in self.breaks(low, high).windows(2) {
            let samples: Vec<f64> = (0..=SAMPLES)
                .map(|sample| span[0] + (span[1] - span[0]) * sample as f64 / SAMPLES as f64)
                .collect();
            for t in &samples {
                bounds = bounds.with(&self.point(*t));
            }
            for axis in 0..3 {
                let value = |t: f64| self.point(t)[axis];
                for window in samples.windows(3) {
                    let (before, here, after) = (value(window[0]), value(window[1]), value(window[2]));
                    let sign = if here >= before && here >= after {
                        1.0
                    } else if here <= before && here <= after {
                        -1.0
                    } else {
                        continue;
                    };
                    let peak = golden(window[0], window[2], |t| sign * value(t));
                    bounds = bounds.with(&self.point(peak));
                }
            }
        }
        bounds
    }

    fn length(&self, from: f64, to: f64) -> f64 {
        self.integral(from.min(to), from.max(to), 0.0, |t| self.derivatives(t, 1)[1].norm())
    }

    fn summary(&self) -> String {
        format!("nurbs degree={} points={}", self.curve.basis.degree, self.curve.points.len())
    }

    fn transformed(&self, by: &Sim3) -> Arc<dyn Curve> {
        Arc::new(Self { curve: self.curve.mapped(|point| by * point), closed: self.closed })
    }

    fn mirrored(&self, mirror: &Mirror) -> Option<Arc<dyn Curve>> {
        Some(Arc::new(Self { curve: self.curve.mapped(|point| mirror.point(point)), closed: self.closed }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The argument in `[low, high]` where `f`, which has one maximum there, is largest.
/// Each step keeps 0.618 of the bracket; a hundred steps exhaust a double.
fn golden(mut low: f64, mut high: f64, f: impl Fn(f64) -> f64) -> f64 {
    const RATIO: f64 = 0.618_033_988_749_894_9;
    let (mut a, mut b) = (high - RATIO * (high - low), low + RATIO * (high - low));
    let (mut fa, mut fb) = (f(a), f(b));
    for _ in 0..100 {
        if fa < fb {
            low = a;
            a = b;
            fa = fb;
            b = low + RATIO * (high - low);
            fb = f(b);
        } else {
            high = b;
            b = a;
            fb = fa;
            a = high - RATIO * (high - low);
            fa = f(a);
        }
    }
    0.5 * (low + high)
}

#[cfg(test)]
mod tests {
    use super::Nurbs;
    use krly3d_core::math::nurbs::{self, weighted};
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::Accuracy;
    use krly3d_kernel::ports::curve::{contract, Curve};
    use std::f64::consts::{PI, TAU};

    fn wavy() -> Nurbs {
        let points = [
            [0.0, 0.0, 0.0],
            [1.0, 2.0, 0.5],
            [3.0, -1.0, 1.0],
            [4.0, 3.0, 0.0],
            [6.0, 1.0, -1.0],
            [7.0, 2.0, 0.0],
        ];
        let weights = [1.0, 0.8, 1.5, 1.0, 0.7, 1.0];
        let control =
            points.iter().zip(weights).map(|(p, w)| weighted(&Point3::new(p[0], p[1], p[2]), w)).collect();
        Nurbs::new(
            nurbs::Curve::new(3, vec![0.0, 0.0, 0.0, 0.0, 0.3, 0.7, 1.0, 1.0, 1.0, 1.0], control).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn satisfies_the_curve_contract() {
        contract::check(&wavy(), &[0.0, 0.2, 0.45, 0.8, 1.0], &Accuracy::for_scale(10.0).unwrap()).unwrap();
    }

    #[test]
    fn a_whole_circle_closes_and_has_its_closed_form_length_and_area() {
        let circle = Nurbs::new(
            nurbs::arc(&Point3::new(1.0, 1.0, 0.0), &Vec3::z(), &Point3::new(4.0, 1.0, 0.0), TAU).unwrap(),
        )
        .unwrap();

        assert_eq!(circle.period(), Some(1.0));
        contract::check(&circle, &[0.1, 0.3, 0.9, 1.4], &Accuracy::for_scale(10.0).unwrap()).unwrap();
        assert!((circle.length(0.0, 1.0) - 6.0 * PI).abs() < 1e-12);
        assert!((circle.vector_area(0.0, 1.0).z - 9.0 * PI).abs() < 1e-12);
    }
}
