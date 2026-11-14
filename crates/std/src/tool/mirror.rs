//! A body reflected in a plane.

use krly3d_core::math::mirror::Mirror as Reflection;
use krly3d_core::math::Point3;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Schema};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};

pub struct Mirror;

impl Plugin<Tool> for Mirror {
    fn id(&self) -> Id {
        Id::new("mirror")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a copy of a body reflected in a plane; faces keep their names",
            inputs: (1, 1),
            params: vec![
                Param::optional("normal", Kind::Triple, [1.0, 0.0, 0.0], "normal of the mirror plane"),
                Param::optional("at", Kind::Triple, [0.0, 0.0, 0.0], "a point of the mirror plane"),
            ],
        })
    }

    fn call(&self, _: &Context, request: &ToolRequest) -> Attempt<Body> {
        let [body] = request.inputs.as_slice() else {
            return Attempt::Failed(Error::input("mirror takes one body"));
        };
        let reflection = request.params.point("at").and_then(|at: Point3| {
            request.params.vector("normal").and_then(|normal| Reflection::new(at, normal))
        });
        match reflection {
            Ok(reflection) => body.mirrored(&reflection).map_or_else(
                || Attempt::declined("the body has a curve or surface that cannot be reflected"),
                Attempt::Done,
            ),
            Err(error) => Attempt::Failed(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, made, volume};
    use krly3d_kernel::schema::{Params, Value};
    use krly3d_kernel::topo::body_bounds;

    #[test]
    fn a_mirrored_cone_keeps_its_volume_and_area_and_points_the_other_way() {
        let cx = context();
        let cone =
            made(&cx, "cone", Params::new().with("r", 3.0).with("h", 6.0).with("at", [5.0, 1.0, 2.0]), &[]);

        let image =
            made(&cx, "mirror", Params::new().with("normal", [0.0, 0.0, 1.0]), std::slice::from_ref(&cone));

        assert!(agree(volume(&cx, &image), volume(&cx, &cone)));
        assert!(agree(area(&cx, &image), area(&cx, &cone)));
        let (before, after) =
            (body_bounds(&cx, &cone).into_result().unwrap(), body_bounds(&cx, &image).into_result().unwrap());
        assert!(agree(after.low.z, -before.high.z) && agree(after.high.z, -before.low.z));
    }

    #[test]
    fn a_filleted_block_mirrored_in_a_slanted_plane_stays_valid_and_keeps_its_names() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0), &[]);
        let round = made(
            &cx,
            "fillet",
            Params::new().with("edges", Value::texts(["front|top"])).with("r", 0.5),
            &[block],
        );

        let image = made(
            &cx,
            "mirror",
            Params::new().with("normal", [1.0, 2.0, -0.5]).with("at", [3.0, 0.0, 1.0]),
            std::slice::from_ref(&round),
        );

        assert!(agree(volume(&cx, &image), volume(&cx, &round)));
        assert!(image.face("fillet(front|top)").is_some());
    }
}
