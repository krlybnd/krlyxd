//! The circular cylinder.

use super::{arc, positive, shifted, straight};
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
use std::f64::consts::TAU;
use std::sync::Arc;

/// The cylinder about the z axis of `frame`; `u` is the angle about the axis and `v` the
/// height along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cylinder {
    frame: Iso3,
    radius: f64,
}

impl Cylinder {
    pub fn new(frame: Iso3, radius: f64) -> Result<Self, Error> {
        Ok(Self { frame, radius: positive("cylinder radius", radius)? })
    }

    pub fn frame(&self) -> &Iso3 {
        &self.frame
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }
}

impl Surface for Cylinder {
    fn classes(&self) -> &'static [&'static str] {
        &["cylinder", "quadric", "surface"]
    }

    fn point(&self, uv: Uv) -> Point3 {
        self.frame * Point3::new(self.radius * uv.x.cos(), self.radius * uv.x.sin(), uv.y)
    }

    fn normal(&self, uv: Uv) -> Vec3 {
        self.frame * Vec3::new(uv.x.cos(), uv.x.sin(), 0.0)
    }

    fn project(&self, point: &Point3) -> Uv {
        let local = self.frame.inverse_transform_point(point);
        Uv::new(local.y.atan2(local.x), local.z)
    }

    fn line(&self, origin: &Point3, direction: &Vec3, hits: &mut Vec<f64>) -> Exactness {
        let o = self.frame.inverse_transform_point(origin).xy().coords;
        let d = self.frame.inverse_transform_vector(direction).xy();
        hits.extend_from_slice(
            quadratic(d.norm_squared(), 2.0 * o.dot(&d), o.norm_squared() - self.radius * self.radius)
                .as_slice(),
        );
        Exactness::Exact
    }

    fn quadric(&self) -> Option<Quadric> {
        let local = Mat4::from_diagonal(&Vector4::new(1.0, 1.0, 0.0, -self.radius * self.radius));
        Some(Quadric::in_frame(local, &self.frame))
    }

    fn periods(&self) -> [Option<f64>; 2] {
        [Some(TAU), None]
    }

    fn walk(&self, from: &Point3, to: &Point3) -> Vec<Stretch> {
        // Along the axis to the height of the target, then around at that height.
        let (start, end) = (self.project(from), self.project(to));
        let (base, axis) = (origin(&self.frame), z_axis(&self.frame));
        let level = from + axis * (end.y - start.y);
        let around = Circle::new(shifted(&self.frame, base + axis * end.y), self.radius);
        straight(*from, level).into_iter().chain(arc(around, start.x, wrap(end.x - start.x))).collect()
    }

    fn extremes(&self, _: &Vec3) -> Vec<Point3> {
        // Straight lines run along the surface, so a face reaches farthest on its boundary.
        Vec::new()
    }

    fn summary(&self) -> String {
        format!("cylinder r={} axis={}", text::num(self.radius), text::direction(&z_axis(&self.frame)))
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
    use super::Cylinder;
    use krly3d_core::math::frame::frame;
    use krly3d_core::math::{Iso3, Point3, Uv, Vec3};
    use krly3d_core::Accuracy;
    use krly3d_kernel::ports::surface::{contract, Surface};

    #[test]
    fn satisfies_the_surface_contract() {
        let placed = frame(&Point3::new(-1.0, 4.0, 2.0), &Vec3::new(1.0, 0.5, 2.0), &Vec3::y()).unwrap();
        let cylinder = Cylinder::new(placed, 3.0).unwrap();
        let samples = [Uv::new(0.0, 0.0), Uv::new(1.9, 4.0), Uv::new(-2.8, -6.5)];

        contract::check(&cylinder, &samples, 1.0, &Accuracy::for_scale(10.0).unwrap()).unwrap();
    }

    #[test]
    fn a_line_across_the_axis_hits_one_radius_before_and_after_it() {
        let cylinder = Cylinder::new(Iso3::identity(), 3.0).unwrap();
        let mut hits = Vec::new();

        cylinder.line(&Point3::new(0.0, -10.0, 7.0), &Vec3::y(), &mut hits);

        assert_eq!(hits, [7.0, 13.0]);
    }

    #[test]
    fn a_line_along_the_axis_direction_never_hits() {
        let cylinder = Cylinder::new(Iso3::identity(), 3.0).unwrap();
        let mut hits = Vec::new();

        cylinder.line(&Point3::new(1.0, 0.0, 0.0), &Vec3::z(), &mut hits);

        assert!(hits.is_empty());
    }
}
