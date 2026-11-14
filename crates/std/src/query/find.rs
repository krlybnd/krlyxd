//! `find`: the faces of a node ranked by a search over their keys and the keys of
//! their neighbours.

use super::node;
use krly3d_core::{text, Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::index::{best, index, Search};
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Kind, Param, Schema, Value};
use krly3d_kernel::{Context, Plugin};

pub struct Find;

impl Plugin<Query> for Find {
    fn id(&self) -> Id {
        Id::new("find")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "faces ranked by a search over their keys, best first with their scores; the faces found are candidates to check with `here`, not statements about the model",
            inputs: (0, 0),
            params: vec![
                node(),
                Param::required(
                    "query",
                    Kind::Text,
                    "keys as words, e.g. `cylinder r:3 near:plane near:n:+z`: a bare word is a kind, `near:` asks a key of the neighbours, `key^2` gives a key a weight; `keys` shows the keys a face has",
                ),
                Param::optional("k", Kind::Count, 5.0, "how many of the best matches to list"),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        find(cx, request).into()
    }
}

fn find(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let body = &request.node("node")?.body;
    let written = request.params.text("query")?;
    let search = Search::parse(written)?;
    let index = index(cx, body).into_result()?;
    let found = best(index.as_ref(), &search, request.params.count("k")?);
    let name = |element: &usize| index.elements().get(*element).map_or("?", String::as_str);
    let lines: Vec<String> =
        found.iter().map(|(element, score)| format!("{} ({})", name(element), text::num(*score))).collect();
    let matches = found.iter().map(|(element, score)| {
        Value::map([("face", Value::from(name(element))), ("score", Value::from(*score))])
    });
    Ok(Answer {
        text: if lines.is_empty() { format!("no face matches `{written}`") } else { lines.join("\n") },
        data: Value::map([("matches", Value::List(matches.collect()))]),
        exact: Exactness::Exact,
    })
}

#[cfg(test)]
mod tests {
    use crate::query::testing::{asked, entry, items, kernel, number, part, plate_with_hole, refused, word};
    use krly3d_core::ErrorKind;
    use krly3d_kernel::schema::{Params, Value};
    use krly3d_kernel::Kernel;

    /// A block with one upright corner rounded: the round face touches `right`, `back`,
    /// `top` and `bottom`, and not `front` or `left`.
    fn rounded_block() -> Kernel {
        let names: Vec<Value> = ["front", "right", "round", "back", "left"].map(Value::from).to_vec();
        let outline =
            Params::new().with("path", "M 0,0 L 40,0 L 40,7 A 37,7 37,10 ccw L 0,10 Z").with("names", names);
        kernel(vec![
            ("outline", "profile", outline, &[]),
            ("part", "extrude", Params::new().with("h", 20.0), &["outline"]),
        ])
    }

    fn search(query: &str) -> Params {
        part().with("query", query)
    }

    #[test]
    fn a_kind_finds_the_faces_of_that_kind() {
        let answer = asked(&rounded_block(), "find", search("cylinder"));

        assert_eq!(answer.text, "round (1)");
    }

    #[test]
    fn several_keys_add_up_and_the_best_match_comes_first() {
        let answer = asked(&rounded_block(), "find", search("plane n:+z"));

        assert_eq!(answer.text.lines().next(), Some("top (2)"));
        assert_eq!(answer.text.lines().count(), 5);
    }

    #[test]
    fn planes_that_touch_the_cylinder_outrank_the_planes_that_do_not() {
        let answer = asked(&rounded_block(), "find", search("plane near:kind:cylinder").with("k", 6.0));

        let lines: Vec<&str> = answer.text.lines().collect();
        let (mut touching, mut apart) = (lines[..4].to_vec(), lines[4..].to_vec());
        touching.sort_unstable();
        apart.sort_unstable();
        assert_eq!(touching, ["back (2)", "bottom (2)", "right (2)", "top (2)"]);
        assert_eq!(apart, ["front (1.75)", "left (1.75)"]);
    }

    #[test]
    fn only_as_many_matches_are_listed_as_asked_for() {
        let answer = asked(&rounded_block(), "find", search("plane").with("k", 2.0));

        assert_eq!(answer.text.lines().count(), 2);
    }

    #[test]
    fn the_matches_come_as_data_with_their_scores() {
        let answer = asked(&plate_with_hole(), "find", search("r:4"));

        let matches = items(entry(&answer.data, "matches"));
        assert_eq!(matches.len(), 1);
        assert_eq!((word(entry(&matches[0], "face")), number(entry(&matches[0], "score"))), ("s5", 1.0));
    }

    #[test]
    fn a_search_that_matches_nothing_says_so() {
        let answer = asked(&rounded_block(), "find", search("made:chamfer"));

        assert_eq!(answer.text, "no face matches `made:chamfer`");
        assert!(items(entry(&answer.data, "matches")).is_empty());
    }

    #[test]
    fn a_search_that_cannot_be_read_is_refused_with_the_word_at_fault() {
        let error = refused(&rounded_block(), "find", search("plane^heavy"));

        assert_eq!(error.kind, ErrorKind::Input);
        assert!(error.message.contains("`plane^heavy`"), "{error}");
    }
}
