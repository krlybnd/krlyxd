//! `here`: the view from one face — its edges and what lies across each, how thick the
//! material is behind it, and whether anything stands in front of it.

use super::{face_name, face_named, node, point_value, surface_text, vector_value};
use krly3d_core::{text, Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::probe::{self, Hit};
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Kind, Param, Schema, Value};
use krly3d_kernel::topo::{anchor, edge_faces, edge_names, Anchor, Body, FaceId, Use};
use krly3d_kernel::{Context, Plugin};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct Here;

impl Plugin<Query> for Here {
    fn id(&self) -> Id {
        Id::new("here")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "the view from one face: each edge with the face across it, whether they meet convex, concave or tangent and at what angle, then how thick the material is behind the face and what stands in front of it",
            inputs: (0, 0),
            params: vec![node(), Param::required("face", Kind::Text, "name of the face to stand on")],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        here(cx, request).into()
    }
}

/// One thing seen from the face: a line of the answer, and the same as data.
struct View {
    line: String,
    data: Value,
}

fn here(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let body = &request.node("node")?.body;
    let (face, data) = face_named(body, request.params.text("face")?)?;
    let stand = anchor(cx, body, face).into_result()?;
    let surface = surface_text(data, &stand)?;
    let mut lines = vec![
        format!("here: {} [{surface}] at {}", data.name, text::point(&stand.point)),
        "edges:".to_string(),
    ];
    let (names, users) = (edge_names(body, cx.accuracy()), edge_faces(body));
    let mut edges = Vec::new();
    for used in data.uses() {
        let edge = edge_view(cx, body, face, used, &names[used.edge], &users[used.edge]);
        lines.push(format!("  {}", edge.line));
        edges.push(edge.data);
    }
    let (behind, front) = sights(cx, body, &stand)?;
    let (behind, front) = (behind_view(body, behind.as_ref()), front_view(body, front.as_ref()));
    lines.extend([behind.line, front.line]);
    Ok(Answer {
        text: lines.join("\n"),
        data: Value::map([
            ("face", Value::from(data.name.as_str())),
            ("surface", Value::from(surface)),
            ("at", point_value(&stand.point)),
            ("normal", vector_value(&stand.normal)),
            ("edges", Value::List(edges)),
            ("behind", behind.data),
            ("front", front.data),
        ]),
        exact: Exactness::Exact,
    })
}

/// One edge of the face: its name, the face across it and how the two meet.
fn edge_view(cx: &Context, body: &Body, face: FaceId, used: &Use, name: &str, users: &[FaceId]) -> View {
    let edge = &body.edges[used.edge];
    let length = edge.curve.length(edge.range.0, edge.range.1);
    let mut data: BTreeMap<String, Value> = BTreeMap::new();
    data.insert("name".into(), Value::from(name));
    data.insert("length".into(), Value::from(length));
    // Of the two faces along the edge the other one lies across it; a face that meets
    // itself there is its own neighbour.
    let across = match users {
        [one, other] => Some(if *one == face { *other } else { *one }),
        _ => None,
    };
    let Some((across, there)) = across.and_then(|across| Some((across, opposite(body, across, used)?)))
    else {
        let line = format!("{name} (free edge)  length {}", text::num(length));
        return View { line, data: Value::Map(data) };
    };
    let (meeting, degrees) = corner(cx, body, face, across, &there);
    data.insert("to".into(), Value::from(face_name(body, across)));
    data.insert("meeting".into(), Value::from(meeting));
    data.insert("angle".into(), Value::from(degrees));
    let line = format!(
        "{name} -> {}  {meeting} {}°  length {}",
        face_name(body, across),
        text::num(degrees),
        text::num(length)
    );
    View { line, data: Value::Map(data) }
}

/// The use of the same edge by the face across it: the one that runs the other way.
fn opposite(body: &Body, across: FaceId, used: &Use) -> Option<Use> {
    body.faces
        .get(across)?
        .uses()
        .find(|there| there.edge == used.edge && there.forward != used.forward)
        .copied()
}

/// How two faces meet at the middle of the edge between them, and the angle between
/// them there: 180° where one runs smoothly into the other, 90° at a square corner
/// whether it sticks out (convex) or is cut in (concave).
fn corner(cx: &Context, body: &Body, face: FaceId, across: FaceId, there: &Use) -> (&'static str, f64) {
    let edge = &body.edges[there.edge];
    let middle = edge.point(0.5);
    let (near, far) = (body.faces[face].outward_normal(&middle), body.faces[across].outward_normal(&middle));
    if cx.accuracy().parallel(&near, &far) && near.dot(&far) > 0.0 {
        return ("tangent", 180.0);
    }
    let travel = edge.curve.tangent(edge.parameter(0.5)) * if there.forward { 1.0 } else { -1.0 };
    // A face lies to the left of its boundary seen from outside. Where the face across
    // rises above the plane of this one the material forms an inside corner.
    let into_far = far.cross(&travel);
    let meeting = if near.dot(&into_far) > 0.0 { "concave" } else { "convex" };
    (meeting, 180.0 - near.angle(&far).to_degrees())
}

