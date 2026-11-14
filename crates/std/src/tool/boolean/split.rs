//! Dividing every face of one solid along the cuts that lie in it.

use super::work::{settle, Stop, Work};
use krly3d_core::math::Vec3;
use krly3d_kernel::ports::trim::{place, Place, TrimRequest};
use krly3d_kernel::topo::{Body, Face, Use};
use krly3d_kernel::Context;
use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::sync::Arc;

/// The solid `side` with every face divided into the pieces its cuts make of it. The
/// edges of the returned body are the shared pieces of the boolean, so a face of it can
/// go into the result as it is.
pub(crate) fn pieces(cx: &Context, work: &Work, side: usize) -> Result<Body, Stop> {
    let body = work.bodies[side];
    let mut divided =
        Body { vertices: work.points.clone(), edges: work.pieces.clone(), faces: Vec::new(), solid: true };
    for (index, face) in body.faces.iter().enumerate() {
        let cuts = &work.cuts[side][index];
        if !cuts.is_empty() && face.surface.euler_characteristic() == Some(0) {
            // On a torus a closed curve need not have two sides, which the grouping of loops relies on.
            return Err(Stop::Declined(format!(
                "face `{}` lies on a torus, and dividing a torus face is not supported yet",
                face.name
            )));
        }
        let mut halves: Vec<Use> = Vec::new();
        for used in face.uses() {
            let along = &work.split[side][used.edge];
            let ordered: Vec<usize> =
                if used.forward { along.clone() } else { along.iter().rev().copied().collect() };
            halves.extend(ordered.into_iter().map(|edge| Use { edge, forward: used.forward }));
        }
        halves.extend(
            cuts.iter()
                .flat_map(|edge| [Use { edge: *edge, forward: true }, Use { edge: *edge, forward: false }]),
        );
        let loops = trace(work, face, &halves)?;
        for group in grouped(cx, work, face, loops)? {
            divided.faces.push(Face { loops: group, ..face.clone() });
        }
    }
    Ok(divided)
}

/// Direction of travel where a half edge starts (`at_end` false) or ends.
fn heading(work: &Work, half: &Use, at_end: bool) -> Vec3 {
    let edge = &work.pieces[half.edge];
    let parameter = if half.forward == at_end { edge.range.1 } else { edge.range.0 };
    edge.curve.tangent(parameter) * if half.forward { 1.0 } else { -1.0 }
}

fn ends(work: &Work, half: &Use) -> Option<(usize, usize)> {
    let (start, end) = work.pieces[half.edge].ends?;
    Some(if half.forward { (start, end) } else { (end, start) })
}

/// Follows the half edges into closed loops, each with its piece of the face on the left.
///
/// At a vertex the loop continues along the first edge found turning clockwise from the
/// way it came, seen from outside: that is the edge that closes off the narrowest wedge
/// of the face next to the edge just travelled.
fn trace(work: &Work, face: &Face, halves: &[Use]) -> Result<Vec<Vec<Use>>, Stop> {
    let mut loops = Vec::new();
    let mut taken = vec![false; halves.len()];
    for first in 0..halves.len() {
        if taken[first] {
            continue;
        }
        let mut current = first;
        let mut traced = Vec::new();
        loop {
            taken[current] = true;
            traced.push(halves[current]);
            let Some((_, vertex)) = ends(work, &halves[current]) else { break };
            let next = turn(work, face, halves, current, vertex)?;
            if next == first {
                break;
            }
            if taken[next] {
                return Err(Stop::Declined(format!(
                    "the cuts in face `{}` do not close into loops",
                    face.name
                )));
            }
            current = next;
        }
        loops.push(traced);
    }
    Ok(loops)
}

/// The half edge that continues a loop arriving at `vertex` along `halves[arriving]`.
fn turn(work: &Work, face: &Face, halves: &[Use], arriving: usize, vertex: usize) -> Result<usize, Stop> {
    let out = face.outward_normal(&work.points[vertex]);
    let back = -heading(work, &halves[arriving], true);
    let across = out.cross(&back);
    let came = halves[arriving];
    let mut best: Option<(f64, usize)> = None;
    for (index, half) in halves.iter().enumerate() {
        if ends(work, half).map(|(start, _)| start) != Some(vertex) {
            continue;
        }
        let returns = half.edge == came.edge && half.forward != came.forward;
        let leaving = heading(work, half, false);
        // Clockwise angle from the way back, in the plane tangent to the face.
        let clockwise =
            if returns { TAU } else { (-leaving.dot(&across)).atan2(leaving.dot(&back)).rem_euclid(TAU) };
        if !returns && (clockwise <= work.accuracy.angle() || TAU - clockwise <= work.accuracy.angle()) {
            return Err(Stop::Declined(format!(
                "two edges leave a vertex of face `{}` in the same direction",
                face.name
            )));
        }
        if best.is_none_or(|(least, _)| clockwise < least) {
            best = Some((clockwise, index));
        }
    }
    best.map(|(_, index)| index).ok_or_else(|| {
        Stop::Declined(format!("an edge of face `{}` ends where nothing continues", face.name))
    })
}

/// Puts together the loops that bound the same piece of the face.
///
/// Two loops bound the same piece exactly when, for every loop, they lie on the same
/// side of it. A loop lies beside itself on its left, and on the right of a loop that
/// runs the same edge the other way; for any other loop the trim rule decides.
fn grouped(cx: &Context, work: &Work, face: &Face, loops: Vec<Vec<Use>>) -> Result<Vec<Vec<Vec<Use>>>, Stop> {
    if loops.len() < 2 {
        return Ok(if loops.is_empty() { vec![Vec::new()] } else { vec![loops] });
    }
    let alone = Arc::new(Body {
        vertices: work.points.clone(),
        edges: work.pieces.clone(),
        faces: loops.iter().map(|uses| Face { loops: vec![uses.clone()], ..face.clone() }).collect(),
        solid: false,
    });
    let mut groups: BTreeMap<Vec<bool>, Vec<Vec<Use>>> = BTreeMap::new();
    for (index, uses) in loops.iter().enumerate() {
        let sample = uses[0];
        let middle = work.pieces[sample.edge].point(0.5);
        let mut sides = Vec::with_capacity(loops.len());
        for (other, others) in loops.iter().enumerate() {
            let opposite =
                others.iter().any(|used| used.edge == sample.edge && used.forward != sample.forward);
            sides.push(if other == index {
                true
            } else if opposite {
                false
            } else {
                match settle(place(
                    cx,
                    &TrimRequest { body: Arc::clone(&alone), face: other, point: middle },
                ))? {
                    Place::Inside => true,
                    Place::Outside => false,
                    Place::Boundary => {
                        return Err(Stop::Declined(format!("cuts in face `{}` touch each other", face.name)))
                    }
                }
            });
        }
        groups.entry(sides).or_default().push(uses.clone());
    }
    Ok(groups.into_values().collect())
}
