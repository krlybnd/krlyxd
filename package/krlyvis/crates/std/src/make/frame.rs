//! `frame outline view=… points=…`: an outline of a drawing placed in the world, by two
//! pixels whose world coordinates are known.

use super::single;
use krly3d_core::schema::{Kind, Param, Schema};
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::frame::AXIS_NAMES;
use krlyvis_kernel::ports::make::{Make, MakeRequest};
use krlyvis_kernel::{Context, Datum, Frame, Pixel, Plugin, View};
use std::sync::Arc;

pub struct FrameTool;

impl Plugin<Make> for FrameTool {
    fn id(&self) -> Id {
        Id::new("frame")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "an outline of an orthographic drawing placed in the world: two pixels with known world coordinates fix scale, signs and position",
            inputs: (1, 1),
            params: vec![
                Param::required("view", Kind::Text, "top (seen along -z), side (along +y) or front (along +x)"),
                Param::required("points", Kind::Numbers, "px,py,a,b for two pixels: a and b are their world coordinates along `axes`"),
                Param::optional("axes", Kind::Text, "", "the world axes image x and image y show, such as y,x; the view's own (top x,y; side x,z; front y,z) when empty"),
            ],
        })
    }

    fn call(&self, _: &Context, request: &MakeRequest) -> Attempt<Datum> {
        framed(request).into()
    }
}

fn framed(request: &MakeRequest) -> Result<Datum, Error> {
    let Datum::Outline { shape } = single(&request.inputs, "frame", "outline")? else {
        return Err(Error::input("`frame` reads an outline"));
    };
    let params = &request.params;
    let view = View::named(params.text("view")?)?;
    let axes = match params.text("axes")? {
        "" => view.axes(),
        written => {
            let named: Option<Vec<usize>> = written
                .split(',')
                .map(|name| AXIS_NAMES.iter().position(|axis| *axis == name.trim()))
                .collect();
            match named.as_deref() {
                Some([first, second]) => [*first, *second],
                _ => {
                    return Err(Error::input(format!(
                        "`axes` names two of x, y, z, such as y,x; got `{written}`"
                    )))
                }
            }
        }
    };
    let points = params.numbers("points")?;
    let [px1, py1, a1, b1, px2, py2, a2, b2] = points[..] else {
        return Err(Error::input(format!(
            "`points` is px,py,a,b for two pixels: eight numbers, got {}",
            points.len()
        )));
    };
    let frame =
        Frame::through(view, axes, (Pixel::new(px1, py1), [a1, b1]), (Pixel::new(px2, py2), [a2, b2]))?;
    Ok(Datum::Placed { shape: Arc::clone(shape), frame })
}
