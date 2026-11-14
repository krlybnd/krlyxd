//! What makes a body valid.

use super::body::{Body, Face, Use};
use krly3d_core::{Accuracy, Error};
use std::collections::BTreeSet;

/// Verifies names, loops, edge uses, that the geometry of edges, faces and vertices agrees,
/// and for a solid that every shell is a closed surface of whole genus.
pub fn check(body: &Body, accuracy: &Accuracy) -> Result<(), Error> {
    names(body)?;
    for face in &body.faces {
        loops(body, face)?;
        geometry(body, face, accuracy)?;
    }
    uses(body)?;
    if body.solid {
        closed_shells(body)?;
    }
    Ok(())
}

fn names(body: &Body) -> Result<(), Error> {
    let mut seen = BTreeSet::new();
    for face in &body.faces {
        if face.name.is_empty() {
            return Err(Error::topology("a face has no name"));
        }
        if !seen.insert(face.name.as_str()) {
            return Err(Error::topology(format!("two faces are named `{}`", face.name)));
        }
    }
    Ok(())
}

fn loops(body: &Body, face: &Face) -> Result<(), Error> {
    let broken = |what: &str| Error::topology(format!("face `{}`: {what}", face.name));
    for uses in &face.loops {
        if uses.is_empty() {
            return Err(broken("an empty boundary loop"));
        }
        for (index, used) in uses.iter().enumerate() {
            let edge =
                body.edges.get(used.edge).ok_or_else(|| broken("a boundary refers to a missing edge"))?;
            let next = &uses[(index + 1) % uses.len()];
            match (ends(body, used), ends(body, next)) {
                (None, _) if uses.len() == 1 => {}
                (Some((_, end)), Some((start, _))) if end == start => {}
                _ => return Err(broken("a boundary loop is not a closed chain of edges")),
            }
            if edge.range.0 >= edge.range.1 {
                return Err(broken("an edge with an empty parameter range"));
            }
        }
    }
    if face.loops.is_empty() && face.surface.euler_characteristic().is_none() {
        return Err(broken("no boundary, although its surface is not closed"));
    }
    Ok(())
}

fn ends(body: &Body, used: &Use) -> Option<(usize, usize)> {
    let (start, end) = body.edges.get(used.edge)?.ends?;
    Some(if used.forward { (start, end) } else { (end, start) })
}

fn geometry(body: &Body, face: &Face, accuracy: &Accuracy) -> Result<(), Error> {
    for used in face.uses() {
        let edge = &body.edges[used.edge];
        for fraction in [0.0, 0.5, 1.0] {
            let gap = face.surface.distance(&edge.point(fraction));
            if !accuracy.is_zero_length(gap) {
                return Err(Error::topology(format!(
                    "an edge of face `{}` is {gap} off its surface",
                    face.name
                )));
            }
        }
        if let Some((start, end)) = edge.ends {
            for (vertex, fraction) in [(start, 0.0), (end, 1.0)] {
                let point = body
                    .vertices
                    .get(vertex)
                    .ok_or_else(|| Error::topology("an edge refers to a missing vertex"))?;
                if !accuracy.same_point(point, &edge.point(fraction)) {
                    return Err(Error::topology(format!(
                        "an edge of face `{}` does not end at its vertex",
                        face.name
                    )));
                }
            }
        }
    }
    Ok(())
}

/// In a solid every edge separates exactly two faces and is run once in each direction;
/// in a sheet an edge on the rim is used once.
fn uses(body: &Body) -> Result<(), Error> {
    let mut counts = vec![(0_u32, 0_u32); body.edges.len()];
    for used in body.faces.iter().flat_map(Face::uses) {
        if used.forward {
            counts[used.edge].0 += 1;
        } else {
            counts[used.edge].1 += 1;
        }
    }
    for (forward, backward) in counts {
        let valid = if body.solid {
            forward == 1 && backward == 1
        } else {
            forward <= 1 && backward <= 1 && forward + backward >= 1
        };
        if !valid {
            let kind = if body.solid { "solid" } else { "sheet" };
            return Err(Error::topology(format!(
                "an edge of the {kind} is used {forward} time(s) forward and {backward} backward"
            )));
        }
    }
    Ok(())
}

/// Euler–Poincaré, written for faces that need not be discs:
/// `V − E + C + Σ χ(face) = 2·(shells − handles)`, where `C` counts closed edges (each
/// stands for a vertex it does not have) and `χ(face)` is that of the whole surface for a
/// face without boundary and `2 − loops` otherwise.
fn closed_shells(body: &Body) -> Result<(), Error> {
    let closed_edges = body.edges.iter().filter(|edge| edge.ends.is_none()).count();
    let mut characteristic = (body.vertices.len() + closed_edges) as i64 - body.edges.len() as i64;
    for face in &body.faces {
        characteristic += match face.loops.len() {
            0 => i64::from(face.surface.euler_characteristic().unwrap_or(0)),
            count => 2 - count as i64,
        };
    }
    let handles_twice = 2 * shells(body) as i64 - characteristic;
    if handles_twice < 0 || handles_twice % 2 != 0 {
        return Err(Error::topology(format!(
            "the faces do not close into whole shells: Euler characteristic {characteristic} for {} shell(s)",
            shells(body)
        )));
    }
    Ok(())
}

/// Number of groups of faces connected through shared edges.
fn shells(body: &Body) -> usize {
    let mut group: Vec<usize> = (0..body.faces.len()).collect();
    fn root(group: &mut [usize], mut face: usize) -> usize {
        while group[face] != face {
            group[face] = group[group[face]];
            face = group[face];
        }
        face
    }
    let mut owner = vec![None; body.edges.len()];
    for (index, face) in body.faces.iter().enumerate() {
        for used in face.uses() {
            match owner[used.edge] {
                Some(other) => {
                    let (a, b) = (root(&mut group, index), root(&mut group, other));
                    group[a] = b;
                }
                None => owner[used.edge] = Some(index),
            }
        }
    }
    (0..body.faces.len()).filter(|face| root(&mut group, *face) == *face).count()
}
