//! Quadric surfaces as symmetric 4×4 matrices.

use super::{Iso3, Mat4, Point3, Vec3};

/// The surface `xᵀ·Q·x = 0` in homogeneous coordinates.
///
/// Plane, sphere, cylinder and cone share this one form, so anything written against it
/// (lines, conics, pencils of two quadrics) covers all of them at once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quadric {
    matrix: Mat4,
}

impl Quadric {
    /// From the matrix of the quadratic form; only its symmetric part matters and is kept.
    pub fn new(matrix: Mat4) -> Self {
        Self { matrix: (matrix + matrix.transpose()) * 0.5 }
    }

    /// The quadric whose equation in the local coordinates of `frame` has the matrix `local`.
    pub fn in_frame(local: Mat4, frame: &Iso3) -> Self {
        let to_local = frame.inverse().to_homogeneous();
        Self::new(to_local.transpose() * local * to_local)
    }

    pub fn matrix(&self) -> &Mat4 {
        &self.matrix
    }

    /// Value of the quadratic form; zero on the surface.
    pub fn value(&self, point: &Point3) -> f64 {
        let x = point.to_homogeneous();
        x.dot(&(self.matrix * x))
    }

    /// Gradient of `value`; normal to the surface at a point of the surface.
    pub fn gradient(&self, point: &Point3) -> Vec3 {
        (self.matrix * point.to_homogeneous()).xyz() * 2.0
    }

    /// Ascending coefficients `[c0, c1, c2]` of the value along `origin + t·direction`.
    pub fn along_line(&self, origin: &Point3, direction: &Vec3) -> [f64; 3] {
        let o = origin.to_homogeneous();
        let d = direction.to_homogeneous();
        let qd = self.matrix * d;
        [o.dot(&(self.matrix * o)), 2.0 * o.dot(&qd), d.dot(&qd)]
    }

    /// Harmonic coefficients `[a0, a1, b1, a2, b2]` of the value along
    /// `center + cos θ·u + sin θ·v`, i.e. `a0 + a1·cos θ + b1·sin θ + a2·cos 2θ + b2·sin 2θ`.
    pub fn along_conic(&self, center: &Point3, u: &Vec3, v: &Vec3) -> [f64; 5] {
        let c = center.to_homogeneous();
        let (u, v) = (u.to_homogeneous(), v.to_homogeneous());
        let (qu, qv) = (self.matrix * u, self.matrix * v);
        let (uu, vv, uv) = (u.dot(&qu), v.dot(&qv), u.dot(&qv));
        [c.dot(&(self.matrix * c)) + 0.5 * (uu + vv), 2.0 * c.dot(&qu), 2.0 * c.dot(&qv), 0.5 * (uu - vv), uv]
    }
}

#[cfg(test)]
mod tests {
    use super::Quadric;
    use crate::math::frame::frame;
    use crate::math::{Mat4, Point3, Vec3};
    use nalgebra::Vector4;

    fn sphere(center: Point3, radius: f64) -> Quadric {
        let placed = frame(&center, &Vec3::new(1.0, 2.0, -1.0), &Vec3::new(0.0, 1.0, 3.0)).unwrap();
        Quadric::in_frame(Mat4::from_diagonal(&Vector4::new(1.0, 1.0, 1.0, -radius * radius)), &placed)
    }

    #[test]
    fn a_placed_sphere_measures_squared_distance_minus_squared_radius() {
        let center = Point3::new(1.0, 2.0, 3.0);
        let quadric = sphere(center, 2.0);
        let point = Point3::new(4.0, -1.0, 0.5);

        let value = quadric.value(&point);

        assert!((value - ((point - center).norm_squared() - 4.0)).abs() < 1e-13);
    }

    #[test]
    fn the_gradient_of_a_sphere_points_away_from_its_centre() {
        let center = Point3::new(1.0, 2.0, 3.0);
        let quadric = sphere(center, 2.0);
        let point = Point3::new(4.0, -1.0, 0.5);

        let gradient = quadric.gradient(&point);

        assert!((gradient - (point - center) * 2.0).norm() < 1e-13);
    }

    #[test]
    fn a_quadric_placed_by_a_frame_takes_its_local_value_at_the_moved_point() {
        let placed = frame(&Point3::new(5.0, -2.0, 1.0), &Vec3::new(1.0, 1.0, 0.0), &Vec3::z()).unwrap();
        let local = Mat4::from_diagonal(&Vector4::new(1.0, 1.0, -0.25, 0.0));
        let local_point = Point3::new(0.3, -1.2, 2.5);

        let world_value = Quadric::in_frame(local, &placed).value(&(placed * local_point));

        assert!((world_value - Quadric::new(local).value(&local_point)).abs() < 1e-13);
    }

    #[test]
    fn coefficients_along_a_line_reproduce_the_value_at_every_parameter() {
        let quadric = sphere(Point3::new(1.0, 2.0, 3.0), 2.0);
        let (origin, direction) = (Point3::new(-3.0, 0.5, 1.0), Vec3::new(2.0, 0.3, -0.7));

        let [c0, c1, c2] = quadric.along_line(&origin, &direction);

        for t in [-1.5, 0.0, 0.4, 3.0] {
            let direct = quadric.value(&(origin + direction * t));
            assert!((c0 + c1 * t + c2 * t * t - direct).abs() < 1e-12, "t = {t}");
        }
    }

    #[test]
    fn coefficients_along_a_conic_reproduce_the_value_at_every_angle() {
        let quadric = sphere(Point3::new(1.0, 2.0, 3.0), 2.0);
        let (center, u, v) =
            (Point3::new(0.5, 1.0, 2.0), Vec3::new(1.5, 0.0, 0.5), Vec3::new(0.2, 0.9, -0.6));

        let [a0, a1, b1, a2, b2] = quadric.along_conic(&center, &u, &v);

        for angle in [-2.0_f64, 0.0, 0.7, 1.9, 3.1] {
            let direct = quadric.value(&(center + u * angle.cos() + v * angle.sin()));
            let harmonic = a0
                + a1 * angle.cos()
                + b1 * angle.sin()
                + a2 * (2.0 * angle).cos()
                + b2 * (2.0 * angle).sin();
            assert!((harmonic - direct).abs() < 1e-12, "angle = {angle}");
        }
    }
}
