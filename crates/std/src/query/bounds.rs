//! `bounds`: how far a node reaches along the world axes.

use super::{node, point_value, vector_value};
use krly3d_core::{text, Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Schema, Value};
use krly3d_kernel::topo::body_bounds;
use krly3d_kernel::{Context, Plugin};

pub struct Bounds;

impl Plugin<Query> for Bounds {
    fn id(&self) -> Id {
        Id::new("bounds")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "the smallest box around a node with its sides along the world axes, as `x low..high  y low..high  z low..high`",
            inputs: (0, 0),
            params: vec![node()],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        bounds(cx, request).into()
    }
}

fn bounds(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let body = &request.node("node")?.body;
    let around = body_bounds(cx, body).into_result()?;
    if around.is_empty() {
        return Err(Error::geometry("the node has nothing to put a box around"));
    }
    let reach = |axis: usize| {
        format!("{} {}..{}", ["x", "y", "z"][axis], text::num(around.low[axis]), text::num(around.high[axis]))
    };
    Ok(Answer {
        text: [reach(0), reach(1), reach(2)].join("  "),
        data: Value::map([
            ("low", point_value(&around.low)),
            ("high", point_value(&around.high)),
            ("size", vector_value(&around.size())),
        ]),
        exact: Exactness::Exact,
    })
}

#[cfg(test)]
mod tests {
    use crate::query::testing::{asked, block, entry, kernel, part, triple};
    use krly3d_kernel::schema::Params;

    #[test]
    fn a_block_reaches_from_its_lowest_to_its_highest_corner() {
        let answer = asked(&block(), "bounds", part());

        assert_eq!(answer.text, "x -20..20  y -5..5  z 0..20");
        assert_eq!(triple(entry(&answer.data, "size")), [40.0, 10.0, 20.0]);
    }

    #[test]
    fn a_ball_reaches_one_radius_from_its_centre_although_it_has_no_edge() {
        let ball =
            kernel(vec![("part", "sphere", Params::new().with("r", 5.0).with("at", [1.0, 2.0, 3.0]), &[])]);

        let answer = asked(&ball, "bounds", part());

        assert_eq!(answer.text, "x -4..6  y -3..7  z -2..8");
        assert_eq!(triple(entry(&answer.data, "low")), [-4.0, -3.0, -2.0]);
    }
}
