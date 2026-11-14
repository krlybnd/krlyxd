//! The torus: a disc turned about an axis beside it.

use super::{recipe, standing, standing_from};
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Params, Schema};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};

pub struct Torus;

impl Plugin<Tool> for Torus {
    fn id(&self) -> Id {
        Id::new("torus")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a ring about an axis through `at`, upright unless `axis` says otherwise; its one face is named skin",
            inputs: (0, 0),
            params: [
                vec![
                    Param::required("ring", Kind::Number, "radius of the circle the tube runs along"),
                    Param::required("tube", Kind::Number, "radius of the tube"),
                ],
                standing(false),
            ]
            .concat(),
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        let (ring, tube) = match (request.params.number("ring"), request.params.number("tube")) {
            (Ok(ring), Ok(tube)) => (ring, tube),
            (Err(error), _) | (_, Err(error)) => return Attempt::Failed(error),
        };
        let path = format!("O {ring},0 {tube}");
        let placed = match standing_from(&request.params, cx.accuracy()) {
            Ok(placed) => placed,
            Err(error) => return Attempt::Failed(error),
        };
        let section = Params::new().with("path", path.as_str()).with("plane", "xz").with("names", "skin");
        placed.stand(recipe(cx, vec![("profile", section), ("revolve", Params::new())]))
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, face_names, made, volume};
    use krly3d_kernel::schema::Params;
    use std::f64::consts::PI;

    #[test]
    fn a_torus_is_one_face_without_boundary_and_has_the_closed_form_content() {
        let cx = context();

        let ring = made(&cx, "torus", Params::new().with("ring", 6.0).with("tube", 2.0), &[]);

        assert_eq!(face_names(&ring), ["skin"]);
        assert_eq!((ring.edges.len(), ring.faces[0].loops.len()), (0, 0));
        assert!(agree(volume(&cx, &ring), 2.0 * PI * PI * 6.0 * 4.0));
        assert!(agree(area(&cx, &ring), 4.0 * PI * PI * 12.0));
    }
}
