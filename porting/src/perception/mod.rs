//! Perception layer: what the AI "sees" of a body.
//!
//! Three views, all recomputed from the B-rep (nothing here is authored data):
//! * `overview` – one line per face (the zoomed-out view);
//! * `here`     – standing on one face: its edges, neighbours, what is behind
//!                it (wall thickness) and in front of it (tool access);
//! * `Index`    – sparse keys + relation graph for search (see index.rs).
//! Plus `face_delta`: which named faces appeared / disappeared in an edit.

pub mod index;
pub mod probe;

pub use index::{Index, IndexBuilder, KeyLayer, Query};
pub use probe::{ray, sentence, Hit};

use crate::brep::{BrepEnvelope, GeometryError};
use crate::math::{add, norm, scale, sub};
use crate::naming::{edge_ends, edge_names, face_by_name};
use probe::{edge_character, face_anchor, num, pt, surface_text};
use std::collections::BTreeSet;
use std::fmt::Write;

/// One line per face.
pub fn overview(brep: &BrepEnvelope) -> Result<String, GeometryError> {
    let mut out = String::new();
    for face in &brep.topology.faces {
        let s = brep.geometry.surface(face.surface)?;
        let (p, n) = face_anchor(brep, face.id)?;
        let text = match s {
            crate::brep::SurfaceGeometry::Plane { .. } => format!("plane n={}", probe::axis(n)),
            _ => surface_text(s),
        };
        let _ = writeln!(out, "{}: {} at {}", face.key, text, pt(p));
    }
    Ok(out)
}

/// The view from one face.
pub fn here(brep: &BrepEnvelope, face_name: &str) -> Result<String, GeometryError> {
    let id = face_by_name(brep, face_name)
        .ok_or_else(|| GeometryError::InvalidTopology(format!("no face named {face_name}")))?;
    let face = &brep.topology.faces[id as usize];
    let surface = brep.geometry.surface(face.surface)?;
    let (p, n) = face_anchor(brep, id)?;
    let names = edge_names(brep);
    let mut out = String::new();
    let shown = match surface {
        crate::brep::SurfaceGeometry::Plane { .. } => format!("plane n={}", probe::axis(n)),
        _ => surface_text(surface),
    };
    let _ = writeln!(out, "here: {} [{}] at {}", face.key, shown, pt(p));
    let _ = writeln!(out, "edges:");
    for half in brep.topology.halfedges.iter().filter(|h| h.face == Some(id)) {
        let edge = half.edge;
        let other = half
            .twin
            .and_then(|t| brep.topology.halfedges.get(t as usize))
            .and_then(|t| t.face);
        let (a, b) = edge_ends(brep, edge).unwrap_or(([0.0; 3], [0.0; 3]));
        let chord = norm(sub(b, a));
        match other {
            Some(o) => {
                let (q, _) = face_anchor(brep, o)?;
                let mid = edge_mid(brep, edge).unwrap_or(scale(add(a, b), 0.5));
                let na = probe::normal_near(brep, id, mid)?;
                let nb = probe::normal_near(brep, o, mid)?;
                let (convex, angle) = edge_character(na, nb, sub(q, mid));
                let kind = if (angle - 180.0).abs() < 1e-6 { "tangent" } else if convex { "convex" } else { "concave" };
                let _ = writeln!(
                    out,
                    "  {} -> {}  {} {}°  chord {}",
                    names[edge as usize],
                    brep.topology.faces[o as usize].key,
                    kind,
                    num(angle),
                    num(chord)
                );
            }
            None => {
                let _ = writeln!(out, "  {} (free edge)", names[edge as usize]);
            }
        }
    }
    // behind: ray into the material; in front: ray away from it
    let eps = brep.accuracy.geometric * 100.0;
    let inward = ray(brep, add(p, scale(n, -eps)), scale(n, -1.0))?;
    match inward.iter().find(|h| !h.entering) {
        Some(h) => {
            let _ = writeln!(out, "behind: material {} thick, then [{}]", num(h.t + eps), h.face);
        }
        None => {
            let _ = writeln!(out, "behind: (no exit found)");
        }
    }
    let outward = ray(brep, add(p, scale(n, eps)), n)?;
    match outward.first() {
        Some(h) => {
            let _ = writeln!(out, "in front: blocked by [{}] after {}", h.face, num(h.t + eps));
        }
        None => {
            let _ = writeln!(out, "in front: free (reachable from outside)");
        }
    }
    Ok(out)
}

