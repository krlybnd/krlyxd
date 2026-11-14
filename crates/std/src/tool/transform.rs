//! A body moved, turned and scaled.

use krly3d_core::math::frame::turning;
use krly3d_core::math::{Sim3, Vec3};
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Params, Schema};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};
use nalgebra::{Rotation3, Translation3, Unit};

pub struct Transform;

impl Plugin<Tool> for Transform {
    fn id(&self) -> Id {
        Id::new("transform")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a copy of a body scaled about a pivot, then turned about it (by `turn` about `about`, or so that the direction `align` points along `onto`), then moved",
            inputs: (1, 1),
            params: vec![
                Param::optional("move", Kind::Triple, [0.0, 0.0, 0.0], "shift applied last"),
                Param::optional("turn", Kind::Number, 0.0, "angle in degrees"),
                Param::optional(
                    "about",
                    Kind::Triple,
                    [0.0, 0.0, 1.0],
                    "direction of the axis to turn about",
                ),
                Param::optional(
                    "pivot",
                    Kind::Triple,
                    [0.0, 0.0, 0.0],
                    "point the axis passes through and scaling keeps fixed",
                ),
                Param::optional("scale", Kind::Number, 1.0, "uniform scale factor, above zero"),
                Param::alternative(
                    "align",
                    Kind::Triple,
                    "a direction of the body that is to point along `onto`; the smallest such turn is taken, in place of `turn`",
                ),
                Param::alternative("onto", Kind::Triple, "the direction `align` is turned to"),
            ],
        })
    }

    fn call(&self, _: &Context, request: &ToolRequest) -> Attempt<Body> {
        let moved =
            similarity(&request.params).map(|by| request.inputs.first().map(|body| body.transformed(&by)));
        match moved {
            Ok(Some(body)) => Attempt::Done(body),
            Ok(None) => Attempt::Failed(Error::input("transform takes one body")),
            Err(error) => Attempt::Failed(error),
        }
    }
}

/// `x ↦ move + pivot + R·s·(x − pivot)` as one similarity.
pub(crate) fn similarity(params: &Params) -> Result<Sim3, Error> {
    let (shift, pivot, scale) = (params.vector("move")?, params.vector("pivot")?, params.number("scale")?);
    if !(scale.is_finite() && scale > 0.0) {
        return Err(Error::input(format!("scale must be above zero, got {scale}")));
    }
    let rotation = match (params.get("align"), params.get("onto")) {
        (None, None) => turn(&params.vector("about")?, params.number("turn")?)?,
        (Some(_), Some(_)) if params.number("turn")? == 0.0 => {
            turning(&params.vector("align")?, &params.vector("onto")?)?
        }
        (Some(_), Some(_)) => {
            return Err(Error::input("give the turn by `turn` or by `align` and `onto`, not both"))
        }
        _ => {
            return Err(Error::input(
                "`align` and `onto` go together: the direction and where it is to point",
            ))
        }
    };
    let translation = shift + pivot - rotation * pivot * scale;
    Ok(Sim3::from_parts(Translation3::from(translation), rotation, scale))
}

pub(crate) fn turn(about: &Vec3, degrees: f64) -> Result<Rotation3<f64>, Error> {
    let axis = Unit::try_new(*about, 0.0)
        .ok_or_else(|| Error::input(format!("the axis to turn about needs a direction, got {about:?}")))?;
    Ok(Rotation3::from_axis_angle(&axis, degrees.to_radians()))
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, context, face_names, made, volume};
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::{Attempt, Id};
    use krly3d_kernel::ports::tool::run;
    use krly3d_kernel::schema::Params;
    use std::sync::Arc;

    #[test]
    fn a_moved_body_keeps_its_names_and_volume() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 2.0).with("d", 4.0).with("h", 6.0), &[]);

        let moved =
            made(&cx, "transform", Params::new().with("move", [10.0, 0.0, 0.0]).with("turn", 30.0), &[block]);

        assert_eq!(face_names(&moved), ["back", "bottom", "front", "left", "right", "top"]);
        assert!(agree(volume(&cx, &moved), 48.0));
    }

    #[test]
    fn scaling_about_a_pivot_keeps_the_pivot_in_place() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 2.0).with("d", 2.0).with("h", 2.0), &[]);

        // The corner (1, 1, 0) is the pivot of a quarter turn and a doubling.
        let params = Params::new().with("pivot", [1.0, 1.0, 0.0]).with("turn", 90.0).with("scale", 2.0);
        let scaled = made(&cx, "transform", params, &[block]);

        assert!(scaled.vertices.iter().any(|point| (point - Point3::new(1.0, 1.0, 0.0)).norm() < 1e-12));
        assert!(agree(volume(&cx, &scaled), 64.0));
    }

    #[test]
    fn align_onto_turns_a_direction_of_the_body_onto_the_one_given() {
        let cx = context();
        let rod = made(&cx, "cylinder", Params::new().with("r", 1.0).with("h", 6.0), &[]);
        let params = Params::new().with("align", [0.0, 0.0, 1.0]).with("onto", [-1.0, 1.0, 1.0]);

        let turned = made(&cx, "transform", params, &[rod]);

        let end = Point3::from(Vec3::new(-1.0, 1.0, 1.0).normalize() * 6.0);
        assert!(turned.face("top").unwrap().1.surface.distance(&end).abs() < 1e-12 * 10.0);
    }

    #[test]
    fn a_turn_given_two_ways_or_half_given_is_refused() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 1.0).with("d", 1.0).with("h", 1.0), &[]);
        let both =
            Params::new().with("align", [0.0, 0.0, 1.0]).with("onto", [1.0, 0.0, 0.0]).with("turn", 30.0);
        let half = Params::new().with("align", [0.0, 0.0, 1.0]);

        for params in [both, half] {
            let attempt = run(&cx, &Id::new("transform"), &params, &[Arc::clone(&block)]);
            assert!(matches!(attempt, Attempt::Failed(_)), "{params:?} gave {attempt:?}");
        }
    }
}
