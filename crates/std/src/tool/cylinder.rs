//! The cylinder: a circle pushed up.

use super::{carry, recipe, standing, standing_from};
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Params, Schema};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};

pub struct Cylinder;

impl Plugin<Tool> for Cylinder {
    fn id(&self) -> Id {
        Id::new("cylinder")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a cylinder with its base centred on `at`, upright or along `axis`, or reaching to `to`; faces: bottom, top, side",
            inputs: (0, 0),
            params: [
                vec![
                    Param::required("r", Kind::Number, "radius"),
                    Param::alternative("h", Kind::Number, "height along the axis"),
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
        let height = match placed.height(given) {
            Ok(height) => height,
            Err(error) => return Attempt::Failed(error),
        };
        placed.stand(recipe(
            cx,
            vec![("circle", carry(given, &["r"])), ("extrude", Params::new().with("h", height))],
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, face_names, made, volume};
    use krly3d_core::math::Point3;
    use krly3d_core::{Attempt, Id};
    use krly3d_kernel::ports::tool::run;
    use krly3d_kernel::schema::Params;
    use std::f64::consts::PI;

    #[test]
    fn a_cylinder_has_the_closed_form_volume_and_area() {
        let cx = context();

        let cylinder = made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[]);

        assert_eq!(face_names(&cylinder), ["bottom", "side", "top"]);
        assert!(agree(volume(&cx, &cylinder), 90.0 * PI));
        assert!(agree(area(&cx, &cylinder), 78.0 * PI));
    }

    #[test]
    fn a_cylinder_reaching_to_a_point_has_its_ends_exactly_there() {
        let cx = context();
        let (from, to) = ([8.0, -8.0, 22.0], [20.0, -20.0, 10.0]);

        let rod = made(&cx, "cylinder", Params::new().with("r", 0.8).with("at", from).with("to", to), &[]);

        // Each end lies on its cap and on the axis, a radius from the side: the cap centres.
        let tolerance = 1e-12 * 30.0;
        for (cap, point) in [("bottom", from), ("top", to)] {
            let point = Point3::from(point);
            assert!(rod.face(cap).unwrap().1.surface.distance(&point).abs() < tolerance, "{cap}");
            assert!(
                (rod.face("side").unwrap().1.surface.distance(&point).abs() - 0.8).abs() < tolerance,
                "{cap}"
            );
        }
        assert!(agree(volume(&cx, &rod), PI * 0.64 * 12.0 * 3.0_f64.sqrt()));
    }

    #[test]
    fn a_cylinder_along_an_axis_keeps_its_closed_form_volume() {
        let cx = context();

        let rod = made(
            &cx,
            "cylinder",
            Params::new().with("r", 2.0).with("h", 5.0).with("axis", [1.0, -2.0, 0.5]),
            &[],
        );

        assert!(agree(volume(&cx, &rod), 20.0 * PI));
    }

    #[test]
    fn two_ways_of_giving_the_same_thing_at_once_are_refused() {
        let cx = context();
        let base = || Params::new().with("r", 1.0).with("to", [0.0, 0.0, 4.0]);

        for params in
            [base().with("h", 4.0), base().with("axis", [0.0, 0.0, 1.0]), Params::new().with("r", 1.0)]
        {
            let attempt = run(&cx, &Id::new("cylinder"), &params, &[]);
            assert!(matches!(attempt, Attempt::Failed(_)), "{params:?} gave {attempt:?}");
        }
    }
}
