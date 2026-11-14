//! Where the faces of the two solids meet.

use super::work::{settle, Stop, Work};
use crate::trim::crossing::{crossings, Uncrossable};
use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::Point3;
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::ports::section::{section, Cut, SectionRequest};
use krly3d_kernel::ports::surface::Stretch;
use krly3d_kernel::ports::trim::{place, Place, TrimRequest};
use krly3d_kernel::topo::{face_bounds, Edge};
use krly3d_kernel::Context;
use std::sync::Arc;

/// A place where a section curve crosses a boundary edge of one of the two faces.
struct Stopping {
    /// Parameter on the section curve.
    at: f64,
    point: Point3,
    side: usize,
    edge: usize,
    /// Parameter on the edge.
    on_edge: f64,
}

/// Finds the section edges of every pair of faces and marks the original edges where
/// they end. Pairs of faces on the same surface are noted for `shared_faces`.
pub(crate) fn sections(cx: &Context, work: &mut Work) -> Result<(), Stop> {
    let reach = |side: usize, work: &Work| -> Result<Vec<Bounds>, Stop> {
        (0..work.bodies[side].faces.len())
            .map(|face| {
                settle(face_bounds(cx, work.bodies[side], face))
                    .map(|bounds| bounds.grown(work.accuracy.length()))
            })
            .collect()
    };
    let (first, second) = (reach(0, work)?, reach(1, work)?);
    let mut crossing = Vec::new();
    for (fa, around_a) in first.iter().enumerate() {
        for (fb, around_b) in second.iter().enumerate() {
            let Some(common) = overlap(around_a, around_b) else { continue };
            let request = SectionRequest {
                first: Arc::clone(&work.bodies[0].faces[fa].surface),
                second: Arc::clone(&work.bodies[1].faces[fb].surface),
            };
            match settle(section(cx, &request))? {
                Cut::Same => work.same.push((fa, fb)),
                Cut::Curves { curves, .. } => {
                    crossing.extend(curves.into_iter().map(|curve| (fa, fb, curve, common)))
                }
                // A tangent contact leaves pieces whose side cannot be told by a point beside
                // the contact; refusing beats a wrong solid.
                Cut::Touching { .. } => return Err(Stop::Declined("the solids touch tangentially".into())),
            }
        }
    }
    for (fa, fb, curve, common) in crossing {
        along_section(cx, work, [fa, fb], &curve, &common)?;
    }
    for (fa, fb) in work.same.clone() {
        boundary_crossings(cx, work, fa, fb)?;
    }
    Ok(())
}

/// The box two boxes have in common, if they overlap.
fn overlap(a: &Bounds, b: &Bounds) -> Option<Bounds> {
    let common = Bounds { low: a.low.sup(&b.low), high: a.high.inf(&b.high) };
    (0..3).all(|axis| common.low[axis] <= common.high[axis]).then_some(common)
}

/// Keeps the stretches of a section curve that lie inside both faces as new edges.
fn along_section(
    cx: &Context,
    work: &mut Work,
    faces: [usize; 2],
    curve: &Arc<dyn Curve>,
    common: &Bounds,
) -> Result<(), Stop> {
    let (from, to) = match curve.period() {
        Some(period) => (0.0, period),
        // A straight section matters only within the box both faces reach into.
        None => match common.line(&curve.point(0.0), &curve.tangent(0.0)) {
            Some(within) => within,
            None => return Ok(()),
        },
    };
    let stretch = Stretch { curve: Arc::clone(curve), from, to };
    let mut stops: Vec<Stopping> = Vec::new();
    for side in [0, 1] {
        let body = work.bodies[side];
        for used in body.faces[faces[side]].uses() {
            match crossings(cx, &stretch, &body.edges[used.edge]) {
                Ok(found) => stops.extend(found.into_iter().map(|crossing| Stopping {
                    at: from + (to - from) * crossing.fraction,
                    point: crossing.point,
                    side,
                    edge: used.edge,
                    on_edge: crossing.parameter,
                })),
                // The section runs along this edge; the test of each stretch below sees that.
                Err(Uncrossable::Along) => {}
                Err(Uncrossable::NoRule(reason)) => return Err(Stop::Declined(reason)),
            }
        }
    }
    stops.sort_by(|a, b| a.at.total_cmp(&b.at));
    let closed = curve.period().is_some();
    for (start, end) in stretches(&stops, (from, to), closed, work) {
        let middle = curve.point(0.5 * (start + end));
        let mut places = [Place::Inside; 2];
        for side in [0, 1] {
            let request =
                TrimRequest { body: Arc::clone(work.bodies[side]), face: faces[side], point: middle };
            places[side] = settle(place(cx, &request))?;
        }
        match places {
            [Place::Inside, Place::Inside] => {
                let piece = work.piece(Arc::clone(curve), (start, end), closed && stops.is_empty());
                work.cuts[0][faces[0]].push(piece);
                work.cuts[1][faces[1]].push(piece);
                let (first, last) = (curve.point(start), curve.point(end));
                for stop in stops.iter().filter(|stop| {
                    work.accuracy.same_point(&stop.point, &first)
                        || work.accuracy.same_point(&stop.point, &last)
                }) {
                    work.marks[stop.side][stop.edge].push(stop.on_edge);
                }
            }
            [Place::Outside, _] | [_, Place::Outside] => {}
            along => explained(work, faces, &middle, along)?,
        }
    }
    Ok(())
}

