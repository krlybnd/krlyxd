//! The mesh reading of a B-rep body, for any surface and any curve: what every kernel needs
//! to show, print or export a body, written once for all of them.
//!
//! Every edge is cut into points once, within half the tolerance, and both faces beside it
//! use exactly those points, so the mesh closes without gaps or T-junctions. Each face is
//! laid out in its parameters (`layout`), cut into triangles and refined until the surface
//! stays within the tolerance (`triangulate`); its triangles are turned to face outward.

mod edges;
mod layout;
mod triangulate;

use krly3d_core::math::Point3;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::mesh::Mesh;
use krly3d_kernel::ports::interpret::{MeshRequest, ToMesh};
use krly3d_kernel::ports::shape::Leaf;
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct BrepMesh;

impl Plugin<ToMesh> for BrepMesh {
    fn id(&self) -> Id {
        Id::new("brep")
    }

    fn call(&self, cx: &Context, request: &MeshRequest) -> Attempt<Arc<Mesh>> {
        let Some(leaf) = request.shape.as_any().downcast_ref::<Leaf>() else {
            return Attempt::declined("the brep mesh rule reads B-rep leaves");
        };
        if !(request.tolerance.is_finite() && request.tolerance > cx.accuracy().length()) {
            return Attempt::Failed(Error::input(format!(
                "a mesh tolerance must be above the length tolerance, got {}",
                request.tolerance
            )));
        }
        match meshed(cx, &leaf.body, request.tolerance) {
            Ok(mesh) => Attempt::Done(Arc::new(mesh)),
            Err(reason) => Attempt::Declined(reason),
        }
    }
}

