//! The circular cone.

use super::{arc, shifted, straight};
use crate::curve::Circle;
use krly3d_core::math::frame::{origin, placed, z_axis};
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::quadric::Quadric;
use krly3d_core::math::roots::{quadratic, wrap};
use krly3d_core::math::{Iso3, Mat4, Point3, Sim3, Uv, Vec3};
use krly3d_core::{text, Error, Exactness};
use krly3d_kernel::ports::surface::{Stretch, Surface};
use nalgebra::Vector4;
use std::any::Any;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

/// The double cone with its apex at the origin of `frame` and the frame's z axis as axis.
/// `u` is the angle about the axis; `v` is the distance from the apex along the surface,
/// positive on the side of the z axis and negative on the opposite nappe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cone {
    frame: Iso3,
    half_angle: f64,
}

impl Cone {
    /// `half_angle` is the angle between the axis and the surface, strictly between zero
    /// and a right angle.
    pub fn new(frame: Iso3, half_angle: f64) -> Result<Self, Error> {
        if half_angle > 0.0 && half_angle < FRAC_PI_2 {
            Ok(Self { frame, half_angle })
        } else {
            Err(Error::input(format!("a cone needs a half angle between 0 and π/2, got {half_angle}")))
        }
    }

    pub fn frame(&self) -> &Iso3 {
        &self.frame
    }

    pub fn half_angle(&self) -> f64 {
        self.half_angle
    }
}

impl Surface for Cone {
    fn classes(&self) -> &'static [&'static str] {
        &["cone", "quadric", "surface"]
    }

    fn point(&self, uv: Uv) -> Point3 {
        let (sin, cos) = self.half_angle.sin_cos();
        self.frame * Point3::new(uv.y * sin * uv.x.cos(), uv.y * sin * uv.x.sin(), uv.y * cos)
    }

    fn normal(&self, uv: Uv) -> Vec3 {
        // Away from the axis on either nappe; on the negative nappe the point sits on the
        // far side of the axis, hence the sign.
        let (sin, cos) = self.half_angle.sin_cos();
        self.frame * (Vec3::new(cos * uv.x.cos(), cos * uv.x.sin(), -sin) * uv.y.signum())
    }

    fn project(&self, point: &Point3) -> Uv {
        let local = self.frame.inverse_transform_point(point);
        let (sin, cos) = self.half_angle.sin_cos();
        let (around, radial) = (local.y.atan2(local.x), local.x.hypot(local.y));
        if local.z >= 0.0 {
            Uv::new(around, radial * sin + local.z * cos)
        } else {
            Uv::new(wrap(around + PI), -(radial * sin - local.z * cos))
        }
    }

    fn line(&self, origin: &Point3, direction: &Vec3, hits: &mut Vec<f64>) -> Exactness {
        let o = self.frame.inverse_transform_point(origin);
        let d = self.frame.inverse_transform_vector(direction);
        let slope = self.half_angle.tan().powi(2);
        let a = d.x * d.x + d.y * d.y - slope * d.z * d.z;
        let b = 2.0 * (o.x * d.x + o.y * d.y - slope * o.z * d.z);
        let c = o.x * o.x + o.y * o.y - slope * o.z * o.z;
        hits.extend_from_slice(quadratic(a, b, c).as_slice());
        Exactness::Exact
    }

    fn quadric(&self) -> Option<Quadric> {
        let local = Mat4::from_diagonal(&Vector4::new(1.0, 1.0, -self.half_angle.tan().powi(2), 0.0));
        Some(Quadric::in_frame(local, &self.frame))
    }

    /// The apex.
    fn poles(&self) -> Vec<f64> {
        vec![0.0]
    }

    fn periods(&self) -> [Option<f64>; 2] {
        [Some(TAU), None]
    }

    fn walk(&self, from: &Point3, to: &Point3) -> Vec<Stretch> {
        // Along the straight line through the apex to the target's distance from it, then around.
        let (start, end) = (self.project(from), self.project(to));
        let (sin, cos) = self.half_angle.sin_cos();
        let (apex, axis) = (origin(&self.frame), z_axis(&self.frame));
        let corner = self.point(Uv::new(start.x, end.y));
        let around = Circle::new(shifted(&self.frame, apex + axis * (end.y * cos)), (end.y * sin).abs());
        // On the far nappe a point sits half a turn from its angle.
        let angle = if end.y < 0.0 { start.x + PI } else { start.x };
        straight(*from, corner).into_iter().chain(arc(around, angle, wrap(end.x - start.x))).collect()
    }

    fn extremes(&self, _: &Vec3) -> Vec<Point3> {
        // Straight lines run along the surface, so only the apex can stick out of a boundary.
        vec![origin(&self.frame)]
    }

    fn summary(&self) -> String {
        let axis = text::direction(&z_axis(&self.frame));
        format!("cone half-angle={}° axis={axis}", text::num(self.half_angle.to_degrees()))
    }

    fn transformed(&self, by: &Sim3) -> Arc<dyn Surface> {
        Arc::new(Self { frame: placed(&self.frame, by), half_angle: self.half_angle })
    }

    fn mirrored(&self, mirror: &Mirror) -> Option<Arc<dyn Surface>> {
        Some(Arc::new(Self { frame: mirror.shape_frame(&self.frame).ok()?, half_angle: self.half_angle }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Cone;
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Iso3, Point3, Uv, Vec3};
    use krly3d_core::{Accuracy, ErrorKind};
    use krly3d_kernel::ports::surface::{contract, Surface};
    use std::f64::consts::FRAC_PI_4;

    #[test]
    fn satisfies_the_surface_contract_on_both_nappes() {
        let placed = frame(&Point3::new(2.0, 0.0, -1.0), &Vec3::new(0.2, -1.0, 0.4), &Vec3::z()).unwrap();
        let cone = Cone::new(placed, 0.4).unwrap();
        let samples = [Uv::new(0.3, 6.0), Uv::new(-2.0, 9.0), Uv::new(2.5, -7.0)];

        contract::check(&cone, &samples, 0.5, &Accuracy::for_scale(10.0).unwrap()).unwrap();
    }

    #[test]
    fn a_line_across_a_right_cone_hits_where_radius_equals_height() {
        let cone = Cone::new(Iso3::identity(), FRAC_PI_4).unwrap();
        let mut hits = Vec::new();

        cone.line(&Point3::new(-10.0, 0.0, 4.0), &Vec3::x(), &mut hits);

        assert!((hits[0] - 6.0).abs() < 1e-14 && (hits[1] - 14.0).abs() < 1e-14, "{hits:?}");
    }

    #[test]
    fn a_line_along_the_axis_touches_only_the_apex() {
        let cone = Cone::new(Iso3::identity(), FRAC_PI_4).unwrap();
        let mut hits = Vec::new();

        cone.line(&Point3::new(0.0, 0.0, -3.0), &Vec3::z(), &mut hits);

        assert!(hits.len() == 2 && hits.iter().all(|hit| (hit - 3.0).abs() < 1e-14), "{hits:?}");
    }

    #[test]
    fn a_flat_or_closed_cone_is_rejected() {
        let flat = Cone::new(Iso3::identity(), std::f64::consts::FRAC_PI_2).unwrap_err();
        let closed = Cone::new(Iso3::identity(), 0.0).unwrap_err();

        assert_eq!([flat.kind, closed.kind], [ErrorKind::Input; 2]);
    }
}