/// The stretches of a section curve between consecutive stops. A stretch inside two
/// bounded faces ends at a boundary at both ends, so what lies before the first stop of a
/// straight curve or after its last is never one; a closed curve without stops is one
/// whole stretch.
fn stretches(stops: &[Stopping], range: (f64, f64), closed: bool, work: &Work) -> Vec<(f64, f64)> {
    let mut distinct: Vec<f64> = Vec::new();
    let mut last_point: Option<Point3> = None;
    for stop in stops {
        if !last_point.is_some_and(|last| work.accuracy.same_point(&last, &stop.point)) {
            distinct.push(stop.at);
            last_point = Some(stop.point);
        }
    }
    let ends_meet = stops
        .first()
        .zip(stops.last())
        .is_some_and(|(first, last)| work.accuracy.same_point(&first.point, &last.point));
    if closed && distinct.len() > 1 && ends_meet {
        distinct.pop();
    }
    match (closed, distinct.first().copied()) {
        (true, None) => vec![range],
        (true, Some(first)) => {
            distinct.push(first + (range.1 - range.0));
            distinct.windows(2).map(|pair| (pair[0], pair[1])).collect()
        }
        (false, _) => distinct.windows(2).map(|pair| (pair[0], pair[1])).collect(),
    }
}

/// A stretch of a section that runs along an edge of one solid is in order when that
/// edge borders a face lying on the same surface as the other solid's face: the edge is
/// then handled with the shared faces. Any other contact along an edge is declined.
fn explained(work: &Work, faces: [usize; 2], middle: &Point3, places: [Place; 2]) -> Result<(), Stop> {
    for side in [0, 1] {
        if places[side] != Place::Boundary {
            continue;
        }
        let body = work.bodies[side];
        let on = |edge: &Edge| work.accuracy.same_point(&edge.curve.point(edge.nearest(middle)), middle);
        let Some(along) =
            body.faces[faces[side]].uses().find(|used| on(&body.edges[used.edge])).map(|used| used.edge)
        else {
            return Err(Stop::Declined(
                "a section runs along the boundary of a face but along none of its edges".into(),
            ));
        };
        let other = faces[1 - side];
        let shares_surface = |neighbour: usize| {
            work.same.contains(&if side == 0 { (neighbour, other) } else { (other, neighbour) })
        };
        let neighbours = (0..body.faces.len())
            .filter(|face| *face != faces[side] && body.faces[*face].uses().any(|used| used.edge == along));
        if !neighbours.into_iter().any(shares_surface) {
            let (mine, theirs) = (&body.faces[faces[side]].name, &work.bodies[1 - side].faces[other].name);
            let which = ["first", "second"][side];
            return Err(Stop::Declined(format!(
                "an edge of face `{mine}` of the {which} solid lies in face `{theirs}` of the other without crossing it"
            )));
        }
    }
    Ok(())
}

/// Marks the boundary edges of two faces on the same surface where they cross each other.
fn boundary_crossings(cx: &Context, work: &mut Work, fa: usize, fb: usize) -> Result<(), Stop> {
    let (a, b) = (work.bodies[0], work.bodies[1]);
    for used_a in a.faces[fa].uses() {
        let edge_a = &a.edges[used_a.edge];
        let stretch = Stretch { curve: Arc::clone(&edge_a.curve), from: edge_a.range.0, to: edge_a.range.1 };
        for used_b in b.faces[fb].uses() {
            match crossings(cx, &stretch, &b.edges[used_b.edge]) {
                Ok(found) => {
                    for crossing in found {
                        work.marks[0][used_a.edge].push(edge_a.nearest(&crossing.point));
                        work.marks[1][used_b.edge].push(crossing.parameter);
                    }
                }
                Err(Uncrossable::Along) => {
                    return Err(Stop::Declined("edges of the two solids run along each other".into()))
                }
                Err(Uncrossable::NoRule(reason)) => return Err(Stop::Declined(reason)),
            }
        }
    }
    Ok(())
}

/// For faces on the same surface: the boundary of one, where it lies inside the other,
/// divides the other. Run after the edges are split, so that each piece is wholly inside
/// or wholly outside.
pub(crate) fn shared_faces(cx: &Context, work: &mut Work) -> Result<(), Stop> {
    for (fa, fb) in work.same.clone() {
        for (cut_side, cut_face, edge_side, edge_face) in [(0, fa, 1, fb), (1, fb, 0, fa)] {
            let edges: Vec<usize> =
                work.bodies[edge_side].faces[edge_face].uses().map(|used| used.edge).collect();
            for edge in edges {
                for piece in work.split[edge_side][edge].clone() {
                    let middle = work.pieces[piece].point(0.5);
                    let request = TrimRequest {
                        body: Arc::clone(work.bodies[cut_side]),
                        face: cut_face,
                        point: middle,
                    };
                    match settle(place(cx, &request))? {
                        Place::Inside if !work.cuts[cut_side][cut_face].contains(&piece) => {
                            work.cuts[cut_side][cut_face].push(piece)
                        }
                        Place::Inside | Place::Outside => {}
                        Place::Boundary => {
                            return Err(Stop::Declined("edges of the two solids run along each other".into()))
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
