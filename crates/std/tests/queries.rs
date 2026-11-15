//! What an agent can ask, through the one door: every query as a call of `Session::call`.

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::{Accuracy, Exactness};
use krly3d_kernel::schema::{Params, Value};
use krly3d_kernel::{MethodKind, Reply, Session};

fn session() -> Session {
    let mut builder = krly3d_std::standard();
    builder.accuracy(Accuracy::for_scale(100.0).unwrap());
    builder.session()
}

/// A session with the node `b`: a block 40 wide, 10 deep and 20 high.
fn with_block() -> Session {
    let mut session = session();
    made(&mut session, "box", Params::new().with("id", "b").with("w", 40.0).with("d", 10.0).with("h", 20.0));
    session
}

/// A session with the node `b`: the block with the upright corner between its right and
/// its back face rounded with radius 3. The round face is called `round`.
fn with_rounded_block() -> Session {
    let mut session = session();
    let names: Vec<Value> = ["front", "right", "round", "back", "left"].map(Value::from).to_vec();
    let outline = Params::new()
        .with("id", "outline")
        .with("path", "M -20,-5 L 20,-5 L 20,2 A 17,2 17,5 ccw L -20,5 Z")
        .with("names", names);
    made(&mut session, "profile", outline);
    made(&mut session, "extrude", Params::new().with("id", "b").with("inputs", "outline").with("h", 20.0));
    session
}

/// Calls a tool that must succeed.
fn made(session: &mut Session, tool: &str, args: Params) {
    let reply = session.call(tool, &args);
    assert!(reply.ok, "{tool}: {}", reply.text);
}

/// Asks a query about the node `b` that must be answered.
fn asked(session: &mut Session, query: &str, args: Params) -> Reply {
    let reply = session.call(query, &args.with("node", "b"));
    assert!(reply.ok, "{query}: {}", reply.text);
    reply
}

fn entry<'a>(value: &'a Value, name: &str) -> &'a Value {
    let Value::Map(entries) = value else { panic!("not a map: {value:?}") };
    entries.get(name).unwrap_or_else(|| panic!("no `{name}` in {value:?}"))
}

fn items(value: &Value) -> &[Value] {
    let Value::List(items) = value else { panic!("not a list: {value:?}") };
    items
}

fn word(value: &Value) -> &str {
    let Value::Text(text) = value else { panic!("not a text: {value:?}") };
    text
}

/// The names of the faces a `find` reply lists, best first.
fn found(reply: &Reply) -> Vec<&str> {
    items(entry(&reply.data, "matches")).iter().map(|found| word(entry(found, "face"))).collect()
}

#[test]
fn every_query_is_a_method_that_says_what_it_is_for() {
    let session = session();

    let methods = session.methods();

    for query in ["bounds", "brief", "edges", "faces", "find", "here", "keys", "measure", "ray"] {
        let method = methods.iter().find(|method| method.name == query);
        let method = method.unwrap_or_else(|| panic!("no method `{query}`"));
        assert_eq!(method.kind, MethodKind::Query, "{query}");
        assert!(!method.schema.about.is_empty(), "{query}");
    }
}

#[test]
fn a_ray_reads_as_a_sentence() {
    let mut session = with_block();

    let reply = asked(
        &mut session,
        "ray",
        Params::new().with("origin", [0.0, -50.0, 10.0]).with("direction", [0.0, 1.0, 0.0]),
    );

    assert_eq!(reply.text, "air 45 > [front] solid 10 > [back] air");
    assert_eq!(reply.exact, Exactness::Exact);
}

#[test]
fn the_view_from_a_face_reports_the_thickness_behind_it_and_how_its_edges_meet() {
    let mut session = with_rounded_block();

    let from_top = asked(&mut session, "here", Params::new().with("face", "top"));
    let from_back = asked(&mut session, "here", Params::new().with("face", "back"));

    assert!(from_top.text.starts_with("here: top [plane n=+z] at "), "{}", from_top.text);
    assert!(from_top.text.contains("behind: material 20 thick, then [bottom]"), "{}", from_top.text);
    assert!(from_top.text.contains("right|top -> right  convex 90°"), "{}", from_top.text);
    assert!(from_top.text.ends_with("in front: free"), "{}", from_top.text);
    assert!(from_back.text.contains("back|round -> round  tangent 180°"), "{}", from_back.text);
}

