//! The ring torus.

use super::{arc, positive, shifted};
use crate::curve::Circle;
use krly3d_core::math::frame::{frame, origin, placed, z_axis};
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::roots::{quartic, wrap};
use krly3d_core::math::{Iso3, Point3, Sim3, Uv, Vec3};
use krly3d_core::{text, Error, Exactness};
use krly3d_kernel::ports::surface::{Stretch, Surface};
use std::any::Any;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

/// The torus about the z axis of `frame`: a circle of radius `minor` whose centre runs on
/// a circle of radius `major`. `u` is the angle about the axis, `v` the angle around the
/// tube, measured from the outer equator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Torus {
    frame: Iso3,
    major: f64,
    minor: f64,
}

impl Torus {
    pub fn new(frame: Iso3, major: f64, minor: f64) -> Result<Self, Error> {
        let (major, minor) = (positive("torus major radius", major)?, positive("torus minor radius", minor)?);
        if minor < major {
            Ok(Self { frame, major, minor })
        } else {
            Err(Error::input(format!(
                "a ring torus needs its tube radius {minor} below its ring radius {major}"
            )))
        }
    }

    pub fn frame(&self) -> &Iso3 {
        &self.frame
    }

    pub fn major(&self) -> f64 {
        self.major
    }

    pub fn minor(&self) -> f64 {
        self.minor
    }
}

