//! The ellipsoid `x²/a² + y²/b² + z²/c² = 1` in its frame; `u` is the longitude about the
//! frame's z axis and `v` the latitude, as on a sphere stretched along the axes.

use super::positive;
use crate::curve::Ellipse;
use krly3d_core::math::frame::{frame, origin, placed};
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::quadric::Quadric;
use krly3d_core::math::roots::{quadratic, wrap};
use krly3d_core::math::{Iso3, Mat4, Point3, Sim3, Uv, Vec3};
use krly3d_core::{text, Error, Exactness};
use krly3d_kernel::ports::surface::{Stretch, Surface};
use nalgebra::Vector4;
use std::any::Any;
use std::f64::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ellipsoid {
    frame: Iso3,
    /// Semi-axes along the frame's x, y and z axes.
    axes: [f64; 3],
}

impl Ellipsoid {
    pub fn new(frame: Iso3, a: f64, b: f64, c: f64) -> Result<Self, Error> {
        Ok(Self {
            frame,
            axes: [
                positive("ellipsoid semi-axis a", a)?,
                positive("ellipsoid semi-axis b", b)?,
                positive("ellipsoid semi-axis c", c)?,
            ],
        })
    }

    pub fn frame(&self) -> &Iso3 {
        &self.frame
    }

    pub fn axes(&self) -> [f64; 3] {
        self.axes
    }

    fn local_point(&self, uv: Uv) -> Vec3 {
        let [a, b, c] = self.axes;
        Vec3::new(a * uv.y.cos() * uv.x.cos(), b * uv.y.cos() * uv.x.sin(), c * uv.y.sin())
    }

    fn parameters(&self, local: &Vec3) -> Uv {
        let [a, b, c] = self.axes;
        let (x, y, z) = (local.x / a, local.y / b, local.z / c);
        Uv::new(y.atan2(x), z.atan2(x.hypot(y)))
    }
}

