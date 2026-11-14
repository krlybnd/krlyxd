//! A triangle mesh: the reading of a shape that games, printers and screens take.
//!
//! Plain data, as a body is. The triangles of a closed mesh run counter-clockwise seen from
//! outside, so the divergence theorem gives its volume.

use krly3d_core::math::Point3;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub points: Vec<Point3>,
    pub triangles: Vec<[usize; 3]>,
}

impl Mesh {
    pub fn corners(&self, triangle: usize) -> [Point3; 3] {
        self.triangles[triangle].map(|index| self.points[index])
    }

    pub fn area(&self) -> f64 {
        (0..self.triangles.len())
            .map(|triangle| {
                let [a, b, c] = self.corners(triangle);
                0.5 * (b - a).cross(&(c - a)).norm()
            })
            .sum()
    }

    /// The enclosed volume, from the triangles alone: `Σ a·(b × c) / 6`.
    pub fn volume(&self) -> f64 {
        (0..self.triangles.len())
            .map(|triangle| {
                let [a, b, c] = self.corners(triangle);
                a.coords.dot(&b.coords.cross(&c.coords)) / 6.0
            })
            .sum()
    }

    /// The sides used by one triangle and not by another one running the other way: none
    /// on a closed, consistently turned mesh.
    pub fn open_sides(&self) -> Vec<(usize, usize)> {
        let mut count: BTreeMap<(usize, usize), i64> = BTreeMap::new();
        for triangle in &self.triangles {
            for side in 0..3 {
                let (from, to) = (triangle[side], triangle[(side + 1) % 3]);
                *count.entry((from.min(to), from.max(to))).or_insert(0) += if from < to { 1 } else { -1 };
            }
        }
        count.into_iter().filter(|(_, balance)| *balance != 0).map(|(side, _)| side).collect()
    }

    /// Adds another mesh's triangles, renumbering its points after these.
    pub fn append(&mut self, other: &Mesh) {
        let base = self.points.len();
        self.points.extend(other.points.iter().copied());
        self.triangles.extend(other.triangles.iter().map(|triangle| triangle.map(|index| index + base)));
    }
}

#[cfg(test)]
mod tests {
    use super::Mesh;
    use krly3d_core::math::Point3;

    fn tetrahedron() -> Mesh {
        let points = vec![
            Point3::origin(),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
        ];
        Mesh { points, triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]] }
    }

    #[test]
    fn a_closed_tetrahedron_has_its_volume_and_no_open_side() {
        let mesh = tetrahedron();

        assert!((mesh.volume() - 1.0 / 6.0).abs() < 1e-15);
        assert!(mesh.open_sides().is_empty());
    }

    #[test]
    fn a_missing_triangle_leaves_its_three_sides_open() {
        let mut mesh = tetrahedron();
        mesh.triangles.pop();

        assert_eq!(mesh.open_sides().len(), 3);
    }
}
