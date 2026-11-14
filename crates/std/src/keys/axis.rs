//! `axis:` — the direction of the axis of a face turned about one: `axis:+z` for an
//! upright cylinder, cone or torus.

use super::direction;
use crate::surface::{Cone, Cylinder, Torus};
use krly3d_core::math::frame::z_axis;
use krly3d_core::math::Vec3;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::keys::{FaceKeys, Keys, KeysRequest};
use krly3d_kernel::{Context, Plugin};

pub struct AxisKey;

impl Plugin<Keys> for AxisKey {
    fn id(&self) -> Id {
        Id::new("axis")
    }

    fn call(&self, cx: &Context, request: &KeysRequest) -> Attempt<FaceKeys> {
        let surface = match request.target() {
            Ok(face) => face.surface.as_any(),
            Err(error) => return Attempt::Failed(error),
        };
        let frame = surface
            .downcast_ref::<Cylinder>()
            .map(Cylinder::frame)
            .or_else(|| surface.downcast_ref::<Cone>().map(Cone::frame))
            .or_else(|| surface.downcast_ref::<Torus>().map(Torus::frame));
        match frame {
            Some(frame) => {
                let along = one_way(&z_axis(frame), cx.accuracy().angle());
                Attempt::Done(FaceKeys {
                    keys: vec![format!("axis:{}", direction(&along))],
                    related: Vec::new(),
                })
            }
            None => Attempt::declined("only a cylinder, a cone and a torus have an axis"),
        }
    }
}

/// An axis runs both ways, so of its two directions the one whose first component that
/// is not zero is positive stands for both: `+z`, never `-z`.
fn one_way(axis: &Vec3, zero: f64) -> Vec3 {
    match axis.iter().find(|component| component.abs() > zero) {
        Some(first) if *first < 0.0 => -axis,
        _ => *axis,
    }
}

#[cfg(test)]
mod tests {
    use crate::keys::testing::keys;
    use crate::tool::testing::{context, made};
    use krly3d_kernel::schema::Params;

    #[test]
    fn a_cylinder_a_cone_and_a_torus_are_keyed_by_the_direction_of_their_axis() {
        let cx = context();
        let can = made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[]);
        let cone = made(&cx, "cone", Params::new().with("r", 4.0).with("h", 9.0), &[]);
        let ring = made(&cx, "torus", Params::new().with("ring", 6.0).with("tube", 2.0), &[]);

        assert_eq!(keys(&cx, "axis", &can, "side"), ["axis:+z"]);
        assert_eq!(keys(&cx, "axis", &cone, "side"), ["axis:+z"]);
        assert_eq!(keys(&cx, "axis", &ring, "skin"), ["axis:+z"]);
    }

    #[test]
    fn an_axis_is_keyed_the_same_whichever_way_it_points() {
        let cx = context();
        let can = made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[]);
        let laid = |degrees: f64| {
            let turn = Params::new().with("turn", degrees).with("about", [0.0, 1.0, 0.0]);
            made(&cx, "transform", turn, std::slice::from_ref(&can))
        };

        assert_eq!(keys(&cx, "axis", &laid(90.0), "side"), ["axis:+x"]);
        assert_eq!(keys(&cx, "axis", &laid(-90.0), "side"), ["axis:+x"]);
    }

    #[test]
    fn a_flat_face_and_a_ball_have_no_axis() {
        let cx = context();
        let can = made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[]);
        let ball = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);

        assert!(keys(&cx, "axis", &can, "top").is_empty());
        assert!(keys(&cx, "axis", &ball, "skin").is_empty());
    }
}
