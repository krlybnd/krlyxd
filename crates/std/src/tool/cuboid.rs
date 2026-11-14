//! The box: a rectangle pushed up. The file is named after the solid because `box` is a
//! reserved word.

use super::{carry, recipe};
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Schema};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};

pub struct Cuboid;

impl Plugin<Tool> for Cuboid {
    fn id(&self) -> Id {
        Id::new("box")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a block standing on the ground plane, its base centred on `at`; faces: bottom, top, front (−y), back, left (−x), right",
            inputs: (0, 0),
            params: vec![
                Param::required("w", Kind::Number, "width along x"),
                Param::required("d", Kind::Number, "depth along y"),
                Param::required("h", Kind::Number, "height along z"),
                Param::optional("at", Kind::Triple, [0.0, 0.0, 0.0], "centre of the base"),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        let given = &request.params;
        recipe(cx, vec![("rectangle", carry(given, &["w", "d", "at"])), ("extrude", carry(given, &["h"]))])
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, face_names, made, volume};
    use krly3d_kernel::schema::Params;

    #[test]
    fn a_box_has_the_volume_and_area_of_its_three_sizes() {
        let cx = context();

        let block = made(&cx, "box", Params::new().with("w", 40.0).with("d", 10.0).with("h", 20.0), &[]);

        assert_eq!(face_names(&block), ["back", "bottom", "front", "left", "right", "top"]);
        assert!(agree(volume(&cx, &block), 8000.0));
        assert!(agree(area(&cx, &block), 2800.0));
    }

    #[test]
    fn a_box_stands_on_its_base_point() {
        let cx = context();
        let params =
            Params::new().with("w", 2.0).with("d", 4.0).with("h", 6.0).with("at", [10.0, 20.0, 30.0]);

        let block = made(&cx, "box", params, &[]);

        let low = block
            .vertices
            .iter()
            .fold([f64::MAX; 3], |low, p| [low[0].min(p.x), low[1].min(p.y), low[2].min(p.z)]);
        let high = block
            .vertices
            .iter()
            .fold([f64::MIN; 3], |high, p| [high[0].max(p.x), high[1].max(p.y), high[2].max(p.z)]);
        assert_eq!((low, high), ([9.0, 18.0, 30.0], [11.0, 22.0, 36.0]));
    }
}
