//! `keys`: the keys a search can match on one face, and the faces related to it.

use super::{face_named, node};
use krly3d_core::{Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::index::index;
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Kind, Param, Schema, Value};
use krly3d_kernel::{Context, Plugin};

pub struct Keys;

impl Plugin<Query> for Keys {
    fn id(&self) -> Id {
        Id::new("keys")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "the keys of one face, as `find` matches them, and the faces counted as its neighbours",
            inputs: (0, 0),
            params: vec![node(), Param::required("face", Kind::Text, "name of the face")],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        keys(cx, request).into()
    }
}

fn keys(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let body = &request.node("node")?.body;
    let (face, data) = face_named(body, request.params.text("face")?)?;
    let index = index(cx, body).into_result()?;
    let keys: Vec<&str> = index.keys(face).iter().map(String::as_str).collect();
    let near: Vec<&str> = index
        .related(face)
        .iter()
        .filter_map(|other| index.elements().get(*other))
        .map(String::as_str)
        .collect();
    let neighbours = if near.is_empty() { "nothing".to_string() } else { near.join(", ") };
    Ok(Answer {
        text: format!("{}: {}\nnear: {neighbours}", data.name, keys.join(" ")),
        data: Value::map([
            ("face", Value::from(data.name.as_str())),
            ("keys", Value::texts(keys.iter().copied())),
            ("near", Value::texts(near.iter().copied())),
        ]),
        exact: Exactness::Exact,
    })
}

#[cfg(test)]
mod tests {
    use crate::query::testing::{asked, block, entry, items, kernel, part, plate_with_hole, refused, word};
    use krly3d_core::ErrorKind;
    use krly3d_kernel::schema::Params;

    #[test]
    fn a_face_lists_its_keys_and_then_its_neighbours() {
        let answer = asked(&block(), "keys", part().with("face", "top"));

        let lines: Vec<&str> = answer.text.lines().collect();
        let mut near: Vec<&str> = lines[1].strip_prefix("near: ").unwrap().split(", ").collect();
        near.sort_unstable();
        assert!(lines[0].starts_with("top: cell:L1:"), "{}", answer.text);
        assert!(lines[0].ends_with(" kind:plane n:+z name:top"), "{}", answer.text);
        assert_eq!(near, ["back", "front", "left", "right"]);
    }

    #[test]
    fn the_wall_of_a_hole_is_keyed_by_its_radius_and_axis() {
        let answer = asked(&plate_with_hole(), "keys", part().with("face", "s5"));

        let keys: Vec<&str> = items(entry(&answer.data, "keys")).iter().map(word).collect();
        let near: Vec<&str> = items(entry(&answer.data, "near")).iter().map(word).collect();
        assert!(
            ["axis:+z", "kind:cylinder", "name:s5", "r:4"].iter().all(|key| keys.contains(key)),
            "{keys:?}"
        );
        assert_eq!(near, ["bottom", "top"]);
    }

    #[test]
    fn a_ball_has_keys_and_no_neighbour() {
        let ball = kernel(vec![("part", "sphere", Params::new().with("r", 5.0), &[])]);

        let answer = asked(&ball, "keys", part().with("face", "skin"));

        assert!(answer.text.contains(" kind:sphere name:skin r:5\nnear: nothing"), "{}", answer.text);
    }

    #[test]
    fn a_face_that_does_not_exist_is_refused() {
        let error = refused(&block(), "keys", part().with("face", "lid"));

        assert_eq!(error.kind, ErrorKind::NotFound);
    }
}
