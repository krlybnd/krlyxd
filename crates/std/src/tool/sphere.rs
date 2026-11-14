//! The sphere: a half disc turned about its straight side.

use super::{carry, recipe};
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Params, Schema, Value};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};

pub struct Sphere;

impl Plugin<Tool> for Sphere {
    fn id(&self) -> Id {
        Id::new("sphere")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a ball centred on `at`; its one face is named skin",
            inputs: (0, 0),
            params: vec![
                Param::required("r", Kind::Number, "radius"),
                Param::optional("at", Kind::Triple, [0.0, 0.0, 0.0], "centre"),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        let radius = match request.params.number("r") {
            Ok(radius) => radius,
            Err(error) => return Attempt::Failed(error),
        };
        // From the south pole around the outside to the north pole, then back along the axis.
        let path = format!("M 0,{} A 0,0 0,{radius} ccw Z", -radius);
        let names: Vec<Value> = ["skin", "axis"].map(Value::from).to_vec();
        let section = carry(&request.params, &["at"])
            .with("path", path.as_str())
            .with("plane", "xz")
            .with("names", names);
        recipe(cx, vec![("profile", section), ("revolve", Params::new())])
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, face_names, made, volume};
    use krly3d_kernel::schema::Params;
    use std::f64::consts::PI;

    #[test]
    fn a_sphere_is_one_face_without_boundary_and_has_the_closed_form_content() {
        let cx = context();

        let ball = made(&cx, "sphere", Params::new().with("r", 5.0).with("at", [3.0, -2.0, 7.0]), &[]);

        assert_eq!(face_names(&ball), ["skin"]);
        assert_eq!((ball.edges.len(), ball.faces[0].loops.len()), (0, 0));
        assert!(agree(volume(&cx, &ball), 500.0 * PI / 3.0));
        assert!(agree(area(&cx, &ball), 100.0 * PI));
    }
}