/// The first thing met behind the face and the first in front of it. One line through
/// the anchor answers both: followed into the material it meets the far side, followed
/// back out whatever stands in the way. Its parameter is a distance, positive behind.
fn sights(cx: &Context, body: &Arc<Body>, stand: &Anchor) -> Result<(Option<Hit>, Option<Hit>), Error> {
    let inward = probe::Ray { origin: stand.point, direction: -stand.normal };
    let on_line = probe::cast(cx, body, vec![inward]).into_result()?;
    let hits = on_line.iter().flatten();
    // The hit within the length tolerance of the anchor is the face stood on.
    let tolerance = cx.accuracy().length();
    let behind = hits.clone().find(|hit| hit.t > tolerance);
    let front = hits.rev().find(|hit| hit.t < -tolerance);
    Ok((behind.copied(), front.copied()))
}

fn behind_view(body: &Body, met: Option<&Hit>) -> View {
    let (line, depth) = match met {
        Some(hit) if body.solid => (
            format!("behind: material {} thick, then [{}]", text::num(hit.t), face_name(body, hit.face)),
            "thickness",
        ),
        // A sheet has no material behind it, only whatever else the line meets.
        Some(hit) => (
            format!("behind: blocked by [{}] after {}", face_name(body, hit.face), text::num(hit.t)),
            "distance",
        ),
        None if body.solid => ("behind: no far side found".to_string(), "thickness"),
        None => ("behind: free".to_string(), "distance"),
    };
    View { line, data: sight_data(body, met, depth) }
}

fn front_view(body: &Body, met: Option<&Hit>) -> View {
    let line = match met {
        Some(hit) => {
            format!("in front: blocked by [{}] after {}", face_name(body, hit.face), text::num(-hit.t))
        }
        None => "in front: free".to_string(),
    };
    View { line, data: sight_data(body, met, "distance") }
}

/// What was met as data: the face and how far it is under the name `depth`, or `free`.
fn sight_data(body: &Body, met: Option<&Hit>, depth: &str) -> Value {
    match met {
        Some(hit) => {
            Value::map([(depth, Value::from(hit.t.abs())), ("face", Value::from(face_name(body, hit.face)))])
        }
        None => Value::map([("free", Value::from(true))]),
    }
}

#[cfg(test)]
mod tests {
    use crate::query::testing::{
        asked, block, entry, items, kernel, number, part, plate_with_hole, refused, word,
    };
    use krly3d_core::ErrorKind;
    use krly3d_kernel::schema::Params;

    fn on(face: &str) -> Params {
        part().with("face", face)
    }

    fn pushed_up(path: &str) -> krly3d_kernel::Kernel {
        kernel(vec![
            ("outline", "profile", Params::new().with("path", path), &[]),
            ("part", "extrude", Params::new().with("h", 2.0), &["outline"]),
        ])
    }

    #[test]
    fn the_top_of_a_block_sees_four_square_corners_the_material_under_it_and_nothing_above() {
        let answer = asked(&block(), "here", on("top"));

        let lines: Vec<&str> = answer.text.lines().collect();
        let mut edges = lines[2..6].to_vec();
        edges.sort_unstable();
        assert_eq!(lines[..2], ["here: top [plane n=+z] at (0 0 20)", "edges:"]);
        assert_eq!(
            edges,
            [
                "  back|top -> back  convex 90°  length 40",
                "  front|top -> front  convex 90°  length 40",
                "  left|top -> left  convex 90°  length 10",
                "  right|top -> right  convex 90°  length 10",
            ]
        );
        assert_eq!(lines[6..], ["behind: material 20 thick, then [bottom]", "in front: free"]);
    }

    #[test]
    fn a_face_that_looks_down_is_described_by_the_way_it_faces() {
        let answer = asked(&block(), "here", on("bottom"));

        assert!(answer.text.starts_with("here: bottom [plane n=-z] at (0 0 0)\n"), "{}", answer.text);
        assert!(
            answer.text.ends_with("behind: material 20 thick, then [top]\nin front: free"),
            "{}",
            answer.text
        );
    }

    #[test]
    fn the_wall_of_a_hole_faces_itself_across_the_hole() {
        let answer = asked(&plate_with_hole(), "here", on("s5"));

        assert!(answer.text.contains("in front: blocked by [s5] after 8"), "{}", answer.text);
        assert!(answer.text.contains("  s5|top -> top  convex 90°  length 25.132741"), "{}", answer.text);
    }

