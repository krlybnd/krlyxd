//! `mesh`: the node read as triangles, for whoever shows, prints or exports it, with what
//! is needed to trust the reading: how many triangles, the volume and area they hold and
//! whether they close.

use super::node;
use krly3d_core::{text, Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::interpret::mesh_of;
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Kind, Param, Schema, Value};
use krly3d_kernel::{Context, Plugin};

pub struct Mesh;

impl Plugin<Query> for Mesh {
    fn id(&self) -> Id {
        Id::new("mesh")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "the node cut into triangles that stay within `tolerance` of it: their count, volume, area and the sides left open (none for a closed body)",
            inputs: (0, 0),
            params: vec![
                node(),
                Param::optional("tolerance", Kind::Number, 0.05, "how far the triangles may stray from the surface"),
                Param::optional("triangles", Kind::Flag, false, "also give the points and the triangles themselves, for a client that draws or exports them"),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        mesh(cx, request).into()
    }
}

fn mesh(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let shape = &request.node("node")?.shape;
    let tolerance = request.params.number("tolerance")?;
    let found = mesh_of(cx, shape, tolerance).into_result()?;
    let open = found.open_sides().len();
    let (volume, area) = (found.volume(), found.area());
    Ok(Answer {
        text: format!(
            "{} triangles, {} points, volume {}, area {}, {open} open sides",
            found.triangles.len(),
            found.points.len(),
            text::num(volume),
            text::num(area)
        ),
        data: {
            let mut data = Value::map([
                ("triangles", (found.triangles.len() as f64).into()),
                ("points", (found.points.len() as f64).into()),
                ("volume", volume.into()),
                ("area", area.into()),
                ("open", (open as f64).into()),
            ]);
            if request.params.flag("triangles")? {
                if let Value::Map(entries) = &mut data {
                    let points =
                        found.points.iter().flat_map(|point| point.coords.iter().copied()).map(Value::from);
                    let corners = found.triangles.iter().flatten().map(|index| Value::from(*index as f64));
                    entries.insert("coordinates".to_string(), Value::List(points.collect()));
                    entries.insert("corners".to_string(), Value::List(corners.collect()));
                }
            }
            data
        },
        // The tolerance is checked at sample points of each triangle, not proved between them.
        exact: Exactness::Sampled { samples: u32::try_from(found.triangles.len()).unwrap_or(u32::MAX) },
    })
}

#[cfg(test)]
mod tests {
    use crate::query::testing::{asked, entry, kernel, part};
    use krly3d_kernel::schema::{Params, Value};
    use std::f64::consts::PI;

    fn number(value: &Value) -> f64 {
        match value {
            Value::Number(number) => *number,
            other => panic!("not a number: {other:?}"),
        }
    }

    #[test]
    fn a_ball_reads_as_closed_triangles_holding_its_volume() {
        let ball = kernel(vec![("part", "sphere", Params::new().with("r", 5.0), &[])]);

        let answer = asked(&ball, "mesh", part().with("tolerance", 0.01));

        assert_eq!(number(entry(&answer.data, "open")), 0.0);
        let exact = 4.0 / 3.0 * PI * 125.0;
        assert!((number(entry(&answer.data, "volume")) - exact).abs() < 0.01 * 4.0 * PI * 25.0);
    }
}
