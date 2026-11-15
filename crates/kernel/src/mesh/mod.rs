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
    /// Optional originating `Body::faces` index for each triangle, recorded by its
    /// tessellator. `None` means provenance is unavailable, never an inferred face.
    pub source_faces: Option<Vec<usize>>,
}

impl Mesh {
    pub fn corners(&self, triangle: usize) -> [Point3; 3] {
        self.triangles[triangle].map(|index| self.points[index])
    }

    pub fn area(&self) -> f64 {
        match self.area_checked(|| Ok::<(), std::convert::Infallible>(())) {
            Ok(area) => area,
            Err(never) => match never {},
        }
    }

    /// The total area, checking for cancellation before work, every 256 triangles, and after.
    pub fn area_checked<E>(&self, mut check: impl FnMut() -> Result<(), E>) -> Result<f64, E> {
        const CHECK_INTERVAL: usize = 256;

        check()?;
        let mut processed = 0;
        let area = (0..self.triangles.len()).try_fold(-0.0, |area, triangle| {
            let [a, b, c] = self.corners(triangle);
            let area = area + 0.5 * (b - a).cross(&(c - a)).norm();
            processed += 1;
            if processed % CHECK_INTERVAL == 0 {
                check()?;
            }
            Ok(area)
        })?;
        check()?;
        Ok(area)
    }

    /// The enclosed volume, from the triangles alone: `Σ a·(b × c) / 6`.
    pub fn volume(&self) -> f64 {
        match self.volume_checked(|| Ok::<(), std::convert::Infallible>(())) {
            Ok(volume) => volume,
            Err(never) => match never {},
        }
    }

    /// The enclosed volume, checking for cancellation before work, every 256 triangles, and after.
    pub fn volume_checked<E>(&self, mut check: impl FnMut() -> Result<(), E>) -> Result<f64, E> {
        const CHECK_INTERVAL: usize = 256;

        check()?;
        let mut processed = 0;
        let volume = (0..self.triangles.len()).try_fold(-0.0, |volume, triangle| {
            let [a, b, c] = self.corners(triangle);
            let volume = volume + a.coords.dot(&b.coords.cross(&c.coords)) / 6.0;
            processed += 1;
            if processed % CHECK_INTERVAL == 0 {
                check()?;
            }
            Ok(volume)
        })?;
        check()?;
        Ok(volume)
    }

    /// The sides used by one triangle and not by another one running the other way: none
    /// on a closed, consistently turned mesh.
    pub fn open_sides(&self) -> Vec<(usize, usize)> {
        match self.open_sides_checked(|| Ok::<(), std::convert::Infallible>(())) {
            Ok(sides) => sides,
            Err(never) => match never {},
        }
    }

    /// The unpaired sides, checking for cancellation before work and every 256 triangles or
    /// sides, and after each pass.
    pub fn open_sides_checked<E>(
        &self,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<Vec<(usize, usize)>, E> {
        const CHECK_INTERVAL: usize = 256;

        check()?;
        let mut count: BTreeMap<(usize, usize), i64> = BTreeMap::new();
        for (index, triangle) in self.triangles.iter().enumerate() {
            for side in 0..3 {
                let (from, to) = (triangle[side], triangle[(side + 1) % 3]);
                *count.entry((from.min(to), from.max(to))).or_insert(0) += if from < to { 1 } else { -1 };
            }
            if (index + 1) % CHECK_INTERVAL == 0 {
                check()?;
            }
        }
        check()?;

        let mut open_sides = Vec::new();
        for (index, (side, balance)) in count.into_iter().enumerate() {
            if balance != 0 {
                open_sides.push(side);
            }
            if (index + 1) % CHECK_INTERVAL == 0 {
                check()?;
            }
        }
        check()?;
        Ok(open_sides)
    }

    /// Adds another mesh's triangles, renumbering its points after these.
    pub fn append(&mut self, other: &Mesh) {
        let base = self.points.len();
        self.points.extend(other.points.iter().copied());
        self.triangles.extend(other.triangles.iter().map(|triangle| triangle.map(|index| index + base)));
        // Appending does not establish a shared source-body face namespace.
        self.source_faces = None;
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
        Mesh { points, triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]], ..Mesh::default() }
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

    #[test]
    fn append_drops_metadata_without_a_shared_source_body_namespace() {
        let mut mesh = tetrahedron();
        mesh.source_faces = Some(vec![0; mesh.triangles.len()]);
        let other = mesh.clone();
        mesh.append(&other);
        assert!(mesh.source_faces.is_none());
        assert_eq!(mesh.points.len(), 8);
        assert_eq!(mesh.triangles.len(), 8);
        assert_eq!(mesh.triangles[4], [4, 6, 5]);
    }

    fn many_disjoint_triangles(count: usize) -> Mesh {
        let points = (0..count * 3).map(|_| Point3::origin()).collect();
        let triangles = (0..count).map(|index| [index * 3, index * 3 + 1, index * 3 + 2]).collect();
        Mesh { points, triangles, ..Mesh::default() }
    }

    #[test]
    fn checked_statistics_match_the_legacy_methods() {
        let mesh = tetrahedron();

        assert_eq!(mesh.area_checked(|| Ok::<(), ()>(())), Ok(mesh.area()));
        assert_eq!(mesh.volume_checked(|| Ok::<(), ()>(())), Ok(mesh.volume()));
        assert_eq!(mesh.open_sides_checked(|| Ok::<(), ()>(())), Ok(mesh.open_sides()));
    }

    #[test]
    fn empty_checked_sums_preserve_sum_negative_zero() {
        let mesh = Mesh::default();
        let negative_zero_bits = (-0.0_f64).to_bits();

        assert_eq!(mesh.area_checked(|| Ok::<(), ()>(())).map(f64::to_bits), Ok(negative_zero_bits));
        assert_eq!(mesh.volume_checked(|| Ok::<(), ()>(())).map(f64::to_bits), Ok(negative_zero_bits));
        assert_eq!(mesh.area().to_bits(), negative_zero_bits);
        assert_eq!(mesh.volume().to_bits(), negative_zero_bits);
    }

    #[test]
    fn checked_statistics_stop_during_triangle_processing() {
        let mesh = many_disjoint_triangles(600);

        let mut checks = 0;
        assert_eq!(
            mesh.area_checked(|| {
                checks += 1;
                if checks == 3 {
                    Err(checks)
                } else {
                    Ok(())
                }
            }),
            Err(3)
        );

        let mut checks = 0;
        assert_eq!(
            mesh.volume_checked(|| {
                checks += 1;
                if checks == 3 {
                    Err(checks)
                } else {
                    Ok(())
                }
            }),
            Err(3)
        );

        let mut checks = 0;
        assert_eq!(
            mesh.open_sides_checked(|| {
                checks += 1;
                if checks == 3 {
                    Err(checks)
                } else {
                    Ok(())
                }
            }),
            Err(3)
        );
    }

    #[test]
    fn open_sides_checks_cancellation_during_entry_processing() {
        let mesh = many_disjoint_triangles(300);
        let mut checks = 0;

        assert_eq!(
            mesh.open_sides_checked(|| {
                checks += 1;
                if checks == 4 {
                    Err(checks)
                } else {
                    Ok(())
                }
            }),
            Err(4)
        );
    }
}