#[test]
fn a_search_finds_a_face_by_what_it_is_and_by_what_is_next_to_it() {
    let mut session = with_rounded_block();

    let round = asked(&mut session, "find", Params::new().with("query", "cylinder").with("k", 5.0));
    let planes =
        asked(&mut session, "find", Params::new().with("query", "plane near:kind:cylinder").with("k", 6.0));

    assert_eq!(found(&round), ["round"]);
    // The planes that touch the round face outrank the two that do not.
    let mut last_two = found(&planes)[4..].to_vec();
    last_two.sort_unstable();
    assert_eq!(last_two, ["front", "left"]);
}

#[test]
fn the_index_follows_an_edit_and_still_answers_for_the_version_before_it() {
    let mut session = with_block();
    let upward = Params::new().with("query", "n:+z");
    let before = asked(&mut session, "find", upward.clone());

    made(
        &mut session,
        "transform",
        Params::new().with("id", "b").with("turn", 180.0).with("about", [1.0, 0.0, 0.0]),
    );
    let after = asked(&mut session, "find", upward.clone());
    let of_first = session.call("find", &upward.with("node", "b@1"));

    assert_eq!((found(&before), found(&after), found(&of_first)), (vec!["top"], vec!["bottom"], vec!["top"]));
}

#[test]
fn rolling_back_restores_the_names_of_the_edges() {
    let mut session = with_block();
    session.call("mark", &Params::new());
    made(&mut session, "cylinder", Params::new().with("id", "b").with("r", 3.0).with("h", 10.0));
    let edited = asked(&mut session, "edges", Params::new());

    session.call("rollback", &Params::new());
    let restored = asked(&mut session, "edges", Params::new());

    let names = |reply: &Reply| -> Vec<String> {
        items(entry(&reply.data, "edges")).iter().map(|edge| word(entry(edge, "name")).to_string()).collect()
    };
    assert_eq!(names(&edited), ["bottom|side", "side|top"]);
    assert!(names(&restored).contains(&"front|top".to_string()), "{}", restored.text);
    assert_eq!(names(&restored).len(), 12);
}

#[test]
fn the_faces_of_a_node_are_listed_with_surface_and_a_point_on_each() {
    let mut session = with_block();

    let reply = asked(&mut session, "faces", Params::new());

    assert_eq!(reply.text.lines().count(), 6);
    assert!(reply.text.lines().any(|line| line == "top: plane n=+z at (0 0 20)"), "{}", reply.text);
    assert!(reply.text.lines().any(|line| line == "front: plane n=-y at (0 -5 10)"), "{}", reply.text);
}

#[test]
fn the_edges_of_a_node_are_listed_with_curve_and_length() {
    let mut session = with_block();

    let reply = asked(&mut session, "edges", Params::new());

    assert_eq!(reply.text.lines().count(), 12);
    assert!(
        reply
            .text
            .lines()
            .any(|line| line.starts_with("back|top: line along ") && line.ends_with(", length 40")),
        "{}",
        reply.text
    );
}

#[test]
fn the_numbers_of_a_node_are_read_on_one_line_with_how_exact_they_are() {
    let mut session = with_block();

    let reply = asked(&mut session, "measure", Params::new());

    assert_eq!(reply.text, "volume =8000 area =2800 faces 6 edges 12 vertices 8");
}

#[test]
fn the_reach_of_a_node_along_the_axes_is_read_on_one_line() {
    let mut session = with_block();

    let reply = asked(&mut session, "bounds", Params::new());

    assert_eq!(reply.text, "x -20..20  y -5..5  z 0..20");
}

#[test]
fn the_keys_of_a_face_are_the_words_a_search_can_use() {
    let mut session = with_rounded_block();

    let reply = asked(&mut session, "keys", Params::new().with("face", "round"));

    let first = reply.text.lines().next().unwrap();
    assert!(first.starts_with("round: axis:+z cell:L1:"), "{}", reply.text);
    assert!(first.ends_with(" kind:cylinder name:round r:3"), "{}", reply.text);
    let near: Vec<&str> = items(entry(&reply.data, "near")).iter().map(word).collect();
    assert_eq!(near.len(), 4);
    assert!(["back", "bottom", "right", "top"].iter().all(|name| near.contains(name)), "{near:?}");
}