/// Midpoint of the edge on its carrying curve (exact for arcs, not the chord).
fn edge_mid(brep: &BrepEnvelope, edge: u32) -> Option<crate::math::Point3> {
    use crate::brep::{Curve, EdgeGeometry};
    let e = brep.topology.edges.get(edge as usize)?;
    match &e.geometry {
        EdgeGeometry::Curve { curve, range } => {
            let c = brep.geometry.curve(*curve).ok()?;
            c.point_at(0.5 * (range.lo + range.hi)).ok()
        }
        EdgeGeometry::Collapsed { vertex } => brep.topology.vertices.get(*vertex as usize).map(|v| v.position),
    }
}

/// Named-face difference between two versions of a body.
pub fn face_delta(before: &BrepEnvelope, after: &BrepEnvelope) -> (Vec<String>, Vec<String>) {
    let a: BTreeSet<&str> = before.topology.faces.iter().map(|f| f.key.as_str()).collect();
    let b: BTreeSet<&str> = after.topology.faces.iter().map(|f| f.key.as_str()).collect();
    (
        b.difference(&a).map(|s| s.to_string()).collect(),
        a.difference(&b).map(|s| s.to_string()).collect(),
    )
}

#[cfg(test)]
mod tests {
    use crate::protocol::Session;
    use serde_json::json;

    fn session() -> Session {
        let mut s = Session::new();
        s.call("create", &json!({"id":"b","primitive":{"kind":"Cuboid","width":40,"height":20,"depth":10}}))
            .unwrap();
        s
    }

    #[test]
    fn ray_reads_as_a_sentence() {
        let mut s = session();
        let r = s.call("ray", &json!({"id":"b","origin":[0,10,50],"direction":[0,0,-1]})).unwrap();
        assert_eq!(r["text"], "air 45 > [front] solid 10 > [back] air");
    }

    #[test]
    fn here_reports_thickness_and_edge_types() {
        let mut s = session();
        s.call("fillet", &json!({"id":"b","edges":["back|top"],"radius":3})).unwrap();
        let text = s.call("here", &json!({"id":"b","face":"top"})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(text.contains("behind: material 20 thick, then [bottom]"), "{text}");
        assert!(text.contains("fillet(back|top)  tangent 180°"), "{text}");
        assert!(text.contains("right|top -> right  convex 90°"), "{text}");
    }

    #[test]
    fn query_uses_graph_context() {
        let mut s = session();
        s.call("fillet", &json!({"id":"b","edges":["back|top"],"radius":3})).unwrap();
        let r = s.call("find", &json!({"id":"b","query":"cylinder","k":5})).unwrap();
        assert_eq!(r["matches"][0][0], "fillet(back|top)");
        // planes touching the cylinder outrank the plane that does not (front)
        let r = s.call("find", &json!({"id":"b","query":"plane near:kind:cylinder","k":6})).unwrap();
        let names: Vec<&str> = r["matches"].as_array().unwrap().iter().map(|m| m[0].as_str().unwrap()).collect();
        assert_eq!(names.last(), Some(&"front"), "{names:?}");
    }

    #[test]
    fn index_follows_edits() {
        let mut s = session();
        assert_eq!(s.call("find", &json!({"id":"b","query":"made:chamfer"})).unwrap()["matches"], json!([]));
        s.call("chamfer", &json!({"id":"b","edges":["front|top"],"distance":2})).unwrap();
        let r = s.call("find", &json!({"id":"b","query":"made:chamfer"})).unwrap();
        assert_eq!(r["matches"][0][0], "chamfer(front|top)");
    }

    #[test]
    fn rollback_restores_names() {
        let mut s = session();
        let mark = s.call("mark", &json!({})).unwrap()["mark"].clone();
        s.call("chamfer", &json!({"id":"b","edges":["front|top"],"distance":2})).unwrap();
        s.call("rollback", &json!({"mark": mark})).unwrap();
        let edges = s.call("edges", &json!({"id":"b"})).unwrap();
        assert!(edges["edges"].as_array().unwrap().contains(&json!("front|top")));
    }
}
