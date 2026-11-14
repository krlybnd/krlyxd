//! Topological naming: stable, human- and AI-readable names for faces and edges.
//!
//! Rule set (deterministic, recomputable, never stored in the B-rep itself):
//! * a face is named by its `key` (authored, e.g. `top`, or inherited);
//! * an edge is named by the sorted names of its two faces: `front|top`;
//!   if two faces share several edges, `#1`, `#2`… are appended in a
//!   geometric order (by midpoint), so the order survives renumbering;
//! * after an operation, result faces inherit the name of the input face
//!   they geometrically coincide with (same surface, same placement).
//!   Faces without a predecessor are named after the operation,
//!   e.g. `chamfer(front|top)`.

use crate::brep::{BrepEnvelope, FaceRole, FaceSource, SurfaceGeometry};
use crate::math::{cross, dot, norm, sub, Point3};
use std::collections::BTreeMap;

/// The two faces bordering an edge (`None` for a free / wire edge).
pub fn edge_faces(brep: &BrepEnvelope, edge: u32) -> Option<(u32, u32)> {
    let t = &brep.topology;
    let e = t.edges.get(edge as usize)?;
    let a = t.halfedges.get(e.halfedge as usize)?.face?;
    let b = t.halfedges.get(e.twin_halfedge? as usize)?.face?;
    Some((a, b))
}

/// Endpoints of an edge in model coordinates.
pub fn edge_ends(brep: &BrepEnvelope, edge: u32) -> Option<(Point3, Point3)> {
    let t = &brep.topology;
    let half = t.halfedges.get(t.edges.get(edge as usize)?.halfedge as usize)?;
    Some((
        t.vertices.get(half.from as usize)?.position,
        t.vertices.get(half.to as usize)?.position,
    ))
}

pub fn face_name(brep: &BrepEnvelope, face: u32) -> Option<&str> {
    brep.topology.faces.get(face as usize).map(|f| f.key.as_str())
}

pub fn face_by_name(brep: &BrepEnvelope, name: &str) -> Option<u32> {
    brep.topology.faces.iter().find(|f| f.key == name).map(|f| f.id)
}

/// Names of all edges, indexed by edge id.
pub fn edge_names(brep: &BrepEnvelope) -> Vec<String> {
    let t = &brep.topology;
    let mut base: Vec<String> = Vec::with_capacity(t.edges.len());
    for edge in &t.edges {
        let name = match edge_faces(brep, edge.id) {
            Some((a, b)) => {
                let mut pair = [
                    face_name(brep, a).unwrap_or("?").to_string(),
                    face_name(brep, b).unwrap_or("?").to_string(),
                ];
                pair.sort();
                format!("{}|{}", pair[0], pair[1])
            }
            None => format!("free-{}", edge.id),
        };
        base.push(name);
    }
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, name) in base.iter().enumerate() {
        groups.entry(name.as_str()).or_default().push(index);
    }
    let mut names = base.clone();
    for (_, mut members) in groups {
        if members.len() < 2 {
            continue;
        }
        members.sort_by(|&x, &y| midpoint_key(brep, x).cmp(&midpoint_key(brep, y)));
        for (rank, index) in members.into_iter().enumerate() {
            names[index] = format!("{}#{}", base[index], rank + 1);
        }
    }
    names
}

fn midpoint_key(brep: &BrepEnvelope, edge: usize) -> [i64; 3] {
    let Some((a, b)) = edge_ends(brep, edge as u32) else {
        return [0; 3];
    };
    std::array::from_fn(|i| (((a[i] + b[i]) * 0.5) * 1e6).round() as i64)
}

pub fn edge_by_name(brep: &BrepEnvelope, name: &str) -> Option<u32> {
    edge_names(brep)
        .iter()
        .position(|n| n == name)
        .map(|i| i as u32)
}

