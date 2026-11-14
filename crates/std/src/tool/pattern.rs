//! Copies of a body in a row or around an axis, as one body.

use super::transform::turn;
use krly3d_core::math::Sim3;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Schema};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};
use nalgebra::{Rotation3, Translation3};

pub struct Pattern;

impl Plugin<Tool> for Pattern {
    fn id(&self) -> Id {
        Id::new("pattern")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "n copies of a body as one body: each shifted by `step` and turned by `turn` more than the one before; faces are named face[k]",
            inputs: (1, 1),
            params: vec![
                Param::required("n", Kind::Count, "number of copies, the original included"),
                Param::optional("step", Kind::Triple, [0.0, 0.0, 0.0], "shift from one copy to the next"),
                Param::optional("turn", Kind::Number, 0.0, "angle in degrees from one copy to the next; 360/n spreads them around"),
                Param::optional("about", Kind::Triple, [0.0, 0.0, 1.0], "direction of the axis to turn about"),
                Param::optional("pivot", Kind::Triple, [0.0, 0.0, 0.0], "point the axis passes through"),
            ],
        })
    }

    fn call(&self, _: &Context, request: &ToolRequest) -> Attempt<Body> {
        copies(request).into()
    }
}

fn copies(request: &ToolRequest) -> Result<Body, Error> {
    let params = &request.params;
    let original = request.inputs.first().ok_or_else(|| Error::input("pattern takes one body"))?;
    let (step, pivot, degrees) = (params.vector("step")?, params.vector("pivot")?, params.number("turn")?);
    if step.norm() == 0.0 && degrees == 0.0 {
        return Err(Error::input(
            "pattern needs a `step` or a `turn`, or every copy would sit on the original",
        ));
    }
    let about = params.vector("about")?;
    let mut parts = Vec::new();
    for copy in 0..params.count("n")? {
        let rotation =
            if degrees == 0.0 { Rotation3::identity() } else { turn(&about, degrees * copy as f64)? };
        let translation = step * copy as f64 + pivot - rotation * pivot;
        parts.push(original.transformed(&Sim3::from_parts(Translation3::from(translation), rotation, 1.0)));
    }
    Ok(Body::merged(&parts, |copy, name| format!("{name}[{}]", copy + 1)))
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, context, made, volume};
    use krly3d_core::Id;
    use krly3d_kernel::ports::tool::run;
    use krly3d_kernel::schema::Params;
    use std::f64::consts::PI;

    #[test]
    fn copies_around_an_axis_are_one_body_with_numbered_faces() {
        let cx = context();
        let peg = made(
            &cx,
            "cylinder",
            Params::new().with("r", 1.0).with("h", 2.0).with("at", [5.0, 0.0, 0.0]),
            &[],
        );

        let ring = made(&cx, "pattern", Params::new().with("n", 6.0).with("turn", 60.0), &[peg]);

        assert_eq!(ring.faces.len(), 18);
        assert!(ring.face("side[1]").is_some() && ring.face("top[6]").is_some());
        assert!(agree(volume(&cx, &ring), 6.0 * 2.0 * PI));
    }

    #[test]
    fn copies_in_a_row_are_shifted_by_the_step() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 1.0).with("d", 1.0).with("h", 1.0), &[]);

        let row = made(&cx, "pattern", Params::new().with("n", 3.0).with("step", [4.0, 0.0, 0.0]), &[block]);

        let reach = row.vertices.iter().map(|point| point.x).fold(f64::MIN, f64::max);
        assert_eq!(reach, 8.5);
        assert!(agree(volume(&cx, &row), 3.0));
    }

    #[test]
    fn a_pattern_that_would_stack_every_copy_is_rejected() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 1.0).with("d", 1.0).with("h", 1.0), &[]);

        let error =
            run(&cx, &Id::new("pattern"), &Params::new().with("n", 3.0), &[block]).into_result().unwrap_err();

        assert!(error.message.starts_with("pattern needs a `step` or a `turn`"), "{error}");
    }
}
