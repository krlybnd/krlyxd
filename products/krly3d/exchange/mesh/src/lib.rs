//! Generic, non-repairing binary STL export. Coordinates must already be millimetres.
//!
//! STL has no unit metadata. The explicit unit argument is a declaration by the
//! caller, not a conversion or an inference from the bounding box.

pub use krly3d_mesh::print::{report, PrintReport};
use krly3d_mesh::{diagnostic, Diagnostic, MeshData};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units {
    Millimetres,
}

pub struct StlExport {
    pub bytes: Vec<u8>,
    pub original: PrintReport,
    pub roundtrip: PrintReport,
}

/// Validate original geometry and the exact f32 coordinates written into STL.
/// Reject rounding collapses, newly coincident vertices and flipped triangles.
/// Repeats the bounded numerical self-intersection check after rounding.
pub fn binary_stl(mesh: &MeshData, units: Units) -> Result<StlExport, Diagnostic> {
    let Units::Millimetres = units;
    let original = report(mesh)?;
    original.require_exportable()?;
    let rounded = rounded_mesh(mesh)?;
    let roundtrip = report(&rounded)?;
    roundtrip.require_exportable().map_err(|e| diagnostic("float32_roundtrip", e.message))?;
    for (source, target) in mesh.triangles.iter().zip(&rounded.triangles) {
        let a = normal(mesh, *source)?;
        let b = normal(&rounded, *target)?;
        if a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>() <= 0.0 {
            return Err(diagnostic("float32_roundtrip", "f32 rounding reverses a triangle normal"));
        }
    }
    let count = u32::try_from(rounded.triangles.len())
        .map_err(|_| diagnostic("resource_limit", "STL triangle count exceeds u32"))?;
    let length = rounded
        .triangles
        .len()
        .checked_mul(50)
        .and_then(|n| n.checked_add(84))
        .ok_or_else(|| diagnostic("resource_limit", "STL byte length overflows"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| diagnostic("resource_limit", "cannot allocate STL output"))?;
    let mut header = [0u8; 80];
    let text = b"KRLY binary STL; coordinates declared mm; numeric preflight, not print proof";
    header[..text.len()].copy_from_slice(text);
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&count.to_le_bytes());
    for triangle in &rounded.triangles {
        for value in normal(&rounded, *triangle)? {
            bytes.extend_from_slice(&(value as f32).to_le_bytes());
        }
        for index in triangle {
            for value in rounded.points[*index as usize] {
                bytes.extend_from_slice(&(value as f32).to_le_bytes());
            }
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
    }
    Ok(StlExport { bytes, original, roundtrip })
}

fn rounded_mesh(mesh: &MeshData) -> Result<MeshData, Diagnostic> {
    let mut points = Vec::with_capacity(mesh.points.len());
    for point in &mesh.points {
        let rounded = point.map(|v| f64::from(v as f32));
        if rounded.iter().any(|v| !v.is_finite()) {
            return Err(diagnostic(
                "float32_roundtrip",
                "coordinate cannot be represented as finite STL f32",
            ));
        }
        points.push(rounded);
    }
    Ok(MeshData { points, triangles: mesh.triangles.clone() })
}

fn normal(mesh: &MeshData, triangle: [u32; 3]) -> Result<[f64; 3], Diagnostic> {
    let [a, b, c] = triangle.map(|i| mesh.points[i as usize]);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let length = n[0].hypot(n[1]).hypot(n[2]);
    if !length.is_finite() || length == 0.0 {
        return Err(diagnostic(
            "float32_roundtrip",
            "triangle normal is degenerate or outside numeric range",
        ));
    }
    Ok(n.map(|x| x / length))
}

/// Strict shape and numeric parsing of the field engine's generic mesh JSON.
/// Integer indices must be JSON integers representable as u32, never rounded floats.
pub fn decode_mesh(bytes: &[u8]) -> Result<MeshData, Diagnostic> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| diagnostic("input", format!("invalid mesh JSON: {e}")))?;
    let object = value.as_object().ok_or_else(|| diagnostic("input", "mesh must be an object"))?;
    if object.len() != 2 || !object.contains_key("points") || !object.contains_key("triangles") {
        return Err(diagnostic("input", "mesh requires exactly points and triangles"));
    }
    let source_points =
        object["points"].as_array().ok_or_else(|| diagnostic("input", "points must be an array"))?;
    let source_triangles =
        object["triangles"].as_array().ok_or_else(|| diagnostic("input", "triangles must be an array"))?;
    if source_points.len() > krly3d_mesh::print::MAX_POINTS
        || source_triangles.len() > krly3d_mesh::print::MAX_TRIANGLES
    {
        return Err(diagnostic("resource_limit", "mesh size limit exceeded"));
    }
    let mut points = Vec::with_capacity(source_points.len());
    for point in source_points {
        let coordinates = triple(point)?;
        let mut parsed = [0.0; 3];
        for (target, value) in parsed.iter_mut().zip(coordinates) {
            *target = value
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(|| diagnostic("input", "coordinates must be finite numbers"))?;
        }
        points.push(parsed);
    }
    let mut triangles = Vec::with_capacity(source_triangles.len());
    for triangle in source_triangles {
        let indices = triple(triangle)?;
        let mut parsed = [0; 3];
        for (target, value) in parsed.iter_mut().zip(indices) {
            *target = value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| diagnostic("input", "triangle indices must be u32 integers"))?;
        }
        triangles.push(parsed);
    }
    Ok(MeshData { points, triangles })
}

