//! The fast ray probe: a tree of boxes over the faces, so that a ray is tested only
//! against the faces whose boxes it passes through.
//!
//! It asks the faces it keeps exactly as the reference probe asks all of them, in the
//! same order, so the two cannot differ in a hit. The tree belongs to the body and is
//! built once, however many rays and calls follow.

use super::{face_hits, settled};
use krly3d_core::math::bounds::Bounds;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::probe::{Hit, Probe, ProbeRequest, Ray, RAYS};
use krly3d_kernel::topo::{face_bounds, Body, FaceId};
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct BoxTree;

/// A box is halved until it holds no more faces than this.
const LEAF: usize = 4;

impl Plugin<Probe> for BoxTree {
    fn id(&self) -> Id {
        Id::new(RAYS)
    }

    fn call(&self, cx: &Context, request: &ProbeRequest) -> Attempt<Vec<Vec<Hit>>> {
        let tree = match cx.derived(&request.body, || Tree::over(cx, &request.body)) {
            Attempt::Done(tree) => tree,
            Attempt::Declined(reason) => return Attempt::Declined(reason),
            Attempt::Failed(error) => return Attempt::Failed(error),
        };
        let mut per_ray = Vec::with_capacity(request.rays.len());
        let (mut faces, mut pending) = (Vec::new(), Vec::new());
        for ray in &request.rays {
            tree.pierced(ray, &mut faces, &mut pending);
            let mut hits = Vec::new();
            for face in &faces {
                if let Err(unanswered) = face_hits(cx, &request.body, *face, ray, &mut hits) {
                    return unanswered;
                }
            }
            per_ray.push(settled(hits, ray, cx.accuracy()));
        }
        Attempt::Done(per_ray)
    }
}

/// The boxes of the faces of one body, each node holding the box around everything
/// below it. The root is node 0.
struct Tree {
    nodes: Vec<Node>,
    /// Faces without a box of their own, which every ray has to ask.
    unbounded: Vec<FaceId>,
}

struct Node {
    bounds: Bounds,
    below: Below,
}

enum Below {
    Faces(Vec<FaceId>),
    Halves(usize, usize),
}

impl Tree {
    fn over(cx: &Context, body: &Arc<Body>) -> Attempt<Self> {
        // A hit counts when it lies within the length tolerance of an edge, so a face
        // reaches that far beyond its exact box; as much again covers the round-off of
        // the box test itself.
        let margin = 2.0 * cx.accuracy().length();
        let mut tree = Self { nodes: Vec::new(), unbounded: Vec::new() };
        let mut boxes = Vec::with_capacity(body.faces.len());
        for face in 0..body.faces.len() {
            match face_bounds(cx, body, face) {
                Attempt::Done(bounds) if bounds.is_empty() => tree.unbounded.push(face),
                Attempt::Done(bounds) => boxes.push((face, bounds.grown(margin))),
                Attempt::Declined(reason) => return Attempt::Declined(reason),
                Attempt::Failed(error) => return Attempt::Failed(error),
            }
        }
        if !boxes.is_empty() {
            tree.grow(boxes);
        }
        Attempt::Done(tree)
    }

    /// Adds the node around `boxes` with everything below it, and returns its number.
    fn grow(&mut self, mut boxes: Vec<(FaceId, Bounds)>) -> usize {
        let bounds = boxes.iter().fold(Bounds::empty(), |around, (_, one)| around.union(one));
        let node = self.nodes.len();
        self.nodes.push(Node { bounds, below: Below::Faces(Vec::new()) });
        if boxes.len() <= LEAF {
            self.nodes[node].below = Below::Faces(boxes.into_iter().map(|(face, _)| face).collect());
            return node;
        }
        // Halved at the median along the direction in which the node is longest.
        let size = bounds.size();
        let axis = (0..3).max_by(|a, b| size[*a].total_cmp(&size[*b])).unwrap_or(0);
        boxes.sort_by(|a, b| a.1.center()[axis].total_cmp(&b.1.center()[axis]));
        let far = boxes.split_off(boxes.len() / 2);
        let near = self.grow(boxes);
        let far = self.grow(far);
        self.nodes[node].below = Below::Halves(near, far);
        node
    }

    /// Fills `faces` with the faces whose boxes the line of the ray passes through.
    /// `pending` is working space, kept by the caller from ray to ray.
    fn pierced(&self, ray: &Ray, faces: &mut Vec<FaceId>, pending: &mut Vec<usize>) {
        faces.clear();
        faces.extend_from_slice(&self.unbounded);
        pending.clear();
        pending.extend((!self.nodes.is_empty()).then_some(0));
        while let Some(node) = pending.pop() {
            let node = &self.nodes[node];
            if node.bounds.line(&ray.origin, &ray.direction).is_none() {
                continue;
            }
            match &node.below {
                Below::Faces(held) => faces.extend_from_slice(held),
                Below::Halves(near, far) => pending.extend([*near, *far]),
            }
        }
        // Where two faces share a hit on their common edge, the one asked first reports
        // it; asking in face order keeps that the same as in the reference probe.
        faces.sort_unstable();
    }
}

