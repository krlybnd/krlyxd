//! The `pick` query: what lies under given pixels of a look at the model — the node, the
//! face and the point in space. With the camera of a photograph, a point marked on the
//! photograph becomes a point on the model.

use super::{nodes, shot_params, size, view};
use krly3d_core::math::bounds::Bounds;
use krly3d_core::text::{num, point};
use krly3d_core::{Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::probe::cast;
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Kind, Param, Schema, Value};
use krly3d_kernel::topo::body_bounds;
use krly3d_kernel::{Context, Plugin};

pub struct Pick;

impl Plugin<Query> for Pick {
    fn id(&self) -> Id {
        Id::new("pick")
    }

    fn schema(&self) -> Option<Schema> {
        let mut params = shot_params();
        params.push(Param::required(
            "pixels",
            Kind::Numbers,
            "the pixels as column,row,column,row,…, counted from the top left",
        ));
        Some(Schema {
            about: "what lies under each pixel of the same look as `render`: node#face and the point in space nearest the eye",
            inputs: (0, 0),
            params,
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        picked(cx, request).into()
    }
}

fn picked(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let (names, bodies) = nodes(request)?;
    let (_, look) = view(cx, &request.params)?;
    let size = size(&request.params)?;
    let pixels = request.params.numbers("pixels")?;
    if pixels.is_empty() || pixels.len() % 2 != 0 {
        return Err(Error::input("`pixels` holds column,row pairs"));
    }
    let mut around = Bounds::empty();
    for body in &bodies {
        around = around.union(&body_bounds(cx, body).into_result()?);
    }
    let projection = look.projection(cx.accuracy(), &around, size)?;
    let mut text = String::new();
    let mut found = Vec::new();
    for pair in pixels.chunks(2) {
        let (column, row) = (pair[0], pair[1]);
        if column < 0.0 || row < 0.0 || column.fract() != 0.0 || row.fract() != 0.0 {
            return Err(Error::input(format!("pixel {column},{row} is not a pair of whole numbers")));
        }
        let ray = projection.ray(column as usize, row as usize);
        let mut nearest: Option<(f64, usize, usize, krly3d_core::math::Point3)> = None;
        for (index, body) in bodies.iter().enumerate() {
            let hits = cast(cx, body, vec![ray]).into_result()?;
            if let Some(hit) = hits[0].iter().find(|hit| hit.t > 0.0) {
                if nearest.is_none_or(|(t, ..)| hit.t < t) {
                    nearest = Some((hit.t, index, hit.face, hit.point));
                }
            }
        }
        match nearest {
            Some((_, body, face, at)) => {
                let name = format!("{}#{}", names[body], bodies[body].faces[face].name);
                text.push_str(&format!("pixel {},{}: {name} at {}\n", num(column), num(row), point(&at)));
                found.push(Value::map([
                    ("pixel", Value::List(vec![column.into(), row.into()])),
                    ("face", Value::from(name)),
                    ("point", Value::from([at.x, at.y, at.z])),
                ]));
            }
            None => {
                text.push_str(&format!("pixel {},{}: nothing\n", num(column), num(row)));
                found.push(Value::map([("pixel", Value::List(vec![column.into(), row.into()]))]));
            }
        }
    }
    // A point on a surface without closed-form line hits is only as exact as its root.
    let numeric =
        bodies.iter().flat_map(|body| &body.faces).any(|face| face.surface.classes().contains(&"nurbs"));
    let exact =
        if numeric { Exactness::Numeric { tolerance: cx.accuracy().length() } } else { Exactness::Exact };
    Ok(Answer { text, data: Value::map([("picked", Value::List(found))]), exact })
}

#[cfg(test)]
mod tests {
    use crate::testing::session;
    use krly3d_kernel::schema::{Params, Value};
    use std::collections::BTreeMap;

    /// A camera 100 in front of the plate, level with its middle, 200 pixels square.
    fn camera() -> Value {
        let entries: BTreeMap<String, Value> = [
            ("eye".to_string(), Value::from([0.0, -100.0, 5.0])),
            ("target".to_string(), Value::from([0.0, 0.0, 5.0])),
            ("focal".to_string(), Value::from(200.0)),
        ]
        .into_iter()
        .collect();
        Value::Map(entries)
    }

    #[test]
    fn a_pixel_shows_the_point_where_its_line_of_sight_from_the_eye_meets_the_model() {
        let mut session = session();
        let params = Params::new()
            .with("view", camera())
            .with("width", 200.0)
            .with("height", 200.0)
            .with("pixels", Value::List([100.0, 100.0, 125.0, 100.0].map(Value::from).to_vec()));

        let reply = session.call("pick", &params);

        // Through the middle of pixel 125 the line runs 25.5/200 to the right per unit ahead
        // and 0.5/200 down: at the plate's front, 80 ahead, that is 10.2 across and 0.2 down.
        let lines: Vec<&str> = reply.text.lines().collect();
        assert!(lines[0].starts_with("pixel 100,100: ball#skin at "), "{}", reply.text);
        assert_eq!(lines[1], "pixel 125,100: plate#front at (10.2 -20 4.8)", "{}", reply.text);
    }
}
