//! How much a model looks like a drawing of it, in the drawing's own plane and units.
//!
//! A drawing is what a peer such as krlyvis hands over: the outline of the subject placed
//! in a world plane (`axes` names the two world axes it shows) at a resolution (`unit`,
//! world length per pixel of the drawing). The answer is vectors too: the overlap of the
//! two silhouettes and, along each axis of the plane, the stations where their extents
//! differ — "at x = 93 the model reaches y ±53.6, the drawing ±48" — so that whoever
//! models knows what to change without looking at a picture.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::ports::shape::Shape;
use crate::schema::Value;
use krly3d_core::{text, Attempt, Error, Id};
use std::sync::Arc;

pub struct Likeness;

/// An outline in a world plane.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    /// The world axes (0 x, 1 y, 2 z) of the plane, in the order of the coordinates.
    pub axes: [usize; 2],
    /// World length of one pixel of the drawing: the finest difference it can show.
    pub unit: f64,
    /// Closed polygons in world coordinates along `axes`, the outer boundary first.
    pub polygons: Vec<Vec<[f64; 2]>>,
}

#[derive(Clone, Debug)]
pub struct LikenessRequest {
    /// The shapes drawn together, as one silhouette.
    pub shapes: Vec<Arc<dyn Shape>>,
    pub drawing: Drawing,
}

/// Where along one axis of the plane the extents across it differ.
#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    /// 0 when stepping along the drawing's first axis, 1 along its second.
    pub along: usize,
    pub at: f64,
    /// The extent across, from lowest to highest, of the model and of the drawing.
    pub model: Option<[f64; 2]>,
    pub drawing: Option<[f64; 2]>,
}

impl Station {
    /// How far apart the two extents are at their worse end; a side that is missing
    /// counts with its whole extent.
    pub fn off(&self) -> f64 {
        match (self.model, self.drawing) {
            (Some(model), Some(drawing)) => (model[0] - drawing[0]).abs().max((model[1] - drawing[1]).abs()),
            (Some(only), None) | (None, Some(only)) => only[1] - only[0],
            (None, None) => 0.0,
        }
    }

    /// One line for a reader, with the axes named.
    pub fn text(&self, axes: [usize; 2]) -> String {
        let (step, across) = (AXES[axes[self.along]], AXES[axes[1 - self.along]]);
        let extent = |found: Option<[f64; 2]>| {
            found.map_or("nothing".to_string(), |[low, high]| {
                format!("{across} {}..{}", text::num(low), text::num(high))
            })
        };
        format!(
            "{step}={}: model {}, drawing {} (off {})",
            text::num(self.at),
            extent(self.model),
            extent(self.drawing),
            text::num(self.off())
        )
    }
}

const AXES: [&str; 3] = ["x", "y", "z"];

#[derive(Clone, Debug, PartialEq)]
pub struct Resemblance {
    /// Area of the overlap over the area of the union, 1 for the same silhouette.
    pub iou: f64,
    pub model_area: f64,
    pub drawing_area: f64,
    /// The stations where the extents differ most, worst first.
    pub worst: Vec<Station>,
}

impl Family for Likeness {
    const NAME: &'static str = "likeness";
    type Request = LikenessRequest;
    type Response = Resemblance;
    type Context = Context;
}

pub fn likeness(cx: &Context, method: &str, request: &LikenessRequest) -> Attempt<Resemblance> {
    cx.call::<Likeness>(&Id::new(method), request)
}

/// The drawing a peer sent as data: `{axes: ["y", "x"], unit, polygons: [[[a, b], …], …]}`.
pub fn drawing_of(value: &Value) -> Result<Drawing, Error> {
    let malformed = |what: &str| Error::format(format!("a drawing needs {what}"));
    let Value::Map(entries) = value else { return Err(malformed("to be a map")) };
    let axes: Vec<usize> = match entries.get("axes") {
        Some(Value::List(names)) => names
            .iter()
            .map(|name| match name {
                Value::Text(name) => {
                    AXES.iter().position(|axis| axis == name).ok_or_else(|| malformed("axes named x, y or z"))
                }
                _ => Err(malformed("axes named x, y or z")),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(malformed("`axes`")),
    };
    let [first, second] = axes[..] else { return Err(malformed("two axes")) };
    let unit = match entries.get("unit") {
        Some(Value::Number(unit)) if unit.is_finite() && *unit > 0.0 => *unit,
        _ => return Err(malformed("a positive `unit`")),
    };
    let number = |value: &Value| match value {
        Value::Number(number) if number.is_finite() => Ok(*number),
        _ => Err(malformed("finite coordinates")),
    };
    let point = |value: &Value| match value {
        Value::List(pair) if pair.len() == 2 => Ok([number(&pair[0])?, number(&pair[1])?]),
        _ => Err(malformed("points of two coordinates")),
    };
    let polygons = match entries.get("polygons") {
        Some(Value::List(polygons)) => polygons
            .iter()
            .map(|polygon| match polygon {
                Value::List(points) if points.len() >= 3 => points.iter().map(point).collect(),
                _ => Err(malformed("polygons of three points or more")),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(malformed("`polygons`")),
    };
    Ok(Drawing { axes: [first, second], unit, polygons })
}
