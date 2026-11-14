//! The plane.

use super::straight;
use krly3d_core::math::frame::{frame_about, placed, z_axis};
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::quadric::Quadric;
use krly3d_core::math::{Iso3, Mat4, Point3, Sim3, Uv, Vec3};
use krly3d_core::{text, Error, Exactness};
use krly3d_kernel::ports::surface::{Stretch, Surface};
use std::any::Any;
use std::sync::Arc;

/// The xy plane of `frame`; `u` and `v` are the coordinates along its x and y axes and the
/// normal is its z axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    frame: Iso3,
}

impl Plane {
    pub fn new(frame: Iso3) -> Self {
        Self { frame }
    }

    pub fn through(origin: &Point3, normal: &Vec3) -> Result<Self, Error> {
        frame_about(origin, normal).map(Self::new)
    }

    pub fn frame(&self) -> &Iso3 {
        &self.frame
    }

    pub fn unit_normal(&self) -> Vec3 {
        z_axis(&self.frame)
    }
}

impl Surface for Plane {
    fn classes(&self) -> &'static [&'static str] {
        &["plane", "quadric", "surface"]
    }

    fn point(&self, uv: Uv) -> Point3 {
        self.frame * Point3::new(uv.x, uv.y, 0.0)
    }

    fn normal(&self, _: Uv) -> Vec3 {
        self.unit_normal()
    }

    fn project(&self, point: &Point3) -> Uv {
        self.frame.inverse_transform_point(point).xy()
    }

    fn distance(&self, point: &Point3) -> f64 {
        self.frame.inverse_transform_point(point).z
    }

    fn line(&self, origin: &Point3, direction: &Vec3, hits: &mut Vec<f64>) -> Exactness {
        let height = self.frame.inverse_transform_point(origin).z;
        let climb = self.frame.inverse_transform_vector(direction).z;
        if climb != 0.0 {
            hits.push(-height / climb);
        }
        Exactness::Exact
    }

    fn quadric(&self) -> Option<Quadric> {
        // xᵀ·Q·x = z in local coordinates: the signed distance itself.
        let mut local = Mat4::zeros();
        local[(2, 3)] = 0.5;
        local[(3, 2)] = 0.5;
        Some(Quadric::in_frame(local, &self.frame))
    }

    fn periods(&self) -> [Option<f64>; 2] {
        [None, None]
    }

    fn walk(&self, from: &Point3, to: &Point3) -> Vec<Stretch> {
        straight(*from, *to).into_iter().collect()
    }

    fn extremes(&self, _: &Vec3) -> Vec<Point3> {
        Vec::new()
    }

    fn summary(&self) -> String {
        format!("plane n={}", text::direction(&self.unit_normal()))
    }

    fn transformed(&self, by: &Sim3) -> Arc<dyn Surface> {
        Arc::new(Self { frame: placed(&self.frame, by) })
    }

    fn mirrored(&self, mirror: &Mirror) -> Option<Arc<dyn Surface>> {
        Some(Arc::new(Self { frame: mirror.shape_frame(&self.frame).ok()? }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Plane;
    use krly3d_core::math::{Point3, Uv, Vec3};
    use krly3d_core::Accuracy;
    use krly3d_kernel::ports::surface::{contract, Surface};

    fn tilted() -> Plane {
        Plane::through(&Point3::new(1.0, 2.0, 3.0), &Vec3::new(1.0, -1.0, 2.0)).unwrap()
    }

    #[test]
    fn satisfies_the_surface_contract() {
        let samples = [Uv::new(0.0, 0.0), Uv::new(3.0, -2.0), Uv::new(-7.5, 4.0)];

        contract::check(&tilted(), &samples, 1.0, &Accuracy::for_scale(10.0).unwrap()).unwrap();
    }

    #[test]
    fn a_line_parallel_to_the_plane_does_not_hit_it() {
        let plane = Plane::through(&Point3::origin(), &Vec3::z()).unwrap();
        let mut hits = Vec::new();

        plane.line(&Point3::new(0.0, 0.0, 1.0), &Vec3::new(1.0, 2.0, 0.0), &mut hits);

        assert!(hits.is_empty());
    }

    #[test]
    fn the_distance_is_signed_by_the_normal() {
        let plane = Plane::through(&Point3::new(0.0, 0.0, 5.0), &Vec3::z()).unwrap();

        assert_eq!(plane.distance(&Point3::new(9.0, 9.0, 7.0)), 2.0);
        assert_eq!(plane.distance(&Point3::new(9.0, 9.0, 1.0)), -4.0);
    }
}
