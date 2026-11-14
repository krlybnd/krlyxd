//! The sphere.

use super::{arc, positive};
use crate::curve::Circle;
use krly3d_core::math::frame::{frame, frame_about, origin, placed, x_axis};
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::quadric::Quadric;
use krly3d_core::math::roots::quadratic;
use krly3d_core::math::{Iso3, Mat4, Point3, Sim3, Uv, Vec3};
use krly3d_core::{text, Error, Exactness};
use krly3d_kernel::ports::surface::{Stretch, Surface};
use nalgebra::Vector4;
use std::any::Any;
use std::f64::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

/// The sphere centred on the origin of `frame`; `u` is the longitude about the frame's
/// z axis and `v` the latitude from its xy plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sphere {
    frame: Iso3,
    radius: f64,
}

impl Sphere {
    pub fn new(frame: Iso3, radius: f64) -> Result<Self, Error> {
        Ok(Self { frame, radius: positive("sphere radius", radius)? })
    }

    pub fn frame(&self) -> &Iso3 {
        &self.frame
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    fn direction(&self, uv: Uv) -> Vec3 {
        Vec3::new(uv.y.cos() * uv.x.cos(), uv.y.cos() * uv.x.sin(), uv.y.sin())
    }
}

impl Surface for Sphere {
    fn classes(&self) -> &'static [&'static str] {
        &["sphere", "quadric", "surface"]
    }

    fn point(&self, uv: Uv) -> Point3 {
        self.frame * Point3::from(self.direction(uv) * self.radius)
    }

    fn normal(&self, uv: Uv) -> Vec3 {
        self.frame * self.direction(uv)
    }

    fn project(&self, point: &Point3) -> Uv {
        let local = self.frame.inverse_transform_point(point);
        Uv::new(local.y.atan2(local.x), local.z.atan2(local.x.hypot(local.y)))
    }

    fn line(&self, origin: &Point3, direction: &Vec3, hits: &mut Vec<f64>) -> Exactness {
        let o = self.frame.inverse_transform_point(origin).coords;
        let d = self.frame.inverse_transform_vector(direction);
        hits.extend_from_slice(
            quadratic(d.norm_squared(), 2.0 * o.dot(&d), o.norm_squared() - self.radius * self.radius)
                .as_slice(),
        );
        Exactness::Exact
    }

    fn quadric(&self) -> Option<Quadric> {
        let local = Mat4::from_diagonal(&Vector4::new(1.0, 1.0, 1.0, -self.radius * self.radius));
        Some(Quadric::in_frame(local, &self.frame))
    }

    /// The two poles, at latitude ±π/2.
    fn poles(&self) -> Vec<f64> {
        vec![-FRAC_PI_2, FRAC_PI_2]
    }

    fn periods(&self) -> [Option<f64>; 2] {
        [Some(TAU), None]
    }

    fn walk(&self, from: &Point3, to: &Point3) -> Vec<Stretch> {
        // Along the great circle through both points, the short way round.
        let center = origin(&self.frame);
        let (a, b) = (from - center, to - center);
        let mut axis = a.cross(&b);
        let turn = axis.norm().atan2(a.dot(&b));
        if axis.norm() == 0.0 {
            // The same point twice needs no walk; opposite points are joined by any great circle.
            axis = frame_about(&center, &a).map_or_else(|_| Vec3::z(), |any| x_axis(&any));
        }
        let circle = frame(&center, &axis, &a).and_then(|placed| Circle::new(placed, self.radius));
        arc(circle, 0.0, turn).into_iter().collect()
    }

    fn extremes(&self, direction: &Vec3) -> Vec<Point3> {
        let reach = direction.normalize() * self.radius;
        vec![origin(&self.frame) + reach, origin(&self.frame) - reach]
    }

    fn summary(&self) -> String {
        format!("sphere r={}", text::num(self.radius))
    }

    fn euler_characteristic(&self) -> Option<i32> {
        Some(2)
    }

    fn transformed(&self, by: &Sim3) -> Arc<dyn Surface> {
        Arc::new(Self { frame: placed(&self.frame, by), radius: self.radius * by.scaling() })
    }

    fn mirrored(&self, mirror: &Mirror) -> Option<Arc<dyn Surface>> {
        Some(Arc::new(Self { frame: mirror.shape_frame(&self.frame).ok()?, radius: self.radius }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Sphere;
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Iso3, Point3, Uv, Vec3};
    use krly3d_core::{Accuracy, ErrorKind};
    use krly3d_kernel::ports::surface::{contract, Surface};

    #[test]
    fn satisfies_the_surface_contract() {
        let placed = frame(&Point3::new(1.0, -2.0, 0.5), &Vec3::new(0.3, 1.0, -0.2), &Vec3::x()).unwrap();
        let sphere = Sphere::new(placed, 5.0).unwrap();
        let samples = [Uv::new(0.0, 0.0), Uv::new(2.0, 0.7), Uv::new(-2.5, -1.2), Uv::new(1.0, 1.5)];

        contract::check(&sphere, &samples, 1.0, &Accuracy::for_scale(10.0).unwrap()).unwrap();
    }

    #[test]
    fn a_line_through_the_centre_hits_one_radius_before_and_after_it() {
        let sphere = Sphere::new(Iso3::identity(), 5.0).unwrap();
        let mut hits = Vec::new();

        sphere.line(&Point3::new(-20.0, 0.0, 0.0), &Vec3::x(), &mut hits);

        assert_eq!(hits, [15.0, 25.0]);
    }

    #[test]
    fn the_centre_is_one_radius_inside() {
        let sphere = Sphere::new(Iso3::identity(), 5.0).unwrap();

        assert_eq!(sphere.distance(&Point3::origin()), -5.0);
    }

    #[test]
    fn a_sphere_without_radius_is_rejected() {
        let error = Sphere::new(Iso3::identity(), -1.0).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Input);
    }
}
