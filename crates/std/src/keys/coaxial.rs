//! `coaxial:` — the other cylinders about the same axis: the bore of a tube is
//! `coaxial:` its outside, and the two are related.

use super::word;
use crate::surface::Cylinder;
use krly3d_core::math::frame::{origin, z_axis};
use krly3d_core::{Accuracy, Attempt, Id};
use krly3d_kernel::ports::keys::{FaceKeys, Keys, KeysRequest};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::{Context, Plugin};

pub struct CoaxialKey;

impl Plugin<Keys> for CoaxialKey {
    fn id(&self) -> Id {
        Id::new("coaxial")
    }

    fn call(&self, cx: &Context, request: &KeysRequest) -> Attempt<FaceKeys> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        let cylinder = |surface: &dyn Surface| surface.as_any().downcast_ref::<Cylinder>().copied();
        let Some(own) = cylinder(face.surface.as_ref()) else {
            return Attempt::declined("only cylinders are looked at for a common axis");
        };
        let mut found = FaceKeys::default();
        for (other, data) in request.body.faces.iter().enumerate() {
            let shares =
                cylinder(data.surface.as_ref()).is_some_and(|other| same_axis(&own, &other, cx.accuracy()));
            if other != request.face && shares {
                found.keys.push(format!("coaxial:{}", word(&data.name)));
                found.related.push(other);
            }
        }
        Attempt::Done(found)
    }
}

/// Whether the axes of two cylinders are one line: parallel, and a point of one lies on
/// the other.
fn same_axis(one: &Cylinder, other: &Cylinder, accuracy: &Accuracy) -> bool {
    let (along, between) = (z_axis(one.frame()), origin(other.frame()) - origin(one.frame()));
    accuracy.parallel(&along, &z_axis(other.frame())) && accuracy.is_zero_length(between.cross(&along).norm())
}

#[cfg(test)]
mod tests {
    use crate::keys::testing::said;
    use crate::tool::testing::{context, made};
    use krly3d_kernel::schema::Params;

    #[test]
    fn the_bore_and_the_outside_of_a_tube_name_each_other() {
        let cx = context();
        let section = Params::new().with("path", "M 2,0 L 5,0 L 5,4 L 2,4 Z").with("plane", "xz");
        let tube = made(&cx, "revolve", Params::new(), &[made(&cx, "profile", section, &[])]);
        let (outside, _) = tube.face("s2").unwrap();

        let of_bore = said(&cx, "coaxial", &tube, "s4").unwrap();

        assert_eq!(of_bore.keys, ["coaxial:s2"]);
        assert_eq!(of_bore.related, [outside]);
        assert_eq!(said(&cx, "coaxial", &tube, "s2").unwrap().keys, ["coaxial:s4"]);
    }

    #[test]
    fn cylinders_side_by_side_do_not_share_an_axis() {
        let cx = context();
        let peg = made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 6.0), &[]);
        let row = made(&cx, "pattern", Params::new().with("n", 3.0).with("step", [8.0, 0.0, 0.0]), &[peg]);

        let of_first = said(&cx, "coaxial", &row, "side[1]").unwrap();

        assert!(of_first.keys.is_empty() && of_first.related.is_empty());
    }

    #[test]
    fn cylinders_stacked_on_one_axis_share_it() {
        let cx = context();
        let peg = made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 6.0), &[]);
        let stack = made(&cx, "pattern", Params::new().with("n", 2.0).with("step", [0.0, 0.0, 6.0]), &[peg]);

        assert_eq!(said(&cx, "coaxial", &stack, "side[1]").unwrap().keys, ["coaxial:side[2]"]);
    }

    #[test]
    fn a_flat_face_is_not_looked_at() {
        let cx = context();
        let peg = made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 6.0), &[]);

        assert_eq!(said(&cx, "coaxial", &peg, "top"), None);
    }
}
