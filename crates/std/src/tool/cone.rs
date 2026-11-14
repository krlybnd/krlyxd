//! The cone and the frustum: an upright triangle or trapezium turned about its axis.

use super::{recipe, standing, standing_from};
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Params, Schema, Value};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};

pub struct Cone;

impl Plugin<Tool> for Cone {
    fn id(&self) -> Id {
        Id::new("cone")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a cone, or a frustum when `top` is given, its base centred on `at`, upright or along `axis`, or reaching to `to`; faces: bottom, side, and top for a frustum",
            inputs: (0, 0),
            params: [
                vec![
                    Param::required("r", Kind::Number, "radius of the base"),
                    Param::alternative("h", Kind::Number, "height along the axis"),
                    Param::optional("top", Kind::Number, 0.0, "radius at the top; 0 ends in a point"),
                ],
                standing(true),
            ]
            .concat(),
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        let given = &request.params;
        let placed = match standing_from(given, cx.accuracy()) {
            Ok(placed) => placed,
            Err(error) => return Attempt::Failed(error),
        };
        let (radius, height, top) = match (given.number("r"), placed.height(given), given.number("top")) {
            (Ok(radius), Ok(height), Ok(top)) => (radius, height, top),
            (Err(error), ..) | (_, Err(error), _) | (.., Err(error)) => return Attempt::Failed(error),
        };
        // Drawn as (distance from the axis, height); the last side lies on the axis.
        let (path, names) = if top == 0.0 {
            (format!("M 0,0 L {radius},0 L 0,{height} Z"), vec!["bottom", "side", "axis"])
        } else {
            (
                format!("M 0,0 L {radius},0 L {top},{height} L 0,{height} Z"),
                vec!["bottom", "side", "top", "axis"],
            )
        };
        let names: Vec<Value> = names.into_iter().map(Value::from).collect();
        let section = Params::new().with("path", path.as_str()).with("plane", "xz").with("names", names);
        placed.stand(recipe(cx, vec![("profile", section), ("revolve", Params::new())]))
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, face_names, made, volume};
    use krly3d_kernel::schema::Params;
    use std::f64::consts::PI;

    #[test]
    fn a_cone_has_the_closed_form_volume_and_area() {
        let cx = context();

        let cone = made(&cx, "cone", Params::new().with("r", 4.0).with("h", 9.0), &[]);

        assert_eq!(face_names(&cone), ["bottom", "side"]);
        assert!(agree(volume(&cx, &cone), 48.0 * PI));
        assert!(agree(area(&cx, &cone), PI * 4.0 * (4.0 + 97.0_f64.sqrt())));
    }

    #[test]
    fn a_frustum_has_the_closed_form_volume_and_area() {
        let cx = context();

        let frustum = made(&cx, "cone", Params::new().with("r", 4.0).with("h", 9.0).with("top", 2.0), &[]);

        assert_eq!(face_names(&frustum), ["bottom", "side", "top"]);
        assert!(agree(volume(&cx, &frustum), 84.0 * PI));
        assert!(agree(area(&cx, &frustum), PI * (20.0 + 6.0 * 85.0_f64.sqrt())));
    }
}
