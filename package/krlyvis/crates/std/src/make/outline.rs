//! `outline picture …`: the subject of a picture, or of a part of it, as polygons.

use super::single;
use krly3d_core::schema::{Kind, Param, Schema, Value};
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::make::{Make, MakeRequest};
use krlyvis_kernel::ports::outline::{outline, OutlineRequest};
use krlyvis_kernel::{Context, Datum, Pixel, Plugin};
use std::sync::Arc;

pub struct OutlineTool;

impl Plugin<Make> for OutlineTool {
    fn id(&self) -> Id {
        Id::new("outline")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "the subject of a picture as polygons in pixels (x right, y down), outer boundary first; frame places it in the world",
            inputs: (1, 1),
            params: vec![
                Param::optional("region", Kind::Numbers, Value::List(Vec::new()), "left,top,right,bottom pixels to look in; the whole picture when empty"),
                Param::optional("seed", Kind::Numbers, Value::List(Vec::new()), "x,y of a pixel of the subject; the largest blob when empty"),
                Param::optional("threshold", Kind::Number, -1.0, "brightness between subject and background, 0..255; found from the picture when negative"),
                Param::optional("simplify", Kind::Number, 1.0, "pixels the polygon may cut from the traced boundary"),
                Param::optional("method", Kind::Text, "trace", "the outline plugin"),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &MakeRequest) -> Attempt<Datum> {
        outlined(cx, request).into()
    }
}

fn outlined(cx: &Context, request: &MakeRequest) -> Result<Datum, Error> {
    let Datum::Picture { image, .. } = single(&request.inputs, "outline", "picture")? else {
        return Err(Error::input("`outline` reads a picture"));
    };
    let params = &request.params;
    let region = match params.numbers("region")?.as_slice() {
        [] => None,
        [left, top, right, bottom] if [left, top, right, bottom].iter().all(|value| **value >= 0.0) => {
            Some([*left as usize, *top as usize, *right as usize, *bottom as usize])
        }
        other => {
            return Err(Error::input(format!("`region` is left,top,right,bottom in pixels, got {other:?}")))
        }
    };
    let seed = match params.numbers("seed")?.as_slice() {
        [] => None,
        [x, y] => Some(Pixel::new(*x, *y)),
        other => return Err(Error::input(format!("`seed` is x,y in pixels, got {other:?}"))),
    };
    let threshold = Some(params.number("threshold")?).filter(|value| *value >= 0.0);
    let simplify = params.number("simplify")?;
    if !(simplify.is_finite() && simplify >= 0.0) {
        return Err(Error::input(format!("`simplify` is a distance in pixels, got {simplify}")));
    }
    let asked = OutlineRequest { image: Arc::clone(image), region, seed, threshold, simplify };
    let shape = outline(cx, params.text("method")?, &asked).into_result()?;
    Ok(Datum::Outline { shape: Arc::new(shape) })
}