fn triple(value: &serde_json::Value) -> Result<&[serde_json::Value], Diagnostic> {
    value
        .as_array()
        .filter(|a| a.len() == 3)
        .map(Vec::as_slice)
        .ok_or_else(|| diagnostic("input", "each point and triangle must contain exactly three values"))
}

#[cfg(test)]
mod tests {
    use super::{binary_stl, decode_mesh, Units};
    use krly3d_mesh::MeshData;

    fn tetrahedron() -> MeshData {
        MeshData {
            points: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        }
    }

    #[test]
    fn exact_binary_framing_normals_and_vertices() {
        let mesh = tetrahedron();
        let export = binary_stl(&mesh, Units::Millimetres).unwrap();
        assert_eq!(export.bytes.len(), 84 + 50 * 4);
        assert_eq!(&export.bytes[80..84], &4u32.to_le_bytes());
        for (index, record) in export.bytes[84..].as_chunks::<50>().0.iter().enumerate() {
            assert_eq!(&record[48..50], &[0, 0]);
            let numbers: Vec<f32> =
                record[..48].as_chunks::<4>().0.iter().map(|v| f32::from_le_bytes(*v)).collect();
            assert!((numbers[..3].iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 1e-6);
            for corner in 0..3 {
                let expected = mesh.points[mesh.triangles[index][corner] as usize].map(|v| v as f32);
                assert_eq!(&numbers[3 + corner * 3..6 + corner * 3], &expected);
            }
        }
        assert_eq!(export.original, export.roundtrip);
    }

    #[test]
    fn rejects_float_overflow_and_rounding_collapses() {
        let mut mesh = tetrahedron();
        mesh.points[1][0] = 1e40;
        assert!(binary_stl(&mesh, Units::Millimetres).is_err());
        let mut mesh = tetrahedron();
        for p in &mut mesh.points {
            p[0] += 1e8;
        }
        assert!(super::report(&mesh).unwrap().export_eligible);
        assert!(binary_stl(&mesh, Units::Millimetres).is_err());
        let mut mesh = tetrahedron();
        mesh.points[0][0] = f64::NAN;
        assert!(binary_stl(&mesh, Units::Millimetres).is_err());
    }

    #[test]
    fn strict_mesh_json() {
        assert!(decode_mesh(br#"{"points":[[0,0,0]],"triangles":[[0,0,0]]}"#).is_ok());
        for input in [
            r#"{"points":[[1e400,0,0]],"triangles":[]}"#,
            r#"{"points":[[null,0,0]],"triangles":[]}"#,
            r#"{"points":[[0,0]],"triangles":[]}"#,
            r#"{"points":[],"triangles":[[0.5,0,0]]}"#,
            r#"{"points":[],"triangles":[[4294967296,0,0]]}"#,
        ] {
            assert!(decode_mesh(input.as_bytes()).is_err());
        }
    }
}
