//! Ray probes.

pub mod box_tree;
pub mod exhaustive;

use krly3d_core::{Accuracy, Attempt};
use krly3d_kernel::ports::probe::{Hit, Probe, Ray};
use krly3d_kernel::ports::trim::{place, Place, TrimRequest};
use krly3d_kernel::topo::{Body, FaceId};
use krly3d_kernel::{Context, KernelBuilder};
use std::sync::Arc;

pub fn register(builder: &mut KernelBuilder) {
    builder.plugin::<Probe>(box_tree::BoxTree);
    builder.plugin::<Probe>(exhaustive::Exhaustive);
}

/// Appends the hits of a ray on one face. Shared by every probe, so that they differ
/// only in which faces they bother to ask.
pub(crate) fn face_hits(
    cx: &Context,
    body: &Arc<Body>,
    face: FaceId,
    ray: &Ray,
    hits: &mut Vec<Hit>,
) -> Result<(), Attempt<Vec<Vec<Hit>>>> {
    let data = &body.faces[face];
    let mut parameters = Vec::new();
    data.surface.line(&ray.origin, &ray.direction, &mut parameters);
    parameters.sort_by(f64::total_cmp);
    let same = |a: f64, b: f64| cx.accuracy().is_zero_length((a - b) * ray.direction.norm());
    let mut index = 0;
    while index < parameters.len() {
        let t = parameters[index];
        let repeats = parameters[index..].iter().take_while(|other| same(t, **other)).count();
        index += repeats;
        // A root met an even number of times is where the line touches the surface without crossing it.
        if repeats % 2 == 0 {
            continue;
        }
        let point = ray.origin + ray.direction * t;
        // A line along the surface there touches it without crossing: a ray parallel to the
        // side of a cylinder meets it, if at all, only in the rounding of its root.
        let normal = data.surface.normal(data.surface.project(&point));
        if normal.dot(&ray.direction).abs() <= cx.accuracy().angle() * ray.direction.norm() {
            continue;
        }
        let on_edge = match place(cx, &TrimRequest { body: Arc::clone(body), face, point }) {
            Attempt::Done(Place::Outside) => continue,
            Attempt::Done(found) => found == Place::Boundary,
            Attempt::Declined(reason) => {
                return Err(Attempt::Declined(format!("face `{}`: {reason}", data.name)))
            }
            Attempt::Failed(error) => return Err(Attempt::Failed(error)),
        };
        hits.push(Hit {
            t,
            face,
            point,
            entering: ray.direction.dot(&data.outward_normal(&point)) < 0.0,
            on_edge,
        });
    }
    Ok(())
}

