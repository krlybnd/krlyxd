//! Content of a whole ellipsoid: volume `4/3·π·abc`, and the area `3V·R_G(a⁻², b⁻², c⁻²)`
//! (DLMF 19.33.1), an elliptic integral evaluated to round-off by Carlson's method.

use super::side;
use crate::surface::Ellipsoid;
use krly3d_core::math::elliptic::rg;
use krly3d_core::{Attempt, Exactness, Id};
use krly3d_kernel::ports::content::{Content, ContentRequest, FaceContent};
use krly3d_kernel::{Context, Plugin};
use std::f64::consts::PI;

/// The relative accuracy claimed for the area: Carlson's duplication stops when its series
/// is exact to round-off, and a few dozen operations add a few units of it.
const AREA_RELATIVE: f64 = 1e-13;

pub struct EllipsoidContent;

impl Plugin<Content> for EllipsoidContent {
    fn id(&self) -> Id {
        Id::new("ellipsoid")
    }

    fn call(&self, _: &Context, request: &ContentRequest) -> Attempt<FaceContent> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        let Some(ellipsoid) = face.surface.as_any().downcast_ref::<Ellipsoid>() else {
            return Attempt::declined("the ellipsoid rule takes a face on an ellipsoid");
        };
        if !face.loops.is_empty() {
            return Attempt::declined(
                "only a whole ellipsoid has a content rule yet; a trimmed piece has none",
            );
        }
        let [a, b, c] = ellipsoid.axes();
        let volume = 4.0 / 3.0 * PI * a * b * c;
        let area = 3.0 * volume * rg(1.0 / (a * a), 1.0 / (b * b), 1.0 / (c * c));
        // The divergence of the position is 3, so ∫ p·n over the closed surface is 3V.
        let flux = side(face.outward) * 3.0 * volume;
        Attempt::Done(FaceContent {
            area,
            flux,
            exact: Exactness::Numeric { tolerance: area * AREA_RELATIVE },
        })
    }
}