#[cfg(test)]
mod tests {
    use super::{BoxTree, Tree};
    use crate::probe::exhaustive::Exhaustive;
    use crate::tool::testing::{context, made};
    use krly3d_core::math::bounds::Bounds;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::probe::{cast, contract, Probe, ProbeRequest, Ray};
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::topo::{body_bounds, Body};
    use krly3d_kernel::{Context, Plugin};
    use std::f64::consts::TAU;
    use std::sync::Arc;

    /// A column shaft with flutes drawn as one profile and pushed up: two faces for every
    /// flute and two caps. With 24 flutes it is the shaft of the reference model.
    fn fluted_shaft(cx: &Context, flutes: u32) -> Arc<Body> {
        let (radius, flute, centre) = (0.5_f64, 0.046_f64, 0.506_f64);
        let alpha = ((radius * radius + centre * centre - flute * flute) / (2.0 * radius * centre)).acos();
        let on = |angle: f64| format!("{},{}", radius * angle.cos(), radius * angle.sin());
        let mut path = format!("M {}", on(-alpha));
        for k in 0..flutes {
            let phi = TAU * f64::from(k) / f64::from(flutes);
            path += &format!(" A {},{} {} cw", centre * phi.cos(), centre * phi.sin(), on(phi + alpha));
            path += &format!(" A 0,0 {} ccw", on(TAU * f64::from(k + 1) / f64::from(flutes) - alpha));
        }
        let outline = made(cx, "profile", Params::new().with("path", (path + " Z").as_str()), &[]);
        made(cx, "extrude", Params::new().with("h", 7.35), &[outline])
    }

    /// A row of `count` pegs as one body, three faces each.
    fn pegs(cx: &Context, count: f64) -> Arc<Body> {
        let peg = made(cx, "cylinder", Params::new().with("r", 3.0).with("h", 6.0), &[]);
        made(cx, "pattern", Params::new().with("n", count).with("step", [8.0, 0.0, 0.0]), &[peg])
    }