    #[test]
    fn an_inside_corner_is_concave() {
        let angle = pushed_up("M 0,0 L 4,0 L 4,1 L 1,1 L 1,4 L 0,4 Z");

        let answer = asked(&angle, "here", on("s3"));

        assert!(answer.text.contains("  s3|s4 -> s4  concave 90°  length 2"), "{}", answer.text);
        assert!(answer.text.contains("  s2|s3 -> s2  convex 90°  length 2"), "{}", answer.text);
        assert!(answer.text.contains("in front: free"), "{}", answer.text);
    }

    #[test]
    fn a_face_inside_a_corner_is_blocked_by_the_face_it_looks_at() {
        let channel = pushed_up("M 0,0 L 5,0 L 5,4 L 4,4 L 4,1 L 1,1 L 1,4 L 0,4 Z");

        let answer = asked(&channel, "here", on("s4"));

        assert!(answer.text.contains("behind: material 1 thick, then [s2]"), "{}", answer.text);
        assert!(answer.text.contains("in front: blocked by [s6] after 3"), "{}", answer.text);
    }

    #[test]
    fn a_round_end_runs_tangent_into_the_flat_side_next_to_it() {
        let slot = pushed_up("M -2,-1 L 2,-1 A 2,0 2,1 ccw L -2,1 A -2,0 -2,-1 ccw Z");

        let answer = asked(&slot, "here", on("s1"));

        assert!(answer.text.contains("  s1|s2 -> s2  tangent 180°  length 2"), "{}", answer.text);
        assert!(answer.text.contains("behind: material 2 thick, then [s3]"), "{}", answer.text);
    }

    #[test]
    fn corners_that_are_not_square_state_their_angle() {
        let wedge = pushed_up("M 0,0 L 4,0 L 0,4 Z");

        let answer = asked(&wedge, "here", on("s1"));

        assert!(answer.text.contains("  s1|s2 -> s2  convex 45°  length 2"), "{}", answer.text);
        assert!(answer.text.contains("  s1|s3 -> s3  convex 90°  length 2"), "{}", answer.text);
    }

    #[test]
    fn a_curved_face_sees_the_opposite_side_of_itself_through_the_material() {
        let can = kernel(vec![("part", "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[])]);

        let answer = asked(&can, "here", on("side"));

        assert!(answer.text.contains("  side|top -> top  convex 90°  length 18.849556"), "{}", answer.text);
        assert!(
            answer.text.ends_with("behind: material 6 thick, then [side]\nin front: free"),
            "{}",
            answer.text
        );
    }

    #[test]
    fn a_profile_has_free_edges_and_nothing_on_either_side() {
        let answer =
            asked(&plate_with_hole(), "here", Params::new().with("node", "outline").with("face", "profile"));

        assert!(answer.text.contains("  s1 (free edge)  length 60"), "{}", answer.text);
        assert!(answer.text.ends_with("behind: free\nin front: free"), "{}", answer.text);
    }

    #[test]
    fn the_view_comes_as_data_too() {
        let answer = asked(&block(), "here", on("top"));

        let edges = items(entry(&answer.data, "edges"));
        let back = edges.iter().find(|edge| word(entry(edge, "name")) == "back|top").unwrap();
        assert_eq!(edges.len(), 4);
        assert_eq!((word(entry(back, "to")), word(entry(back, "meeting"))), ("back", "convex"));
        assert!((number(entry(back, "angle")) - 90.0).abs() < 1e-9);
        assert_eq!(number(entry(entry(&answer.data, "behind"), "thickness")), 20.0);
        assert_eq!(word(entry(entry(&answer.data, "behind"), "face")), "bottom");
    }

    #[test]
    fn a_body_of_many_faces_names_only_the_first_few_in_the_refusal() {
        let peg = ("peg", "cylinder", Params::new().with("r", 3.0).with("h", 6.0), &[][..]);
        let row =
            ("part", "pattern", Params::new().with("n", 5.0).with("step", [8.0, 0.0, 0.0]), &["peg"][..]);

        let error = refused(&kernel(vec![peg, row]), "here", on("lid"));

        assert!(error.message.starts_with("there is no face `lid`; the faces are bottom[1], "), "{error}");
        assert!(error.message.ends_with(", side[4] and 3 more, which `faces` lists"), "{error}");
    }

    #[test]
    fn a_face_that_does_not_exist_is_refused_with_the_names_that_do() {
        let error = refused(&block(), "here", on("lid"));

        let (lead, listed) = error.message.split_once("; the faces are ").unwrap();
        let mut listed: Vec<&str> = listed.split(", ").collect();
        listed.sort_unstable();
        assert_eq!((error.kind, lead), (ErrorKind::NotFound, "there is no face `lid`"));
        assert_eq!(listed, ["back", "bottom", "front", "left", "right", "top"]);
    }
}
