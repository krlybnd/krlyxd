//! A rectangular profile, centred on the origin of its plane.

use super::placement;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::tool::{run, Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Params, Schema, Value};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};

pub struct Rectangle;

impl Plugin<Tool> for Rectangle {
    fn id(&self) -> Id {
        Id::new("rectangle")
    }

    fn schema(&self) -> Option<Schema> {
        let [plane, at] = placement();
        Some(Schema {
            about: "a rectangular profile centred on the origin of its plane",
            inputs: (0, 0),
            params: vec![
                Param::required("w", Kind::Number, "size along the first axis of the plane"),
                Param::required("d", Kind::Number, "size along the second axis of the plane"),
                plane,
                at,
                Param::optional(
                    "names",
                    Kind::Texts,
                    ["front", "right", "back", "left"].map(Value::from).to_vec(),
                    "names of the four sides, starting with the one towards −y and going counter-clockwise",
                ),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        let params = &request.params;
        let (half_w, half_d) = match (params.number("w"), params.number("d")) {
            (Ok(w), Ok(d)) => (0.5 * w, 0.5 * d),
            (Err(error), _) | (_, Err(error)) => return Attempt::Failed(error),
        };
        let path = format!(
            "M {},{} L {half_w},{} L {half_w},{half_d} L {},{half_d} Z",
            -half_w, -half_d, -half_d, -half_w
        );
        let mut drawn = Params::new().with("path", path.as_str());
        for name in ["plane", "at", "names"] {
            if let Some(value) = params.get(name) {
                drawn.set(name, value.clone());
            }
        }
        run(cx, &Id::new("profile"), &drawn, &[])
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, made};
    use krly3d_kernel::schema::Params;

    #[test]
    fn a_rectangle_has_four_named_sides_and_the_area_of_its_sizes() {
        let cx = context();

        let rectangle = made(&cx, "rectangle", Params::new().with("w", 40.0).with("d", 10.0), &[]);

        let labels: Vec<&str> = rectangle.edges.iter().filter_map(|edge| edge.label.as_deref()).collect();
        assert_eq!(labels, ["front", "right", "back", "left"]);
        assert!(agree(area(&cx, &rectangle), 400.0));
    }
}
