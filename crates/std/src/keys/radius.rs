//! `r:` — the radius of a round face: `r:3` for a cylinder or a sphere of radius 3.

use crate::surface::{Cylinder, Sphere};
use krly3d_core::{text, Attempt, Id};
use krly3d_kernel::ports::keys::{FaceKeys, Keys, KeysRequest};
use krly3d_kernel::{Context, Plugin};

pub struct RadiusKey;

impl Plugin<Keys> for RadiusKey {
    fn id(&self) -> Id {
        Id::new("r")
    }

    fn call(&self, _: &Context, request: &KeysRequest) -> Attempt<FaceKeys> {
        let surface = match request.target() {
            Ok(face) => face.surface.as_any(),
            Err(error) => return Attempt::Failed(error),
        };
        let radius = surface
            .downcast_ref::<Cylinder>()
            .map(Cylinder::radius)
            .or_else(|| surface.downcast_ref::<Sphere>().map(Sphere::radius));
        match radius {
            Some(radius) => Attempt::Done(FaceKeys {
                keys: vec![format!("r:{}", text::num(radius))],
                related: Vec::new(),
            }),
            None => Attempt::declined("only a cylinder and a sphere have one radius"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::keys::testing::keys;
    use crate::tool::testing::{context, made};
    use krly3d_kernel::schema::Params;

    #[test]
    fn a_cylinder_and_a_sphere_are_keyed_by_their_radius() {
        let cx = context();
        let can = made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[]);
        let ball = made(&cx, "sphere", Params::new().with("r", 2.5), &[]);

        assert_eq!(keys(&cx, "r", &can, "side"), ["r:3"]);
        assert_eq!(keys(&cx, "r", &ball, "skin"), ["r:2.5"]);
    }

    #[test]
    fn a_face_without_a_single_radius_has_no_such_key() {
        let cx = context();
        let can = made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[]);
        let ring = made(&cx, "torus", Params::new().with("ring", 6.0).with("tube", 2.0), &[]);

        assert!(keys(&cx, "r", &can, "top").is_empty());
        assert!(keys(&cx, "r", &ring, "skin").is_empty());
    }
}
