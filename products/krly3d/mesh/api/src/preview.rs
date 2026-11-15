//! Additive Rust rendering data. This contract has no v1/v2 JSON encoding.

use crate::{diagnostic, Diagnostic, Length, LengthUnit, MeshData};

/// Format identity for the additive Rust-only preview contract. This does not
/// define an encoding or advertise a format on the existing v1/v2 JSON wires.
pub const FORMAT: &str = "krly3d.mesh.preview/1";

#[derive(Clone, Debug, PartialEq)]
pub struct RenderMesh {
    pub geometry: MeshData,
    /// One index into `face_names` per triangle, in geometry order.
    pub source_faces: Vec<u32>,
    /// Outward source-surface normals in triangle/corner order. Shared positions
    /// may carry different normals at a crease.
    pub corner_normals: Vec<[[f64; 3]; 3]>,
    /// Source body face names, in source body face order.
    pub face_names: Vec<String>,
    /// Requested model tolerance; does not establish achieved error.
    pub tolerance: Length,
}

impl RenderMesh {
    /// Checks finite data, indices, alignment and a positive model tolerance.
    /// Zero normals are permitted; rendering consumers may require nonzero normals.
    /// Does not prove closure, winding, unit/outward normals, surface membership,
    /// manifoldness, or an achieved tessellation error.
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.tolerance.unit != LengthUnit::Model
            || !self.tolerance.value.is_finite()
            || self.tolerance.value <= 0.0
        {
            return Err(diagnostic("input", "render mesh requires a positive finite model tolerance"));
        }
        if self.source_faces.len() != self.geometry.triangles.len()
            || self.corner_normals.len() != self.geometry.triangles.len()
        {
            return Err(diagnostic("input", "render mesh face and normal counts must match its triangles"));
        }
        if self.geometry.points.iter().flatten().any(|value| !value.is_finite())
            || self.corner_normals.iter().flatten().flatten().any(|value| !value.is_finite())
        {
            return Err(diagnostic("input", "render mesh positions and normals must be finite"));
        }
        if self.geometry.triangles.iter().flatten().any(|index| *index as usize >= self.geometry.points.len())
        {
            return Err(diagnostic("input", "a render mesh triangle references a missing point"));
        }
        if self.source_faces.iter().any(|index| *index as usize >= self.face_names.len()) {
            return Err(diagnostic("input", "a render mesh triangle references a missing source face"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::RenderMesh;
    use crate::{Length, LengthUnit, MeshData};

    fn mesh() -> RenderMesh {
        RenderMesh {
            geometry: MeshData {
                points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                triangles: vec![[0, 1, 2]],
            },
            source_faces: vec![0],
            corner_normals: vec![[[0.0, 0.0, 1.0]; 3]],
            face_names: vec!["top".into()],
            tolerance: Length { value: 0.05, unit: LengthUnit::Model },
        }
    }

    #[test]
    fn validates_only_the_documented_structural_contract() {
        let mut found = mesh();
        assert!(found.validate().is_ok());
        found.corner_normals[0] = [[0.0; 3]; 3];
        found.geometry.triangles[0] = [0; 3];
        assert!(found.validate().is_ok()); // No unit-normal or nondegeneracy proof.
        found.geometry.points.clear();
        found.geometry.triangles.clear();
        found.source_faces.clear();
        found.corner_normals.clear();
        found.face_names.clear();
        assert!(found.validate().is_ok());
    }

    #[test]
    fn rejects_nonfinite_positions_normals_and_invalid_indices() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut found = mesh();
            found.geometry.points[2][1] = value;
            assert!(found.validate().is_err());
            let mut found = mesh();
            found.corner_normals[0][2][1] = value;
            assert!(found.validate().is_err());
        }
        let mut found = mesh();
        found.geometry.triangles[0][1] = 3;
        assert!(found.validate().is_err());
        found.geometry.triangles[0][1] = u32::MAX;
        assert!(found.validate().is_err());
        let mut found = mesh();
        found.source_faces[0] = 1;
        assert!(found.validate().is_err());
        found.source_faces[0] = u32::MAX;
        assert!(found.validate().is_err());
    }

    #[test]
    fn rejects_unaligned_counts_and_nonpositive_or_nonmodel_tolerance() {
        for count in [0, 2] {
            let mut found = mesh();
            found.source_faces.resize(count, 0);
            assert!(found.validate().is_err());
            let mut found = mesh();
            found.corner_normals.resize(count, [[0.0; 3]; 3]);
            assert!(found.validate().is_err());
        }
        for value in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut found = mesh();
            found.tolerance.value = value;
            assert!(found.validate().is_err());
        }
        let mut found = mesh();
        found.tolerance.unit = LengthUnit::Metre;
        assert!(found.validate().is_err());
    }
}