#[test]
fn a_brief_tells_the_model_to_an_agent_that_opens_it() {
    let mut session = session();
    let text = "k3d 1\nlayer geo\nplate = box d=40 h=10 w=60\npeg = cylinder at=20,10,10 h=6 r=3\n\
                pegs = pattern peg n=3 step=-20,0,0\nlayer ai\nplate#top note=\"pegs stand on this face\"\n";
    assert!(session.call("open", &Params::new().with("text", text)).ok);

    let reply = session.call("brief", &Params::new());

    assert_eq!(
        reply.text,
        "plate@1 = box d=40 h=10 w=60 -> 6 faces, volume =24000\n\
         peg@1 = cylinder at=20,10,10 h=6 r=3 -> 3 faces, volume =169.646003\n\
         pegs@1 = pattern peg n=3 step=-20,0,0 -> 9 faces, volume =508.93801\n\
         layer ai: 1 line"
    );
}

#[test]
fn a_question_that_cannot_be_answered_comes_back_as_a_reply_that_says_why() {
    let mut session = with_block();

    let no_node = session.call("faces", &Params::new().with("node", "lid"));
    let no_face = session.call("here", &Params::new().with("node", "b").with("face", "lid"));
    let no_argument = session.call("ray", &Params::new().with("node", "b"));

    assert_eq!((no_node.ok, no_node.text.as_str()), (false, "not found: there is no node `lid`"));
    assert!(!no_face.ok && no_face.text.starts_with("not found: there is no face `lid`; the faces are "));
    assert_eq!(
        (no_argument.ok, no_argument.text.as_str()),
        (false, "input: `ray` needs `origin`: where the ray starts")
    );
}

#[test]
fn asking_changes_nothing() {
    let mut session = with_rounded_block();
    let saved = session.call("save", &Params::new()).text;

    for (query, args) in [
        ("faces", Params::new()),
        ("edges", Params::new()),
        ("measure", Params::new()),
        ("bounds", Params::new()),
        ("here", Params::new().with("face", "round")),
        ("keys", Params::new().with("face", "round")),
        ("find", Params::new().with("query", "plane")),
        ("ray", Params::new().with("origin", [0.0, 0.0, 50.0]).with("direction", [0.0, 0.0, -1.0])),
    ] {
        asked(&mut session, query, args);
    }
    assert!(session.call("brief", &Params::new()).ok);

    assert_eq!(session.call("save", &Params::new()).text, saved);
}

#[test]
fn a_batch_may_ask_right_after_it_makes() {
    let mut session = session();
    let call = |method: &str, args: Params| {
        Value::map([("method", Value::from(method)), ("args", args.into_value())])
    };
    let calls = vec![
        call("sphere", Params::new().with("id", "b").with("r", 5.0)),
        call("measure", Params::new().with("node", "b")),
        call(
            "ray",
            Params::new()
                .with("node", "b")
                .with("origin", [-20.0, 0.0, 0.0])
                .with("direction", [1.0, 0.0, 0.0]),
        ),
    ];

    let reply = session.call("batch", &Params::new().with("calls", calls));

    assert!(reply.ok, "{}", reply.text);
    assert!(
        reply.text.ends_with("faces 1 edges 0 vertices 0\nair 15 > [skin] solid 10 > [skin] air\n"),
        "{}",
        reply.text
    );
}

