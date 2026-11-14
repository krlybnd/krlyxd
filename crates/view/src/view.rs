//! The `view` query: a look at the model in words, for a reader without eyes — every
//! face seen, by name, with its share of the image.

use super::{shot_params, Shot};
use krly3d_core::text::num;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Schema, Value};
use krly3d_kernel::{Context, Plugin};

pub struct Visible;

impl Plugin<Query> for Visible {
    fn id(&self) -> Id {
        Id::new("view")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "what a picture of the model shows, in words: every face seen as node#face with its share of the image in percent, the largest first, and the share left empty",
            inputs: (0, 0),
            params: shot_params(),
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        Shot::take(cx, request).map(|shot| described(&shot))
    }
}

fn described(shot: &Shot) -> Answer {
    let tally = shot.tally();
    let all = shot.image.seen.len();
    let share = |pixels: usize| 100.0 * pixels as f64 / all as f64;
    let mut text = format!("{}\n", shot.headline("view", tally.len()));
    let mut faces = Vec::with_capacity(tally.len());
    for (seen, pixels) in &tally {
        let (node, face) = (&shot.nodes[seen.body], &shot.bodies[seen.body].faces[seen.face].name);
        let name = format!("{node}#{face}");
        text.push_str(&format!("{name} ?{}%\n", percent(share(*pixels), *pixels)));
        faces.push(Value::map([
            ("name", Value::from(name)),
            ("node", Value::from(node.as_str())),
            ("face", Value::from(face.as_str())),
            ("pixels", Value::from(*pixels)),
            ("share", Value::from(share(*pixels))),
        ]));
    }
    let empty = all - tally.iter().map(|(_, pixels)| pixels).sum::<usize>();
    text.push_str(&format!("background ?{}%\n", percent(share(empty), empty)));
    let mut data = shot.data();
    data.insert("faces".to_string(), Value::List(faces));
    data.insert(
        "background".to_string(),
        Value::map([("pixels", Value::from(empty)), ("share", Value::from(share(empty)))]),
    );
    Answer { text, data: Value::Map(data), exact: shot.exactness() }
}

/// A share with one decimal. A face that is seen at all never reads as a share of zero.
fn percent(share: f64, pixels: usize) -> String {
    let rounded = (share * 10.0).round() / 10.0;
    if pixels > 0 && rounded == 0.0 {
        "<0.1".to_string()
    } else {
        num(rounded)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::session;
    use krly3d_core::{Exactness, Id};
    use krly3d_kernel::ports::query::contract;
    use krly3d_kernel::schema::{Params, Value};

    fn entry<'a>(data: &'a Value, name: &str) -> &'a Value {
        let Value::Map(entries) = data else { panic!("not a map: {data:?}") };
        entries.get(name).unwrap_or_else(|| panic!("no `{name}`"))
    }

    fn small() -> Params {
        Params::new().with("width", 160.0).with("height", 120.0)
    }

    #[test]
    fn the_query_satisfies_the_contract_of_its_family() {
        let session = session();
        let (document, model) = session.kernel().snapshot();

        let checked =
            contract::check(session.kernel().context(), &Id::new("view"), &small(), &document, &model);

        assert_eq!(checked, Ok(()));
    }

    #[test]
    fn a_face_seen_squarely_is_listed_with_exactly_its_share_of_the_image() {
        let mut session = session();
        let look = Params::new()
            .with("nodes", "plate")
            .with("view", "front")
            .with("width", 400.0)
            .with("height", 100.0);

        let reply = session.call("view", &look);

        assert_eq!(
            reply.text,
            "view front 400x100 of plate: 1 face seen, 1 px = 0.166667\nplate#front ?54%\nbackground ?46%\n"
        );
        assert_eq!(reply.exact, Exactness::Sampled { samples: 40_000 });
    }

    #[test]
    fn the_faces_seen_are_listed_from_the_largest_share_to_the_smallest() {
        let mut session = session();

        let reply = session.call("view", &small());

        let lines: Vec<&str> = reply.text.lines().collect();
        let names: Vec<&str> = lines[1..].iter().map(|line| line.split(' ').next().unwrap()).collect();
        let shares: Vec<f64> = lines[1..lines.len() - 1]
            .iter()
            .map(|line| line.rsplit('?').next().unwrap().trim_end_matches('%').parse().unwrap())
            .collect();
        assert_eq!(lines[0], "view iso 160x120 of plate, ball: 4 faces seen, 1 px = 0.608898");
        assert_eq!(names, ["plate#top", "plate#front", "plate#right", "ball#skin", "background"]);
        assert!(shares.is_sorted_by(|a, b| a >= b), "{shares:?}");
    }

    #[test]
    fn the_same_shares_come_as_data_and_add_up_to_the_whole_image() {
        let mut session = session();

        let reply = session.call("view", &small());

        let Value::List(faces) = entry(&reply.data, "faces") else { panic!("faces are not a list") };
        let pixels =
            |of: &Value| if let Value::Number(pixels) = entry(of, "pixels") { *pixels } else { panic!() };
        let seen: f64 = faces.iter().map(pixels).sum();
        assert_eq!(seen + pixels(entry(&reply.data, "background")), 160.0 * 120.0);
        assert_eq!(entry(&faces[0], "name"), &Value::from("plate#top"));
        assert_eq!(
            (entry(&faces[0], "node"), entry(&faces[0], "face")),
            (&Value::from("plate"), &Value::from("top"))
        );
        assert_eq!(entry(&reply.data, "view"), &Value::from("iso"));
    }

    #[test]
    fn a_node_asked_for_at_a_version_is_named_as_it_was_asked_for() {
        let mut session = session();
        let lifted = Params::new().with("id", "plate").with("move", [0.0, 0.0, 5.0]);
        assert!(session.call("transform", &lifted).ok);

        let reply = session.call("view", &small().with("nodes", "plate@1").with("view", "top"));

        assert!(reply.text.contains("\nplate@1#top ?"), "{}", reply.text);
    }

    #[test]
    fn a_face_too_small_for_a_tenth_of_a_percent_is_still_listed_as_seen() {
        let mut session = session();
        let speck = Params::new().with("id", "speck").with("r", 0.3).with("at", [0.0, -40.0, 20.0]);
        assert!(session.call("sphere", &speck).ok);

        let reply = session.call("view", &Params::new().with("view", "front"));

        assert!(reply.text.contains("\nspeck#skin ?<0.1%\n"), "{}", reply.text);
    }
}
