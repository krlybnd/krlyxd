//! Trim rule for any surface that can be walked on: go from the point to the boundary
//! and see from which side the boundary is reached.

use super::crossing::{crossings, Crossing, Uncrossable};
use super::on_boundary;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::surface::Stretch;
use krly3d_kernel::ports::trim::{Place, Trim, TrimRequest};
use krly3d_kernel::topo::{Body, Face, Use};
use krly3d_kernel::{Context, Plugin};

pub struct WalkTrim;

/// Where along an edge the walk is aimed, in the order tried. The middle comes first;
/// the others are for the walks that reach the middle through a corner or along an edge.
const AIMS: [f64; 7] = [0.5, 0.25, 0.75, 0.125, 0.375, 0.625, 0.875];

impl Plugin<Trim> for WalkTrim {
    fn id(&self) -> Id {
        Id::new("surface")
    }

    fn call(&self, cx: &Context, request: &TrimRequest) -> Attempt<Place> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        if face.loops.is_empty() {
            return Attempt::Done(Place::Inside);
        }
        if on_boundary(&request.body, face, &request.point, cx.accuracy()) {
            return Attempt::Done(Place::Boundary);
        }
        for aim in AIMS {
            for used in face.uses() {
                let target = request.body.edges[used.edge].point(aim);
                let path = face.surface.walk(&request.point, &target);
                match first_crossing(cx, &request.body, face, &path) {
                    Ok(Some(place)) => return Attempt::Done(place),
                    Ok(None) => {}
                    Err(reason) => return Attempt::declined(reason),
                }
            }
        }
        Attempt::declined(
            "every walk tried from the point reaches the boundary through a corner or along an edge",
        )
    }
}

/// Follows the path to the first boundary edge it crosses. The face lies to the left of
/// its boundary, so a path heading left across an edge is entering the face and started
/// outside it. `None` when the first crossing does not say: at a corner, or grazing.
fn first_crossing(cx: &Context, body: &Body, face: &Face, path: &[Stretch]) -> Result<Option<Place>, String> {
    let accuracy = cx.accuracy();
    for (index, stretch) in path.iter().enumerate() {
        let mut found: Vec<(&Use, Crossing)> = Vec::new();
        for used in face.uses() {
            let met = match crossings(cx, stretch, &body.edges[used.edge]) {
                Ok(met) => met,
                // A walk that runs along an edge cannot say on which side of it it is; another aim will.
                Err(Uncrossable::Along) => return Ok(None),
                Err(missed) => return Err(missed.reason()),
            };
            found.extend(met.into_iter().map(|crossing| (used, crossing)));
        }
        if index > 0 {
            // A crossing where a later stretch starts was met at the end of the one before.
            found.retain(|(_, crossing)| !accuracy.same_point(&crossing.point, &stretch.point(0.0)));
        }
        found.sort_by(|a, b| a.1.fraction.total_cmp(&b.1.fraction));
        let Some((used, first)) = found.first() else { continue };
        if found.get(1).is_some_and(|(_, second)| accuracy.same_point(&second.point, &first.point)) {
            return Ok(None);
        }
        let edge = &body.edges[used.edge];
        let along_edge = edge.curve.tangent(first.parameter) * if used.forward { 1.0 } else { -1.0 };
        let left = face.outward_normal(&first.point).cross(&along_edge);
        let travel = stretch.travel(first.fraction);
        let heading = travel.dot(&left);
        if heading.abs() <= accuracy.angle() * travel.norm() * left.norm() {
            return Ok(None);
        }
        return Ok(Some(if heading > 0.0 { Place::Outside } else { Place::Inside }));
    }
    Ok(None)
}
