//! What every content rule must satisfy: a real, non-negative area, and the same face
//! turned inside out has the same area and the opposite flux.

use super::{content, ContentRequest, FaceContent};
use crate::context::Context;
use krly3d_core::Attempt;
use std::sync::Arc;

pub fn check(cx: &Context, request: &ContentRequest, stated: &FaceContent) -> Result<(), String> {
    let name = request.target().map(|face| face.name.clone()).map_err(|error| error.to_string())?;
    if !(stated.area.is_finite() && stated.flux.is_finite() && stated.area >= -cx.accuracy().area()) {
        return Err(format!("face `{name}` has area {} and flux {}", stated.area, stated.flux));
    }
    let turned = ContentRequest { body: Arc::new(request.body.inverted()), face: request.face };
    match content(cx, &turned) {
        Attempt::Done(inside_out) => {
            let same_area = (inside_out.area - stated.area).abs() <= cx.accuracy().area();
            let opposite_flux = (inside_out.flux + stated.flux).abs() <= cx.accuracy().volume();
            if same_area && opposite_flux {
                Ok(())
            } else {
                Err(format!("face `{name}` turned inside out gives {inside_out:?} against {stated:?}"))
            }
        }
        other => Err(format!("face `{name}` turned inside out is not handled: {other:?}")),
    }
}
