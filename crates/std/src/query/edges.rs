//! `edges`: the edges of a node by name, one line each.

use super::{node, point_value};
use krly3d_core::{text, Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Schema, Value};
use krly3d_kernel::topo::edge_names;
use krly3d_kernel::{Context, Plugin};

pub struct Edges;

impl Plugin<Query> for Edges {
    fn id(&self) -> Id {
        Id::new("edges")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "the edges of a node, one line each: name, curve and length; an edge is named by the two faces it separates (`back|top`, then `back|top#2` when they meet again), an edge of a profile by its label",
            inputs: (0, 0),
            params: vec![node()],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        edges(cx, request).into()
    }
}

fn edges(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let body = &request.node("node")?.body;
    let names = edge_names(body, cx.accuracy());
    let (mut lines, mut listed) = (Vec::new(), Vec::new());
    for (edge, name) in body.edges.iter().zip(&names) {
        let (curve, length) = (edge.curve.summary(), edge.curve.length(edge.range.0, edge.range.1));
        lines.push(format!("{name}: {curve}, length {}", text::num(length)));
        listed.push(Value::map([
            ("name", Value::from(name.as_str())),
            ("curve", Value::from(curve)),
            ("length", Value::from(length)),
            ("middle", point_value(&edge.point(0.5))),
        ]));
    }
    // A ball or a ring is one face that closes on itself.
    let text = if lines.is_empty() { "no edges".to_string() } else { lines.join("\n") };
    Ok(Answer { text, data: Value::map([("edges", Value::List(listed))]), exact: Exactness::Exact })
}

#[cfg(test)]
mod tests {
    use crate::query::testing::{asked, block, entry, items, kernel, number, part, triple, word};
    use krly3d_kernel::schema::{Params, Value};
    use krly3d_kernel::topo::edge_by_name;
    use krly3d_kernel::Kernel;

    /// A half disc of radius 3 pushed up by 10: its round face `s1` and its flat face
    /// `s2` meet along two straight edges.
    fn half_round() -> Kernel {
        kernel(vec![
            ("outline", "profile", Params::new().with("path", "M -3,0 A 0,0 3,0 ccw Z"), &[]),
            ("part", "extrude", Params::new().with("h", 10.0), &["outline"]),
        ])
    }

    fn names(listed: &Value) -> Vec<&str> {
        let mut names: Vec<&str> = items(listed).iter().map(|edge| word(entry(edge, "name"))).collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn every_edge_of_a_block_is_named_by_the_two_faces_it_separates_in_name_order() {
        let answer = asked(&block(), "edges", part());

        assert_eq!(
            names(entry(&answer.data, "edges")),
            [
                "back|bottom",
                "back|left",
                "back|right",
                "back|top",
                "bottom|front",
                "bottom|left",
                "bottom|right",
                "front|left",
                "front|right",
                "front|top",
                "left|top",
                "right|top",
            ]
        );
    }

    #[test]
    fn an_edge_is_listed_with_its_curve_and_length() {
        let can = kernel(vec![("part", "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[])]);

        let answer = asked(&can, "edges", part());

        assert_eq!(
            answer.text,
            "bottom|side: circle r=3 axis=+z, length 18.849556\nside|top: circle r=3 axis=+z, length 18.849556"
        );
    }

    #[test]
    fn a_straight_edge_states_its_direction_and_its_length() {
        let answer = asked(&block(), "edges", part());

        let top_front = items(entry(&answer.data, "edges"))
            .iter()
            .find(|edge| word(entry(edge, "name")) == "front|top")
            .unwrap();
        assert!(["line along +x", "line along -x"].contains(&word(entry(top_front, "curve"))));
        assert_eq!(number(entry(top_front, "length")), 40.0);
        assert_eq!(triple(entry(top_front, "middle")), [0.0, -5.0, 20.0]);
    }

    #[test]
    fn edges_between_the_same_two_faces_are_numbered_in_the_order_of_their_midpoints() {
        let half_round = half_round();

        let answer = asked(&half_round, "edges", part());

        // The two straight edges stand at x = −3 and x = 3.
        let x_of = |name: &str| {
            let listed = items(entry(&answer.data, "edges"));
            triple(entry(listed.iter().find(|edge| word(entry(edge, "name")) == name).unwrap(), "middle"))[0]
        };
        assert_eq!(
            names(entry(&answer.data, "edges")),
            ["bottom|s1", "bottom|s2", "s1|s2", "s1|s2#2", "s1|top", "s2|top"]
        );
        assert_eq!((x_of("s1|s2"), x_of("s1|s2#2")), (-3.0, 3.0));
    }

    #[test]
    fn an_edge_is_found_by_its_name() {
        let half_round = half_round();
        let body = half_round.body("part").unwrap();
        let accuracy = half_round.context().accuracy();

        let (found, missing) =
            (edge_by_name(&body, accuracy, "s1|s2#2"), edge_by_name(&body, accuracy, "s1|s3"));

        assert_eq!(found.map(|edge| body.edges[edge].point(0.5).x), Some(3.0));
        assert_eq!(missing, None);
    }

    #[test]
    fn the_edges_of_a_profile_keep_the_names_they_were_drawn_with() {
        let outline =
            Params::new().with("path", "M 0,0 L 4,0 L 4,3 Z").with("names", vec![Value::from("base")]);
        let drawn = kernel(vec![("part", "profile", outline, &[])]);

        let answer = asked(&drawn, "edges", part());

        assert_eq!(names(entry(&answer.data, "edges")), ["base", "s2", "s3"]);
    }

    #[test]
    fn a_ball_has_no_edges_and_says_so() {
        let ball = kernel(vec![("part", "sphere", Params::new().with("r", 5.0), &[])]);

        let answer = asked(&ball, "edges", part());

        assert_eq!(answer.text, "no edges");
    }
}