impl Surface for Ellipsoid {
    fn classes(&self) -> &'static [&'static str] {
        &["ellipsoid", "quadric", "surface"]
    }

    fn point(&self, uv: Uv) -> Point3 {
        self.frame * Point3::from(self.local_point(uv))
    }

    fn normal(&self, uv: Uv) -> Vec3 {
        let local = self.local_point(uv);
        let [a, b, c] = self.axes;
        self.frame * Vec3::new(local.x / (a * a), local.y / (b * b), local.z / (c * c)).normalize()
    }

    fn project(&self, point: &Point3) -> Uv {
        let local = self.frame.inverse_transform_point(point).coords;
        self.parameters(&Vec3::from(nearest(self.axes, local.into())))
    }

    fn line(&self, origin: &Point3, direction: &Vec3, hits: &mut Vec<f64>) -> Exactness {
        // Scaled by the semi-axes the ellipsoid is the unit sphere and the line a line.
        let [a, b, c] = self.axes;
        let scale = |v: Vec3| Vec3::new(v.x / a, v.y / b, v.z / c);
        let o = scale(self.frame.inverse_transform_point(origin).coords);
        let d = scale(self.frame.inverse_transform_vector(direction));
        hits.extend_from_slice(
            quadratic(d.norm_squared(), 2.0 * o.dot(&d), o.norm_squared() - 1.0).as_slice(),
        );
        Exactness::Exact
    }

    fn quadric(&self) -> Option<Quadric> {
        let [a, b, c] = self.axes;
        let local = Mat4::from_diagonal(&Vector4::new(1.0 / (a * a), 1.0 / (b * b), 1.0 / (c * c), -1.0));
        Some(Quadric::in_frame(local, &self.frame))
    }

    /// The two poles on the frame's z axis, at latitude ±π/2.
    fn poles(&self) -> Vec<f64> {
        vec![-FRAC_PI_2, FRAC_PI_2]
    }

    fn periods(&self) -> [Option<f64>; 2] {
        [Some(TAU), None]
    }

    fn euler_characteristic(&self) -> Option<i32> {
        Some(2)
    }

    fn walk(&self, from: &Point3, to: &Point3) -> Vec<Stretch> {
        // Along the meridian of the start to the latitude of the target, then along that
        // parallel; both are ellipses.
        let (start, end) = (self.project(from), self.project(to));
        let [a, b, c] = self.axes;
        let centre = origin(&self.frame);
        let up = self.frame * Vec3::z();
        let mut stretches = Vec::new();
        let across = self.frame * Vec3::new(a * start.x.cos(), b * start.x.sin(), 0.0);
        let meridian = frame(&centre, &across.cross(&up), &across)
            .and_then(|placed| Ellipse::new(placed, across.norm(), c));
        if let (Ok(meridian), true) = (meridian, end.y != start.y) {
            stretches.push(Stretch { curve: Arc::new(meridian), from: start.y, to: end.y });
        }
        let level = self.frame * Point3::new(0.0, 0.0, c * end.y.sin());
        let (along_a, along_b) = (a * end.y.cos(), b * end.y.cos());
        let turn = wrap(end.x - start.x);
        let parallel = Ellipse::new(
            Iso3::from_parts(nalgebra::Translation3::from(level.coords), self.frame.rotation),
            along_a,
            along_b,
        );
        if let (Ok(parallel), true) = (parallel, turn != 0.0) {
            stretches.push(Stretch { curve: Arc::new(parallel), from: start.x, to: start.x + turn });
        }
        stretches
    }

    fn extremes(&self, direction: &Vec3) -> Vec<Point3> {
        // The farthest point along d is A·d / √(dᵀA·d) with A = diag(a², b², c²).
        let [a, b, c] = self.axes;
        let d = self.frame.inverse_transform_vector(direction);
        let reach = Vec3::new(a * a * d.x, b * b * d.y, c * c * d.z);
        let size = reach.dot(&d).sqrt();
        if size == 0.0 {
            return Vec::new();
        }
        let tip = reach / size;
        vec![self.frame * Point3::from(tip), self.frame * Point3::from(-tip)]
    }

    fn summary(&self) -> String {
        let [a, b, c] = self.axes;
        format!(
            "ellipsoid a={} b={} c={} axis={}",
            text::num(a),
            text::num(b),
            text::num(c),
            text::direction(&(self.frame * Vec3::z()))
        )
    }

    fn transformed(&self, by: &Sim3) -> Arc<dyn Surface> {
        Arc::new(Self { frame: placed(&self.frame, by), axes: self.axes.map(|axis| axis * by.scaling()) })
    }

    fn mirrored(&self, mirror: &Mirror) -> Option<Arc<dyn Surface>> {
        // Every coordinate plane of the frame is a plane of symmetry, so the frame whose y
        // axis turned the other way carries the same ellipsoid.
        Some(Arc::new(Self { frame: mirror.shape_frame(&self.frame).ok()?, axes: self.axes }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The point of the ellipsoid with semi-axes `e` nearest to `y`, in the ellipsoid's own
/// coordinates.
///
/// The foot point is `xᵢ = rᵢ·yᵢ / (s + rᵢ)` for the root `s` of a function that falls
/// monotonically between two known bounds, found by bisection to the last bit (Eberly,
/// "Distance from a point to an ellipse, an ellipsoid, or a hyperellipsoid"). The axes are
/// sorted and the point is reflected into the first octant first; the degenerate cases of
/// a point on a coordinate plane have closed forms of their own.
fn nearest(e: [f64; 3], y: [f64; 3]) -> [f64; 3] {
    let mut order = [0, 1, 2];
    order.sort_by(|i, j| e[*j].total_cmp(&e[*i]));
    let sorted = nearest_sorted(order.map(|i| e[i]), order.map(|i| y[i].abs()));
    let mut x = [0.0; 3];
    for (k, axis) in order.iter().enumerate() {
        x[*axis] = sorted[k].copysign(y[*axis]);
    }
    x
}

fn nearest_sorted(e: [f64; 3], y: [f64; 3]) -> [f64; 3] {
    let [e0, e1, e2] = e;
    let [y0, y1, y2] = y;
    if y2 > 0.0 {
        if y1 > 0.0 && y0 > 0.0 {
            let z = [y0 / e0, y1 / e1, y2 / e2];
            let g = z[0] * z[0] + z[1] * z[1] + z[2] * z[2] - 1.0;
            if g == 0.0 {
                return y;
            }
            let r = [(e0 / e2).powi(2), (e1 / e2).powi(2), 1.0];
            let s = bisect(
                [r[0] * z[0], r[1] * z[1], z[2]],
                r,
                z[2] - 1.0,
                if g < 0.0 { 0.0 } else { (r[0] * z[0]).hypot(r[1] * z[1]).hypot(z[2]) - 1.0 },
            );
            return [r[0] * y0 / (s + r[0]), r[1] * y1 / (s + r[1]), y2 / (s + 1.0)];
        }
        if y1 > 0.0 {
            let [x1, x2] = nearest_on_ellipse([e1, e2], [y1, y2]);
            return [0.0, x1, x2];
        }
        if y0 > 0.0 {
            let [x0, x2] = nearest_on_ellipse([e0, e2], [y0, y2]);
            return [x0, 0.0, x2];
        }
        return [0.0, 0.0, e2];
    }
    let (d0, d1) = (e0 * e0 - e2 * e2, e1 * e1 - e2 * e2);
    let (n0, n1) = (e0 * y0, e1 * y1);
    if n0 < d0 && n1 < d1 {
        let (q0, q1) = (n0 / d0, n1 / d1);
        let rest = 1.0 - q0 * q0 - q1 * q1;
        if rest > 0.0 {
            return [e0 * q0, e1 * q1, e2 * rest.sqrt()];
        }
    }
    let [x0, x1] = nearest_on_ellipse([e0, e1], [y0, y1]);
    [x0, x1, 0.0]
}

/// The same for an ellipse with semi-axes `e0 ≥ e1` and a point in the first quadrant.
fn nearest_on_ellipse(e: [f64; 2], y: [f64; 2]) -> [f64; 2] {
    let [e0, e1] = e;
    let [y0, y1] = y;
    if y1 > 0.0 {
        if y0 > 0.0 {
            let z = [y0 / e0, y1 / e1];
            let g = z[0] * z[0] + z[1] * z[1] - 1.0;
            if g == 0.0 {
                return y;
            }
            let r0 = (e0 / e1).powi(2);
            let s = bisect(
                [r0 * z[0], z[1], 0.0],
                [r0, 1.0, 1.0],
                z[1] - 1.0,
                if g < 0.0 { 0.0 } else { (r0 * z[0]).hypot(z[1]) - 1.0 },
            );
            return [r0 * y0 / (s + r0), y1 / (s + 1.0)];
        }
        return [0.0, e1];
    }
    let (n0, d0) = (e0 * y0, e0 * e0 - e1 * e1);
    if n0 < d0 {
        let q = n0 / d0;
        return [e0 * q, e1 * (1.0 - q * q).sqrt()];
    }
    [e0, 0.0]
}

/// The root in `[low, high]` of `Σ (nᵢ / (s + rᵢ))² − 1`, which falls monotonically there.
/// Halving a double interval ends after at most about 1100 steps, when the midpoint can no
/// longer be told from an end.
fn bisect(n: [f64; 3], r: [f64; 3], mut low: f64, mut high: f64) -> f64 {
    let mut s = 0.5 * (low + high);
    for _ in 0..1200 {
        s = 0.5 * (low + high);
        if s == low || s == high {
            break;
        }
        let g: f64 = (0..3).map(|i| (n[i] / (s + r[i])).powi(2)).sum::<f64>() - 1.0;
        if g > 0.0 {
            low = s;
        } else if g < 0.0 {
            high = s;
        } else {
            break;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::Ellipsoid;
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Iso3, Point3, Uv, Vec3};
    use krly3d_core::Accuracy;
    use krly3d_kernel::ports::surface::{contract, Surface};

    #[test]
    fn satisfies_the_surface_contract() {
        let placed = frame(&Point3::new(0.5, 1.5, -2.0), &Vec3::new(-0.4, 0.3, 1.0), &Vec3::x()).unwrap();
        let ellipsoid = Ellipsoid::new(placed, 6.0, 3.0, 4.0).unwrap();
        let samples = [Uv::new(0.0, 0.0), Uv::new(2.2, 1.0), Uv::new(-1.3, -0.9), Uv::new(3.0, 0.4)];

        contract::check(&ellipsoid, &samples, 0.5, &Accuracy::for_scale(10.0).unwrap()).unwrap();
    }

    #[test]
    fn the_foot_of_a_point_is_the_nearest_point_of_the_ellipsoid() {
        let ellipsoid = Ellipsoid::new(Iso3::identity(), 5.0, 3.0, 2.0).unwrap();
        for point in [
            Point3::new(1.0, 0.5, 0.3),
            Point3::new(-7.0, 3.0, 1.0),
            Point3::new(0.0, 0.0, 9.0),
            Point3::new(4.0, 0.0, 0.0),
        ] {
            let foot = ellipsoid.point(ellipsoid.project(&point));
            let mut nearest = f64::INFINITY;
            for i in 0..400 {
                for j in 0..=200 {
                    let uv = Uv::new(
                        std::f64::consts::TAU * f64::from(i) / 400.0,
                        -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * f64::from(j) / 200.0,
                    );
                    nearest = nearest.min((ellipsoid.point(uv) - point).norm());
                }
            }
            assert!((foot - point).norm() <= nearest + 1e-9, "{point:?}");
        }
    }
}
