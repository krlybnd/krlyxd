//! Trim rules: whether a point of a face's surface belongs to the face.

pub(crate) mod crossing;
pub mod nurbs;
pub mod plane;
pub mod walk;

use krly3d_core::math::Point3;
use krly3d_core::Accuracy;
use krly3d_kernel::ports::trim::Trim;
use krly3d_kernel::topo::{Body, Face};
use krly3d_kernel::KernelBuilder;

pub fn register(builder: &mut KernelBuilder) {
    builder.plugin::<Trim>(plane::PlaneTrim).plugin::<Trim>(nurbs::NurbsTrim).plugin::<Trim>(walk::WalkTrim);
}

/// Points along each loop of a face: both ends and three inner points of every edge.
pub(crate) fn loop_points(body: &Body, face: &Face) -> Vec<Vec<Point3>> {
    face.loops
        .iter()
        .map(|uses| {
            uses.iter()
                .flat_map(|used| [0.0, 0.25, 0.5, 0.75].map(|fraction| body.edges[used.edge].point(fraction)))
                .collect()
        })
        .collect()
}

/// Whether the point lies on a boundary edge of the face.
fn on_boundary(body: &Body, face: &Face, point: &Point3, accuracy: &Accuracy) -> bool {
    face.uses().any(|used| {
        let edge = &body.edges[used.edge];
        accuracy.same_point(&edge.curve.point(edge.nearest(point)), point)
    })
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{context, made};
    use krly3d_core::math::Point3;
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::trim::{contract, place, Place, TrimRequest};
    use krly3d_kernel::schema::Params;
    use std::sync::Arc;

    #[test]
    fn every_face_of_every_kind_of_solid_satisfies_the_trim_contract() {
        let cx = context();
        let upright = |path: &str| Params::new().with("path", path).with("plane", "xz");
        let tube = made(&cx, "profile", upright("M 2,0 L 5,0 L 5,4 L 2,4 Z"), &[]);
        let disc = made(&cx, "profile", upright("O 6,0 2"), &[]);
        let plate = made(
            &cx,
            "profile",
            Params::new().with("path", "M -30,-20 L 30,-20 L 30,20 L -30,20 Z O 20,15 4"),
            &[],
        );
        let solids = [
            made(&cx, "box", Params::new().with("w", 40.0).with("d", 10.0).with("h", 20.0), &[]),
            made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[]),
            made(&cx, "cone", Params::new().with("r", 4.0).with("h", 9.0), &[]),
            made(&cx, "cone", Params::new().with("r", 4.0).with("h", 9.0).with("top", 2.0), &[]),
            made(&cx, "revolve", Params::new(), &[Arc::clone(&tube)]),
            made(&cx, "revolve", Params::new().with("angle", 250.0), &[tube]),
            made(&cx, "revolve", Params::new().with("angle", 200.0), &[disc]),
            made(&cx, "extrude", Params::new().with("h", 10.0), &[plate]),
        ];
        for solid in &solids {
            for face in 0..solid.faces.len() {
                contract::check(&cx, solid, face, 0.01).unwrap();
            }
        }
    }

    #[test]
    fn every_point_of_a_face_without_boundary_is_inside_it() {
        let cx = context();
        let ball = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);

        let found = place(&cx, &TrimRequest { body: ball, face: 0, point: Point3::new(0.0, 5.0, 0.0) });

        assert_eq!(found, Attempt::Done(Place::Inside));
    }

    #[test]
    fn a_point_in_a_hole_of_a_flat_face_is_outside_the_face() {
        let cx = context();
        let outline = Params::new().with("path", "M -30,-20 L 30,-20 L 30,20 L -30,20 Z O 20,15 4");
        let plate = made(&cx, "profile", outline, &[]);
        let at = |x: f64, y: f64| {
            place(&cx, &TrimRequest { body: Arc::clone(&plate), face: 0, point: Point3::new(x, y, 0.0) })
        };

        assert_eq!(at(0.0, 0.0), Attempt::Done(Place::Inside));
        assert_eq!(at(20.0, 15.0), Attempt::Done(Place::Outside));
        assert_eq!(at(24.0, 15.0), Attempt::Done(Place::Boundary));
        assert_eq!(at(31.0, 0.0), Attempt::Done(Place::Outside));
    }
}
