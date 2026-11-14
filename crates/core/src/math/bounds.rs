//! Axis-aligned boxes.

use super::{Point3, Vec3};

/// The smallest box with sides along the world axes around some points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub low: Point3,
    pub high: Point3,
}

impl Bounds {
    /// The box around nothing: including a point makes it that point.
    pub fn empty() -> Self {
        Self {
            low: Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            high: Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.low.x > self.high.x
    }

    pub fn of(points: impl IntoIterator<Item = Point3>) -> Self {
        points.into_iter().fold(Self::empty(), |bounds, point| bounds.with(&point))
    }

    pub fn with(self, point: &Point3) -> Self {
        Self { low: self.low.inf(point), high: self.high.sup(point) }
    }

    pub fn union(self, other: &Self) -> Self {
        Self { low: self.low.inf(&other.low), high: self.high.sup(&other.high) }
    }

    /// The box widened by `margin` on every side.
    pub fn grown(self, margin: f64) -> Self {
        let margin = Vec3::new(margin, margin, margin);
        Self { low: self.low - margin, high: self.high + margin }
    }

    pub fn center(&self) -> Point3 {
        nalgebra::center(&self.low, &self.high)
    }

    pub fn size(&self) -> Vec3 {
        self.high - self.low
    }

    pub fn contains(&self, point: &Point3) -> bool {
        (0..3).all(|axis| point[axis] >= self.low[axis] && point[axis] <= self.high[axis])
    }

    /// The stretch of parameters `t` for which `origin + t·direction` is inside the box.
    pub fn line(&self, origin: &Point3, direction: &Vec3) -> Option<(f64, f64)> {
        let (mut enter, mut leave) = (f64::NEG_INFINITY, f64::INFINITY);
        for axis in 0..3 {
            if direction[axis] == 0.0 {
                if origin[axis] < self.low[axis] || origin[axis] > self.high[axis] {
                    return None;
                }
            } else {
                let (a, b) = (
                    (self.low[axis] - origin[axis]) / direction[axis],
                    (self.high[axis] - origin[axis]) / direction[axis],
                );
                enter = enter.max(a.min(b));
                leave = leave.min(a.max(b));
            }
        }
        (enter <= leave).then_some((enter, leave))
    }
}

#[cfg(test)]
mod tests {
    use super::Bounds;
    use crate::math::{Point3, Vec3};

    fn unit() -> Bounds {
        Bounds::of([Point3::origin(), Point3::new(1.0, 1.0, 1.0)])
    }

    #[test]
    fn a_box_around_points_reaches_from_their_lowest_to_their_highest_coordinates() {
        let bounds = Bounds::of([Point3::new(1.0, 5.0, -2.0), Point3::new(-3.0, 2.0, 4.0)]);

        assert_eq!((bounds.low, bounds.high), (Point3::new(-3.0, 2.0, -2.0), Point3::new(1.0, 5.0, 4.0)));
        assert!(Bounds::empty().is_empty() && !bounds.is_empty());
    }

    #[test]
    fn a_line_through_a_box_is_inside_between_two_parameters() {
        let inside = unit().line(&Point3::new(-1.0, 0.5, 0.5), &Vec3::new(2.0, 0.0, 0.0));

        assert_eq!(inside, Some((0.5, 1.0)));
    }

    #[test]
    fn a_line_beside_a_box_is_never_inside() {
        assert_eq!(unit().line(&Point3::new(-1.0, 2.0, 0.5), &Vec3::x()), None);
        assert_eq!(unit().line(&Point3::new(-1.0, 3.0, 0.5), &Vec3::new(1.0, -0.5, 0.0)), None);
    }
}