#[test]
fn mesh_preserves_legacy_text_defaults_exactness_and_flattened_triangles() {
    let mut session = session();
    made(&mut session, "box", Params::new().with("id", "part").with("w", 6.0).with("d", 4.0).with("h", 2.0));
    let default = session.call("mesh", &Params::new().with("node", "part"));
    let explicit = session
        .call("mesh", &Params::new().with("node", "part").with("tolerance", 0.05).with("triangles", false));
    assert_eq!(default, explicit);
    assert!(default.ok);
    assert_eq!(default.text, "12 triangles, 8 points, volume 48, area 88, 0 open sides");
    assert_eq!(default.exact, Exactness::Sampled { samples: 12 });
    assert_eq!(
        default.data,
        Value::map([
            ("area", 88.0.into()),
            ("open", 0.0.into()),
            ("points", 8.0.into()),
            ("triangles", 12.0.into()),
            ("volume", 48.0.into())
        ])
    );
    assert_eq!(session.call("mesh", &Params::new().with("node", "part@1").with("tolerance", 0.1)), default);
    let full = session.call("mesh", &Params::new().with("node", "part").with("triangles", true));
    assert!(full.ok);
    assert_eq!(full.text, default.text);
    assert_eq!(
        entry(&full.data, "coordinates"),
        &Value::List(
            [
                -3., -2., 0., 3., -2., 0., -3., -2., 2., 3., -2., 2., 3., 2., 0., 3., 2., 2., -3., 2., 0.,
                -3., 2., 2.
            ]
            .into_iter()
            .map(Value::from)
            .collect()
        )
    );
    assert_eq!(
        entry(&full.data, "corners"),
        &Value::List(
            [
                6., 1., 0., 1., 6., 4., 3., 5., 7., 7., 2., 3., 6., 0., 2., 2., 7., 6., 0., 1., 3., 3., 2.,
                0., 1., 4., 5., 5., 3., 1., 4., 6., 7., 7., 5., 4.
            ]
            .into_iter()
            .map(Value::from)
            .collect()
        )
    );
}

#[test]
fn mesh_preserves_legacy_missing_node_tolerance_type_and_unknown_parameter_errors() {
    let mut session = session();
    made(&mut session, "box", Params::new().with("id", "part").with("w", 6.0).with("d", 4.0).with("h", 2.0));
    for (params, kind, message) in [
        (Params::new().with("node", "missing"), "not found", "not found: there is no node `missing`"),
        (
            Params::new().with("node", "part").with("tolerance", 0.0),
            "input",
            "input: a mesh tolerance must be above the length tolerance, got 0",
        ),
        (
            Params::new().with("node", "part").with("triangles", "yes"),
            "input",
            "input: `mesh`: `triangles` must be true or false, got Text(\"yes\")",
        ),
        (
            Params::new().with("node", "part").with("extra", 2.0),
            "input",
            "input: `mesh` has no parameter `extra`; it takes node, tolerance, triangles",
        ),
    ] {
        let reply = session.call("mesh", &params);
        assert!(!reply.ok);
        assert_eq!(reply.text, message);
        assert_eq!(reply.data, Value::map([("error", Value::from(kind))]));
        assert_eq!(reply.exact, Exactness::Exact);
    }
}

#[test]
fn every_node_of_every_model_file_answers_every_question() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models");
    let mut files: Vec<_> = std::fs::read_dir(&folder).unwrap().map(|entry| entry.unwrap().path()).collect();
    files.retain(|file| file.extension().is_some_and(|extension| extension == "k3d"));
    assert!(!files.is_empty(), "no model files in {}", folder.display());
    for file in files {
        let mut session = session();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(session.call("open", &Params::new().with("text", text.as_str())).ok, "{}", file.display());
        let nodes: Vec<String> = session.kernel().model().nodes().map(|(name, _)| name.to_string()).collect();

        for node in &nodes {
            let about = Params::new().with("node", node.as_str());
            let faces: Vec<String> =
                session.kernel().body(node).unwrap().faces.iter().map(|face| face.name.clone()).collect();
            let mut questions = vec![
                ("faces", about.clone()),
                ("edges", about.clone()),
                ("measure", about.clone()),
                ("bounds", about.clone()),
                ("find", about.clone().with("query", "plane near:cylinder")),
            ];
            for face in &faces {
                questions.push(("here", about.clone().with("face", face.as_str())));
                questions.push(("keys", about.clone().with("face", face.as_str())));
            }

            for (query, args) in questions {
                let reply = session.call(query, &args);

                assert!(
                    reply.ok && !reply.text.is_empty(),
                    "{}: {query} {args:?}: {}",
                    file.display(),
                    reply.text
                );
            }
        }
    }
}
