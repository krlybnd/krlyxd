//! The ellipsoid: a ball stretched along three axes.

use super::{standing, standing_from};
use crate::surface::Ellipsoid as Surface;
use krly3d_core::math::frame::frame;
use krly3d_core::math::Vec3;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Schema};
use krly3d_kernel::topo::{Body, Face};
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct Ellipsoid;

impl Plugin<Tool> for Ellipsoid {
    fn id(&self) -> Id {
        Id::new("ellipsoid")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "an ellipsoid centred on `at` with semi-axes rx, ry along x and y and rz along `axis` (upright when left out); its one face is named skin",
            inputs: (0, 0),
            params: [
                vec![
                    Param::required("rx", Kind::Number, "semi-axis along x"),
                    Param::required("ry", Kind::Number, "semi-axis along y"),
                    Param::required("rz", Kind::Number, "semi-axis along the axis"),
                ],
                standing(false),
            ]
            .concat(),
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        made(cx, request).into()
    }
}

fn made(cx: &Context, request: &ToolRequest) -> Result<Body, Error> {
    let params = &request.params;
    let placed = standing_from(params, cx.accuracy())?;
    // Upright, x and y are the world's; along another axis they are turned with it.
    let turn = krly3d_core::math::frame::turning(&Vec3::z(), &placed.axis)?;
    let placement = frame(&placed.at, &placed.axis, &(turn * Vec3::x()))?;
    let surface = Surface::new(placement, params.number("rx")?, params.number("ry")?, params.number("rz")?)?;
    let skin = Face { name: "skin".into(), surface: Arc::new(surface), outward: true, loops: Vec::new() };
    Ok(Body { vertices: Vec::new(), edges: Vec::new(), faces: vec![skin], solid: true })
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{area, context, made, volume};
    use krly3d_kernel::schema::Params;
    use std::f64::consts::PI;

    #[test]
    fn an_ellipsoid_has_the_volume_four_thirds_pi_abc() {
        let cx = context();

        let egg = made(
            &cx,
            "ellipsoid",
            Params::new().with("rx", 5.0).with("ry", 3.0).with("rz", 2.0).with("axis", [1.0, 1.0, 0.0]),
            &[],
        );

        assert!((volume(&cx, &egg) - 40.0 * PI).abs() < 1e-11);
    }

    #[test]
    fn spheroids_have_the_areas_of_their_closed_forms() {
        let cx = context();
        // Oblate (a = b = 5, c = 3): 2πa²·(1 + (1 − e²)/e·atanh e) with e² = 1 − c²/a².
        let oblate =
            made(&cx, "ellipsoid", Params::new().with("rx", 5.0).with("ry", 5.0).with("rz", 3.0), &[]);
        let e: f64 = (1.0 - 9.0 / 25.0_f64).sqrt();
        let expected = 2.0 * PI * 25.0 * (1.0 + (1.0 - e * e) / e * e.atanh());
        assert!(
            (area(&cx, &oblate) - expected).abs() < 1e-10 * expected,
            "{} vs {expected}",
            area(&cx, &oblate)
        );
        // Prolate (a = b = 2, c = 5): 2πa²·(1 + c/(a·e)·asin e) with e² = 1 − a²/c².
        let prolate =
            made(&cx, "ellipsoid", Params::new().with("rx", 2.0).with("ry", 2.0).with("rz", 5.0), &[]);
        let e: f64 = (1.0 - 4.0 / 25.0_f64).sqrt();
        let expected = 2.0 * PI * 4.0 * (1.0 + 5.0 / (2.0 * e) * e.asin());
        assert!(
            (area(&cx, &prolate) - expected).abs() < 1e-10 * expected,
            "{} vs {expected}",
            area(&cx, &prolate)
        );
    }
}