fn meshed(cx: &Context, body: &Arc<Body>, tolerance: f64) -> Result<Mesh, String> {
    let mut points: Vec<Point3> = body.vertices.clone();
    let edges = edges::sample(body, 0.5 * tolerance, &mut points);
    let mut triangles = Vec::new();
    for (index, face) in body.faces.iter().enumerate() {
        let mut flat = layout::lay_out(cx, body, index, &edges, &mut points, 0.5 * tolerance)
            .map_err(|reason| format!("face `{}`: {reason}", face.name))?;
        let found = triangulate::triangles(&mut flat, face.surface.as_ref(), &mut points, tolerance)
            .map_err(|reason| format!("face `{}`: {reason}", face.name))?;
        let kept: Vec<[usize; 3]> = found
            .into_iter()
            .map(|corners| corners.map(|corner| flat.ids[corner]))
            .filter(|ids| ids[0] != ids[1] && ids[1] != ids[2] && ids[2] != ids[0])
            .collect();
        // The layout's triangles all turn the same way in the plane, so one sense holds for
        // the whole face. It is voted by area: a sliver's normal is too noisy to trust alone.
        let vote: f64 = kept
            .iter()
            .map(|ids| {
                let [a, b, c] = ids.map(|id| points[id]);
                let normal = (b - a).cross(&(c - a));
                let centre = Point3::from((a.coords + b.coords + c.coords) / 3.0);
                normal.dot(&face.outward_normal(&centre))
            })
            .sum();
        triangles
            .extend(kept.into_iter().map(|ids| if vote >= 0.0 { ids } else { [ids[0], ids[2], ids[1]] }));
    }
    Ok(Mesh { points, triangles })
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{area, context, made, volume};
    use krly3d_kernel::ports::interpret::contract::check_mesh;
    use krly3d_kernel::ports::interpret::mesh_of;
    use krly3d_kernel::ports::shape::Leaf;
    use krly3d_kernel::schema::{Params, Value};
    use krly3d_kernel::topo::Body;
    use krly3d_kernel::Context;
    use std::sync::Arc;

    fn closed_with_its_volume(cx: &Context, body: &Arc<Body>, tolerance: f64) {
        let shape = Leaf::of(Arc::clone(body));
        let kinds: Vec<&str> = body.faces.iter().map(|face| face.surface.classes()[0]).collect();
        if let Err(problem) = check_mesh(cx, &shape, tolerance, volume(cx, body), area(cx, body)) {
            panic!("a body of {kinds:?}: {problem}");
        }
    }

    fn numbers(values: &[f64]) -> Value {
        Value::List(values.iter().map(|value| (*value).into()).collect())
    }

    #[test]
    fn primitives_mesh_into_closed_meshes_holding_their_volume() {
        let cx = context();
        for (tool, params) in [
            ("box", Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0)),
            ("cylinder", Params::new().with("r", 3.0).with("h", 5.0)),
            ("cone", Params::new().with("r", 3.0).with("h", 5.0)),
            ("cone", Params::new().with("r", 3.0).with("h", 5.0).with("top", 1.0)),
            ("sphere", Params::new().with("r", 3.0)),
            ("torus", Params::new().with("ring", 4.0).with("tube", 1.0)),
            ("ellipsoid", Params::new().with("rx", 4.0).with("ry", 2.0).with("rz", 3.0)),
        ] {
            let body = made(&cx, tool, params, &[]);
            closed_with_its_volume(&cx, &body, 0.01);
        }
    }

    #[test]
    fn faces_with_holes_seams_and_cuts_mesh_closed() {
        let cx = context();
        let outline = Params::new().with("path", "M -30,-20 L 30,-20 L 30,20 L -30,20 Z O 20,15 4");
        let plate =
            made(&cx, "extrude", Params::new().with("h", 10.0), &[made(&cx, "profile", outline, &[])]);
        let rod = made(
            &cx,
            "cylinder",
            Params::new().with("r", 4.0).with("h", 30.0).with("at", [-10.0, 0.0, -10.0]),
            &[],
        );
        let drilled = made(&cx, "subtract", Params::new(), &[Arc::clone(&plate), rod]);
        let rounded = made(
            &cx,
            "fillet",
            Params::new().with("edges", Value::texts(["front|top"])).with("r", 2.0),
            &[made(&cx, "box", Params::new().with("w", 20.0).with("d", 10.0).with("h", 6.0), &[])],
        );
        let bar = made(
            &cx,
            "pipe",
            Params::new()
                .with("r", 1.0)
                .with("bend", 3.0)
                .with("points", numbers(&[0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0])),
            &[],
        );
        for body in [plate, drilled, rounded, bar] {
            closed_with_its_volume(&cx, &body, 0.02);
        }
    }

    #[test]
    fn a_loft_through_sections_meshes_closed() {
        let cx = context();
        let section = |path: &str, z: f64| {
            made(&cx, "profile", Params::new().with("path", path).with("at", [0.0, 0.0, z]), &[])
        };

        let vase = made(
            &cx,
            "loft",
            Params::new(),
            &[section("O 0,0 4", 0.0), section("O 0,0 6", 4.0), section("O 0,0 2", 9.0)],
        );

        closed_with_its_volume(&cx, &vase, 0.02);
    }

    #[test]
    fn a_finer_tolerance_gives_a_closer_volume() {
        let cx = context();
        let ball = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);
        let shape = Leaf::of(Arc::clone(&ball));
        let exact = volume(&cx, &ball);

        let coarse = mesh_of(&cx, &shape, 0.2).into_result().unwrap();
        let fine = mesh_of(&cx, &shape, 0.005).into_result().unwrap();

        assert!((fine.volume() - exact).abs() < (coarse.volume() - exact).abs());
        assert!(fine.triangles.len() > coarse.triangles.len());
    }

    #[test]
    fn a_mesh_asked_for_again_at_the_same_tolerance_is_the_one_made_before() {
        let cx = context();
        let ball = Leaf::of(made(&cx, "sphere", Params::new().with("r", 5.0), &[]));

        let first = mesh_of(&cx, &ball, 0.2).into_result().unwrap();
        let again = mesh_of(&cx, &ball, 0.2).into_result().unwrap();
        let finer = mesh_of(&cx, &ball, 0.1).into_result().unwrap();

        assert!(Arc::ptr_eq(&first, &again));
        assert!(!Arc::ptr_eq(&first, &finer));
    }

    /// A section of two circular arcs meeting at a corner at each end, as an airfoil is
    /// drawn, in the xz plane at `y`.
    fn lens(cx: &Context, front: f64, back: f64, thickness: f64, y: f64) -> Arc<Body> {
        let (half, middle) = (0.5 * thickness, 0.5 * (front + back));
        let drop = ((0.5 * (back - front)).powi(2) + half * half) / thickness - half;
        let path = format!("M {front},0 A {middle},{} {back},0 cw A {middle},{drop} {front},0 cw Z", -drop);
        made(
            cx,
            "profile",
            Params::new().with("path", path.as_str()).with("plane", "xz").with("at", [0.0, y, 0.0]),
            &[],
        )
    }

    #[test]
    fn a_loft_of_sharp_edged_sections_swept_hard_meshes_closed() {
        let cx = context();
        // The chine of an aircraft: its leading edge runs nearly along the span.
        let chine = made(
            &cx,
            "loft",
            Params::new(),
            &[lens(&cx, 4.6, 11.4, 0.12, -1.2), lens(&cx, 8.0, 11.4, 0.1, -1.86)],
        );

        closed_with_its_volume(&cx, &chine, 0.022);
    }
}