/// Do two analytic surfaces describe the same point set?
pub fn coincident(a: &SurfaceGeometry, b: &SurfaceGeometry, tol: f64) -> bool {
    let parallel = |u: Point3, v: Point3| norm(cross(u, v)) <= 1e-9;
    let on_axis = |o: Point3, origin: Point3, axis: Point3| norm(cross(sub(o, origin), axis)) <= tol;
    let same = |x: f64, y: f64| (x - y).abs() <= tol;
    match (a, b) {
        (SurfaceGeometry::Plane { frame: f }, SurfaceGeometry::Plane { frame: g }) => {
            parallel(f.z, g.z) && dot(sub(g.origin, f.origin), f.z).abs() <= tol
        }
        (
            SurfaceGeometry::Cylinder { frame: f, radius: r },
            SurfaceGeometry::Cylinder { frame: g, radius: s },
        ) => same(*r, *s) && parallel(f.z, g.z) && on_axis(g.origin, f.origin, f.z),
        (
            SurfaceGeometry::Sphere { frame: f, radius: r },
            SurfaceGeometry::Sphere { frame: g, radius: s },
        ) => same(*r, *s) && norm(sub(f.origin, g.origin)) <= tol,
        (
            SurfaceGeometry::Cone { frame: f, semi_angle: r },
            SurfaceGeometry::Cone { frame: g, semi_angle: s },
        ) => (r - s).abs() <= 1e-9 && parallel(f.z, g.z) && norm(sub(f.origin, g.origin)) <= tol,
        (
            SurfaceGeometry::Torus { frame: f, major_radius: r, minor_radius: m },
            SurfaceGeometry::Torus { frame: g, major_radius: s, minor_radius: n },
        ) => same(*r, *s) && same(*m, *n) && parallel(f.z, g.z) && norm(sub(f.origin, g.origin)) <= tol,
        _ => false,
    }
}

/// Give the faces of `result` the names of the `input` faces they coincide with.
/// Faces with no predecessor get `created` (with `#n` if there are several).
/// One input face continuing in several result faces is a split: `key#1`, `key#2`…
pub fn inherit(input: &BrepEnvelope, result: &mut BrepEnvelope, created: &str) {
    inherit_from(&[(input, None)], result, created);
}

/// As `inherit`, from several inputs in priority order. Faces inherited from an
/// input with a prefix are named `prefix:key` (e.g. the wall of a drilled hole
/// is `drill:side`).
pub fn inherit_from(inputs: &[(&BrepEnvelope, Option<&str>)], result: &mut BrepEnvelope, created: &str) {
    let tol = result.accuracy.geometric.max(1e-7) * 10.0;
    // (input, face) each result face continues, if any
    let mut matches: Vec<Option<(usize, usize)>> = Vec::with_capacity(result.topology.faces.len());
    for face in &result.topology.faces {
        let surface = result.geometry.surface(face.surface).ok();
        let found = surface.and_then(|s| {
            inputs.iter().enumerate().find_map(|(i, (input, _))| {
                input
                    .topology
                    .faces
                    .iter()
                    .position(|old| input.geometry.surface(old.surface).map(|o| coincident(o, s, tol)).unwrap_or(false))
                    .map(|f| (i, f))
            })
        });
        matches.push(found);
    }
    let name_of = |(i, f): (usize, usize)| -> String {
        let key = &inputs[i].0.topology.faces[f].key;
        match inputs[i].1 {
            Some(prefix) => format!("{prefix}:{key}"),
            None => key.clone(),
        }
    };
    let mut total: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for m in matches.iter().flatten() {
        *total.entry(*m).or_insert(0) += 1;
    }
    let fresh_total = matches.iter().filter(|m| m.is_none()).count();
    let mut seen: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    let mut fresh = 0usize;
    for (face, found) in result.topology.faces.iter_mut().zip(&matches) {
        match found {
            Some(m) => {
                let (input, old) = (inputs[m.0].0, &inputs[m.0].0.topology.faces[m.1]);
                let rank = seen.entry(*m).or_insert(0);
                *rank += 1;
                let split = total[m] > 1;
                face.key = if split { format!("{}#{}", name_of(*m), rank) } else { name_of(*m) };
                face.provenance.role = if split { FaceRole::Split } else { FaceRole::Preserved };
                face.provenance.sources = vec![FaceSource {
                    entity: input.id.clone(),
                    body: input.id.clone(),
                    key: old.key.clone(),
                    face: old.id,
                }];
            }
            None => {
                fresh += 1;
                face.key = if fresh_total == 1 { created.to_string() } else { format!("{created}#{fresh}") };
                face.provenance.role = FaceRole::Authored;
            }
        }
    }
}

#[cfg(test)]
mod tests;
