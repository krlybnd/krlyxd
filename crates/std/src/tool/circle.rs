//! A circular profile, centred on the origin of its plane.

use super::placement;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::tool::{run, Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Params, Schema};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};

pub struct Circle;

impl Plugin<Tool> for Circle {
    fn id(&self) -> Id {
        Id::new("circle")
    }

    fn schema(&self) -> Option<Schema> {
        let [plane, at] = placement();
        Some(Schema {
            about: "a circular profile centred on the origin of its plane",
            inputs: (0, 0),
            params: vec![
                Param::required("r", Kind::Number, "radius"),
                plane,
                at,
                Param::optional("name", Kind::Text, "side", "name of the edge"),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        let params = &request.params;
        let (radius, name) = match (params.number("r"), params.text("name")) {
            (Ok(radius), Ok(name)) => (radius, name),
            (Err(error), _) | (_, Err(error)) => return Attempt::Failed(error),
        };
        let path = format!("O 0,0 {radius}");
        let mut drawn = Params::new().with("path", path.as_str()).with("names", name);
        for name in ["plane", "at"] {
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
    use std::f64::consts::PI;

    #[test]
    fn a_circle_is_one_closed_edge_around_a_disc() {
        let cx = context();

        let circle = made(&cx, "circle", Params::new().with("r", 3.0), &[]);

        assert_eq!((circle.edges.len(), circle.vertices.len()), (1, 0));
        assert!(agree(area(&cx, &circle), 9.0 * PI));
    }
}