impl Surface for Torus {
    fn classes(&self) -> &'static [&'static str] {
        &["torus", "surface"]
    }

    fn point(&self, uv: Uv) -> Point3 {
        let ring = self.major + self.minor * uv.y.cos();
        self.frame * Point3::new(ring * uv.x.cos(), ring * uv.x.sin(), self.minor * uv.y.sin())
    }

    fn normal(&self, uv: Uv) -> Vec3 {
        self.frame * Vec3::new(uv.y.cos() * uv.x.cos(), uv.y.cos() * uv.x.sin(), uv.y.sin())
    }

    fn project(&self, point: &Point3) -> Uv {
        let local = self.frame.inverse_transform_point(point);
        Uv::new(local.y.atan2(local.x), local.z.atan2(local.x.hypot(local.y) - self.major))
    }

    fn line(&self, origin: &Point3, direction: &Vec3, hits: &mut Vec<f64>) -> Exactness {
        // (|p|² + R² − r²)² = 4R²(x² + y²) along p = o + t·d is a quartic in t.
        //
        // It is set up from the point of the line nearest the centre, `t₀` along it, and its
        // roots are moved back by `t₀`. From a far origin the coefficients would carry |o|⁴
        // and round away roots that lie close together on a thin tube; from the nearest point
        // they are of the size of the torus. The equation is the same either way.
        let far = self.frame.inverse_transform_point(origin).coords;
        let d = self.frame.inverse_transform_vector(direction);
        let g = d.norm_squared();
        if g == 0.0 {
            return Exactness::Exact;
        }
        let nearest = -far.dot(&d) / g;
        let o = far + d * nearest;
        let h = 2.0 * o.dot(&d);
        let i = o.norm_squared() + self.major * self.major - self.minor * self.minor;
        let ring = 4.0 * self.major * self.major;
        let coefficients = [
            i * i - ring * (o.x * o.x + o.y * o.y),
            2.0 * h * i - 2.0 * ring * (o.x * d.x + o.y * d.y),
            h * h + 2.0 * g * i - ring * (d.x * d.x + d.y * d.y),
            2.0 * g * h,
            g * g,
        ];
        hits.extend(quartic(coefficients).as_slice().iter().map(|t| t + nearest));
        Exactness::Exact
    }

    fn periods(&self) -> [Option<f64>; 2] {
        [Some(TAU), Some(TAU)]
    }

    fn walk(&self, from: &Point3, to: &Point3) -> Vec<Stretch> {
        // Around the tube to the target's angle on it, then around the axis.
        let (start, end) = (self.project(from), self.project(to));
        let (center, axis) = (origin(&self.frame), z_axis(&self.frame));
        let outward = self.frame * Vec3::new(start.x.cos(), start.x.sin(), 0.0);
        let tube = frame(&(center + outward * self.major), &outward.cross(&axis), &outward)
            .and_then(|placed| Circle::new(placed, self.minor));
        let ring_frame = shifted(&self.frame, center + axis * (self.minor * end.y.sin()));
        let ring = Circle::new(ring_frame, self.major + self.minor * end.y.cos());
        arc(tube, start.y, wrap(end.y - start.y))
            .into_iter()
            .chain(arc(ring, start.x, wrap(end.x - start.x)))
            .collect()
    }

    fn extremes(&self, direction: &Vec3) -> Vec<Point3> {
        let local = self.frame.inverse_transform_vector(direction);
        let flat = local.x.hypot(local.y);
        if flat == 0.0 {
            // Along the axis the whole top and bottom circles reach equally far; one point of each stands for it.
            return vec![self.point(Uv::new(0.0, FRAC_PI_2)), self.point(Uv::new(0.0, -FRAC_PI_2))];
        }
        // The normal points along the direction at four places: outside and inside the ring, front and back.
        let (around, up) = (local.y.atan2(local.x), local.z.atan2(flat));
        [(around, up), (around, up + PI), (around + PI, PI - up), (around + PI, -up)]
            .map(|(u, v)| self.point(Uv::new(u, v)))
            .to_vec()
    }

    fn summary(&self) -> String {
        let axis = text::direction(&z_axis(&self.frame));
        format!("torus ring={} tube={} axis={axis}", text::num(self.major), text::num(self.minor))
    }

    fn euler_characteristic(&self) -> Option<i32> {
        Some(0)
    }

    fn transformed(&self, by: &Sim3) -> Arc<dyn Surface> {
        Arc::new(Self {
            frame: placed(&self.frame, by),
            major: self.major * by.scaling(),
            minor: self.minor * by.scaling(),
        })
    }

    fn mirrored(&self, mirror: &Mirror) -> Option<Arc<dyn Surface>> {
        Some(Arc::new(Self {
            frame: mirror.shape_frame(&self.frame).ok()?,
            major: self.major,
            minor: self.minor,
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Torus;
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Iso3, Point3, Uv, Vec3};
    use krly3d_core::{Accuracy, ErrorKind};
    use krly3d_kernel::ports::surface::{contract, Surface};

    #[test]
    fn satisfies_the_surface_contract() {
        let placed = frame(&Point3::new(0.5, 1.5, -2.0), &Vec3::new(-0.4, 0.3, 1.0), &Vec3::x()).unwrap();
        let torus = Torus::new(placed, 6.0, 2.0).unwrap();
        let samples = [Uv::new(0.0, 0.0), Uv::new(2.2, 1.0), Uv::new(-1.3, 2.9), Uv::new(3.0, -2.0)];

        contract::check(&torus, &samples, 0.5, &Accuracy::for_scale(10.0).unwrap()).unwrap();
    }

    #[test]
    fn a_line_through_the_hole_in_the_ring_plane_hits_the_tube_four_times() {
        let torus = Torus::new(Iso3::identity(), 6.0, 2.0).unwrap();
        let mut hits = Vec::new();

        torus.line(&Point3::new(-20.0, 0.0, 0.0), &Vec3::x(), &mut hits);

        let expected = [12.0, 16.0, 24.0, 28.0];
        assert!(
            hits.len() == 4 && hits.iter().zip(expected).all(|(hit, e)| (hit - e).abs() < 1e-11),
            "{hits:?}"
        );
    }

    #[test]
    fn a_line_from_far_away_hits_a_thin_tube_where_the_closed_form_says() {
        // A picture of a small ring taken along its axis from far away: every ray parallel to
        // the axis, from 80 units off, crossing the tube at ρ from the axis. The tube is met
        // where (ρ − R)² + z² = r², at z = ±√(r² − (ρ − R)²).
        let torus = Torus::new(Iso3::identity(), 1.3, 0.25).unwrap();
        for step in 0..=40 {
            let distance = 1.06 + 0.012 * f64::from(step);
            let mut hits = Vec::new();

            torus.line(&Point3::new(distance, 0.0, -80.0), &Vec3::z(), &mut hits);

            let half = (0.0625 - (distance - 1.3).powi(2)).sqrt();
            let expected = [80.0 - half, 80.0 + half];
            assert!(
                hits.len() == 2 && hits.iter().zip(expected).all(|(hit, e)| (hit - e).abs() < 1e-9),
                "ρ={distance}: {hits:?} instead of {expected:?}"
            );
        }
    }

    #[test]
    fn a_line_along_the_axis_passes_through_the_hole() {
        let torus = Torus::new(Iso3::identity(), 6.0, 2.0).unwrap();
        let mut hits = Vec::new();

        torus.line(&Point3::new(0.0, 0.0, -9.0), &Vec3::z(), &mut hits);

        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn a_tube_as_wide_as_the_ring_is_rejected() {
        let error = Torus::new(Iso3::identity(), 2.0, 2.0).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Input);
    }
}