    fn solids(cx: &Context) -> Vec<(&'static str, Arc<Body>)> {
        let upright = |path: &str| Params::new().with("path", path).with("plane", "xz");
        let tube = made(cx, "profile", upright("M 2,0 L 5,0 L 5,4 L 2,4 Z"), &[]);
        let disc = made(cx, "profile", upright("O 6,0 2"), &[]);
        let holed = Params::new().with("path", "M -30,-20 L 30,-20 L 30,20 L -30,20 Z O 20,15 4");
        let plate = made(cx, "profile", holed, &[]);
        vec![
            ("block", made(cx, "box", Params::new().with("w", 40.0).with("d", 10.0).with("h", 20.0), &[])),
            ("plate with a hole", made(cx, "extrude", Params::new().with("h", 10.0), &[plate])),
            ("cylinder", made(cx, "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[])),
            ("cone", made(cx, "cone", Params::new().with("r", 4.0).with("h", 9.0), &[])),
            ("frustum", made(cx, "cone", Params::new().with("r", 4.0).with("h", 9.0).with("top", 2.0), &[])),
            ("ball", made(cx, "sphere", Params::new().with("r", 5.0).with("at", [1.0, 2.0, 3.0]), &[])),
            ("ring", made(cx, "torus", Params::new().with("ring", 6.0).with("tube", 2.0), &[])),
            ("tube", made(cx, "revolve", Params::new(), &[Arc::clone(&tube)])),
            ("part of a tube", made(cx, "revolve", Params::new().with("angle", 250.0), &[tube])),
            ("part of a ring", made(cx, "revolve", Params::new().with("angle", 200.0), &[disc])),
            ("row of pegs", pegs(cx, 12.0)),
            ("fluted shaft", fluted_shaft(cx, 6)),
        ]
    }

    /// The points of a lattice with `steps` intervals a side, filling the box.
    fn lattice(bounds: &Bounds, steps: u32) -> Vec<Point3> {
        let size = bounds.size();
        let at = |i: u32, axis: usize| bounds.low[axis] + size[axis] * f64::from(i) / f64::from(steps);
        let mut points = Vec::new();
        for i in 0..=steps {
            for j in 0..=steps {
                for k in 0..=steps {
                    points.push(Point3::new(at(i, 0), at(j, 1), at(k, 2)));
                }
            }
        }
        points
    }

    /// Rays from a lattice of points around the body, and one in its middle, towards a
    /// lattice of points all over its box. The targets on the sides of the box send rays
    /// along faces and through edges and corners on purpose.
    fn rays_around(cx: &Context, body: &Arc<Body>, origins: u32, targets: u32) -> Vec<Ray> {
        let bounds = body_bounds(cx, body).into_result().unwrap();
        let around = bounds.grown(0.75 * bounds.size().norm());
        let mut rays = Vec::new();
        for origin in lattice(&around, origins) {
            for target in lattice(&bounds, targets) {
                if (target - origin).norm() > 0.0 {
                    rays.push(Ray { origin, direction: target - origin });
                }
            }
        }
        rays
    }

    #[test]
    fn it_finds_exactly_the_hits_of_the_exhaustive_probe() {
        let cx = context();
        for (name, body) in solids(&cx) {
            let rays = rays_around(&cx, &body, 2, 2);
            // One ray to a call, so that a failure names the ray.
            for ray in rays {
                let request = ProbeRequest { body: Arc::clone(&body), rays: vec![ray] };

                let (reference, found) = (Exhaustive.call(&cx, &request), BoxTree.call(&cx, &request));

                assert!(matches!(reference, Attempt::Done(_)), "{name}: {ray:?}: {reference:?}");
                assert_eq!(found, reference, "{name}: {ray:?}");
            }
        }
    }

    #[test]
    fn its_hits_satisfy_the_probe_contract() {
        let cx = context();
        // Directions with no two components equal, so that no ray runs along an edge.
        let skew = [[1.0, 0.618, 0.382], [-0.618, 1.0, 0.382], [0.382, -0.618, 1.0]].map(Vec3::from);
        for (name, body) in solids(&cx) {
            let centre = body_bounds(&cx, &body).into_result().unwrap().center();
            let rays: Vec<Ray> =
                skew.iter().map(|direction| Ray { origin: centre, direction: *direction }).collect();

            let found = BoxTree.call(&cx, &ProbeRequest { body: Arc::clone(&body), rays: rays.clone() });

            let Attempt::Done(per_ray) = found else { panic!("{name}: {found:?}") };
            for (ray, hits) in rays.iter().zip(&per_ray) {
                contract::check(&body, ray, hits, cx.accuracy())
                    .unwrap_or_else(|broken| panic!("{name}: {broken}"));
            }
        }
    }

    #[test]
    fn a_ray_beside_every_box_hits_nothing() {
        let cx = context();
        let row = pegs(&cx, 12.0);
        let beside = Ray { origin: Point3::new(-10.0, 5.0, 3.0), direction: Vec3::x() };

        let found = BoxTree.call(&cx, &ProbeRequest { body: row, rays: vec![beside] });

        assert_eq!(found, Attempt::Done(vec![Vec::new()]));
    }

    #[test]
    fn the_tree_of_a_body_is_built_once_and_found_again_by_later_calls() {
        let cx = context();
        let row = pegs(&cx, 12.0);
        let along = Ray { origin: Point3::new(-10.0, 0.0, 3.0), direction: Vec3::x() };
        let request = ProbeRequest { body: Arc::clone(&row), rays: vec![along] };
        let remembered = |cx: &Context| cx.derived::<Tree>(&row, || Attempt::declined("no tree was kept"));

        let before = remembered(&cx).map(|_| ());
        let first = BoxTree.call(&cx, &request);
        let Attempt::Done(kept) = remembered(&cx) else { panic!("no tree after the first call") };
        let second = BoxTree.call(&cx, &request);
        let Attempt::Done(kept_later) = remembered(&cx) else { panic!("no tree after the second call") };

        assert_eq!(before, Attempt::declined("no tree was kept"));
        assert!(Arc::ptr_eq(&kept, &kept_later));
        assert_eq!(first, second);
    }

    #[test]
    fn the_standard_kernel_asks_it_before_the_exhaustive_probe() {
        let cx = context();
        let row = pegs(&cx, 12.0);
        let along = Ray { origin: Point3::new(-10.0, 0.0, 3.0), direction: Vec3::x() };

        let found = cast(&cx, &row, vec![along]).into_result().unwrap();

        let tree = cx.derived::<Tree>(&row, || Attempt::declined("the box tree was not asked"));
        assert_eq!(found[0].len(), 24);
        assert!(matches!(tree, Attempt::Done(_)), "the box tree was not asked");
    }

    /// Prints the times for the report; run with `--nocapture` to read them. Nothing is
    /// asserted about time, only that the probes agree on the rays that were timed.
    #[test]
    fn both_probes_are_timed_on_the_same_rays_and_agree() {
        let cx = context();
        for (name, body) in [("fluted shaft", fluted_shaft(&cx, 24)), ("row of 60 pegs", pegs(&cx, 60.0))] {
            let rays = rays_around(&cx, &body, 1, 2);
            let request = ProbeRequest { body: Arc::clone(&body), rays: rays.clone() };
            let timed = |probe: &dyn Plugin<Probe>| {
                let start = cx.clock().now();
                let found = probe.call(&cx, &request);
                (found, (cx.clock().now() - start).as_secs_f64() * 1e3)
            };

            let (reference, exhaustive) = timed(&Exhaustive);
            let (first, building) = timed(&BoxTree);
            let (second, kept) = timed(&BoxTree);

            println!(
                "{name}: {} faces, {} rays: exhaustive {exhaustive:.2} ms, box-tree {building:.2} ms with \
                 building its tree, {kept:.2} ms with the tree kept",
                body.faces.len(),
                rays.len()
            );
            assert!(matches!(reference, Attempt::Done(_)), "{name}: {reference:?}");
            assert_eq!(first, reference, "{name}");
            assert_eq!(second, reference, "{name}");
        }
    }
}
