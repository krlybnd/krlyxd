//! Names of edges. They are worked out from the faces and never stored, so an edge keeps
//! its name however the edges of its body happen to be numbered.

use super::body::{Body, EdgeId, FaceId};
use krly3d_core::math::Point3;
use krly3d_core::Accuracy;
use std::collections::BTreeMap;

/// For every edge, the faces whose boundary runs along it, in face order. An edge of a
/// solid lies between two faces, or twice on the boundary of one; an edge on the rim of a
/// sheet belongs to a single face.
pub fn edge_faces(body: &Body) -> Vec<Vec<FaceId>> {
    let mut faces = vec![Vec::new(); body.edges.len()];
    for (face, data) in body.faces.iter().enumerate() {
        for used in data.uses() {
            if let Some(users) = faces.get_mut(used.edge) {
                users.push(face);
            }
        }
    }
    faces
}

/// The name of every edge, by edge number.
///
/// An edge between two faces is called after them, in name order: `back|top`. Any other
/// edge keeps the label it was drawn with, which is how the edges of a profile are named,
/// and is called `e` and its number when it has none. Edges that would share a name are
/// told apart by a running number, `back|top`, `back|top#2`, …, given in the order of
/// their midpoints by x, then y, then z, so that the numbering does not depend on the
/// order the edges are stored in.
pub fn edge_names(body: &Body, accuracy: &Accuracy) -> Vec<String> {
    let faces = edge_faces(body);
    let mut names: Vec<String> = body
        .edges
        .iter()
        .enumerate()
        .map(|(edge, data)| match faces[edge].as_slice() {
            [one, other] => between(&body.faces[*one].name, &body.faces[*other].name),
            _ => data.label.clone().unwrap_or_else(|| format!("e{}", edge + 1)),
        })
        .collect();
    let mut sharing: BTreeMap<String, Vec<EdgeId>> = BTreeMap::new();
    for (edge, name) in names.iter().enumerate() {
        sharing.entry(name.clone()).or_default().push(edge);
    }
    for (name, mut edges) in sharing {
        edges.sort_by_cached_key(|edge| grid_cell(&body.edges[*edge].point(0.5), accuracy));
        for (position, edge) in edges.into_iter().enumerate().skip(1) {
            names[edge] = format!("{name}#{}", position + 1);
        }
    }
    names
}

/// The edge called `name`, by the rules of [`edge_names`].
pub fn edge_by_name(body: &Body, accuracy: &Accuracy, name: &str) -> Option<EdgeId> {
    edge_names(body, accuracy).iter().position(|known| known == name)
}

fn between(one: &str, other: &str) -> String {
    if one <= other {
        format!("{one}|{other}")
    } else {
        format!("{other}|{one}")
    }
}

/// The point in whole steps of the length tolerance. Coordinates that differ only by
/// round-off fall on the same step, so they cannot decide an order.
fn grid_cell(point: &Point3, accuracy: &Accuracy) -> [i64; 3] {
    [point.x, point.y, point.z].map(|coordinate| (coordinate / accuracy.length()).round() as i64)
}