/// The hits of one ray in order, with the hits that share a point settled: a ray through
/// an edge crosses the boundary once if both faces agree on which way, and only touches
/// it if they do not.
pub(crate) fn settled(mut hits: Vec<Hit>, ray: &Ray, accuracy: &Accuracy) -> Vec<Hit> {
    hits.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut kept: Vec<Hit> = Vec::with_capacity(hits.len());
    let mut index = 0;
    while index < hits.len() {
        let first = hits[index];
        let together = hits[index..]
            .iter()
            .take_while(|hit| accuracy.is_zero_length((hit.t - first.t) * ray.direction.norm()))
            .count();
        let group = &hits[index..index + together];
        if group.iter().all(|hit| hit.entering == first.entering) {
            kept.push(Hit { on_edge: group.iter().any(|hit| hit.on_edge), ..first });
        }
        index += together;
    }
    kept
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{context, made};
    use krly3d_core::math::bounds::Bounds;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::probe::{cast, contract, side, Ray, Side};
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::topo::{body_bounds, Body};
    use krly3d_kernel::Context;
    use std::sync::Arc;

    /// The names of the faces a ray hits with the parameter of each hit, after checking the probe contract.
    fn hits(cx: &Context, body: &Arc<Body>, origin: [f64; 3], direction: [f64; 3]) -> Vec<(String, f64)> {
        let ray = Ray { origin: Point3::from(origin), direction: Vec3::from(direction) };
        let found = cast(cx, body, vec![ray]).into_result().unwrap().remove(0);
        contract::check(body, &ray, &found, cx.accuracy()).unwrap();
        found.iter().map(|hit| (body.faces[hit.face].name.clone(), hit.t)).collect()
    }

    fn plate(cx: &Context) -> Arc<Body> {
        let outline = Params::new().with("path", "M -30,-20 L 30,-20 L 30,20 L -30,20 Z O 20,15 4");
        made(cx, "extrude", Params::new().with("h", 10.0), &[made(cx, "profile", outline, &[])])
    }

    #[test]
    fn rays_along_the_side_of_a_turned_rod_are_answered_and_cross_it_only_inside() {
        // The rod's axis is turned onto -y, so rays along y are parallel to its side only up to
        // rounding. A picture taken from the front casts such rays at every distance from the
        // axis, those that run along the side included.
        let cx = context();
        let params =
            Params::new().with("r", 0.55).with("at", [-2.4, -4.4, 68.4]).with("to", [-2.4, -4.75, 68.4]);
        let rod = made(&cx, "cylinder", params, &[]);

        for step in -200..=200 {
            let offset = 0.55 + f64::from(step) * 1e-10;
            for (dx, dz) in [(offset, 0.0), (0.0, offset), (offset * 0.6, offset * 0.8)] {
                let found = hits(&cx, &rod, [-2.4 + dx, -20.0, 68.4 + dz], [0.0, 1.0, 0.0]);
                let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
                assert!(names.is_empty() || names == ["top", "bottom"], "at {offset}: {found:?}");
            }
        }
    }

    #[test]
    fn a_ray_through_a_block_enters_by_one_face_and_leaves_by_the_opposite_one() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 40.0).with("d", 10.0).with("h", 20.0), &[]);

        let found = hits(&cx, &block, [0.0, -50.0, 10.0], [0.0, 1.0, 0.0]);

        assert_eq!(found, [("front".to_string(), 45.0), ("back".to_string(), 55.0)]);
    }

    #[test]
    fn a_ray_across_a_hole_leaves_the_material_and_enters_it_again() {
        let cx = context();

        let found = hits(&cx, &plate(&cx), [20.0, -50.0, 5.0], [0.0, 1.0, 0.0]);

        let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["s1", "s5", "s5", "s3"]);
        assert_eq!(found.iter().map(|(_, t)| *t).collect::<Vec<_>>(), [30.0, 61.0, 69.0, 70.0]);
    }

    #[test]
    fn a_ray_down_a_hole_hits_nothing() {
        let cx = context();

        assert!(hits(&cx, &plate(&cx), [20.0, 15.0, 50.0], [0.0, 0.0, -1.0]).is_empty());
    }

    #[test]
    fn a_ray_through_an_edge_crosses_the_boundary_once_and_a_ray_touching_it_not_at_all() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 2.0).with("d", 2.0).with("h", 2.0), &[]);

        let through = hits(&cx, &block, [-3.0, -3.0, 1.0], [1.0, 1.0, 0.0]);
        let touching = hits(&cx, &block, [-3.0, 1.0, 1.0], [1.0, -1.0, 0.0]);

        assert_eq!(through.iter().map(|(_, t)| *t).collect::<Vec<_>>(), [2.0, 4.0]);
        assert!(touching.is_empty(), "{touching:?}");
    }

    #[test]
    fn rays_hit_curved_solids_where_their_equations_say() {
        let cx = context();
        let ball = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);
        let ring = made(&cx, "torus", Params::new().with("ring", 6.0).with("tube", 2.0), &[]);
        let cone = made(&cx, "cone", Params::new().with("r", 4.0).with("h", 8.0), &[]);

        let through_ball = hits(&cx, &ball, [-20.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let through_ring = hits(&cx, &ring, [-20.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let through_cone = hits(&cx, &cone, [-20.0, 0.0, 4.0], [1.0, 0.0, 0.0]);

        assert_eq!(through_ball.iter().map(|(_, t)| *t).collect::<Vec<_>>(), [15.0, 25.0]);
        assert_eq!(through_ring.len(), 4);
        assert!(
            through_cone
                .iter()
                .zip([18.0, 22.0])
                .all(|((name, t), expected)| name == "side" && (t - expected).abs() < 1e-12),
            "{through_cone:?}"
        );
    }

    #[test]
    fn a_point_is_inside_outside_or_on_the_boundary_of_a_solid() {
        let cx = context();
        let plate = plate(&cx);
        let at = |x: f64, y: f64, z: f64| side(&cx, &plate, &Point3::new(x, y, z));

        assert_eq!(at(0.0, 0.0, 5.0), Attempt::Done(Side::Inside));
        assert_eq!(at(20.0, 15.0, 5.0), Attempt::Done(Side::Outside));
        assert_eq!(at(0.0, 0.0, 10.0), Attempt::Done(Side::Boundary));
        assert_eq!(at(24.0, 15.0, 5.0), Attempt::Done(Side::Boundary));
        assert_eq!(at(0.0, 0.0, 11.0), Attempt::Done(Side::Outside));
    }

    #[test]
    fn the_box_around_a_solid_reaches_exactly_as_far_as_the_solid() {
        let cx = context();
        let around = |tool: &str, params: Params| {
            body_bounds(&cx, &made(&cx, tool, params, &[])).into_result().unwrap()
        };
        let reaches = |bounds: Bounds, low: [f64; 3], high: [f64; 3]| {
            (bounds.low - Point3::from(low)).norm() < 1e-12
                && (bounds.high - Point3::from(high)).norm() < 1e-12
        };

        assert!(reaches(
            around("box", Params::new().with("w", 40.0).with("d", 10.0).with("h", 20.0)),
            [-20.0, -5.0, 0.0],
            [20.0, 5.0, 20.0]
        ));
        assert!(reaches(
            around("cylinder", Params::new().with("r", 3.0).with("h", 10.0)),
            [-3.0, -3.0, 0.0],
            [3.0, 3.0, 10.0]
        ));
        assert!(reaches(
            around("sphere", Params::new().with("r", 5.0).with("at", [1.0, 2.0, 3.0])),
            [-4.0, -3.0, -2.0],
            [6.0, 7.0, 8.0]
        ));
        assert!(reaches(
            around("torus", Params::new().with("ring", 6.0).with("tube", 2.0)),
            [-8.0, -8.0, -2.0],
            [8.0, 8.0, 2.0]
        ));
        assert!(reaches(
            around("cone", Params::new().with("r", 4.0).with("h", 9.0)),
            [-4.0, -4.0, 0.0],
            [4.0, 4.0, 9.0]
        ));
    }
}
