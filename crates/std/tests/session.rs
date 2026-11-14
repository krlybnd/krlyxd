//! The kernel through its one door: `Session::methods` and `Session::call`.

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::Accuracy;
use krly3d_kernel::schema::{Params, Value};
use krly3d_kernel::{MethodKind, Session};

fn session() -> Session {
    let mut builder = krly3d_std::standard();
    builder.accuracy(Accuracy::for_scale(100.0).unwrap());
    builder.session()
}

fn plate() -> Params {
    Params::new().with("id", "plate").with("w", 60.0).with("d", 40.0).with("h", 10.0)
}

fn call(method: &str, args: Params) -> Value {
    Value::map([("method", Value::from(method)), ("args", args.into_value())])
}

fn names(list: &Value) -> Vec<&str> {
    let Value::List(items) = list else { panic!("not a list: {list:?}") };
    items
        .iter()
        .map(|item| if let Value::Text(text) = item { text.as_str() } else { panic!("not a text: {item:?}") })
        .collect()
}

fn entry<'a>(value: &'a Value, name: &str) -> &'a Value {
    let Value::Map(entries) = value else { panic!("not a map: {value:?}") };
    entries.get(name).unwrap_or_else(|| panic!("no `{name}` in {value:?}"))
}

#[test]
fn every_registered_tool_is_a_method_with_its_own_and_the_common_arguments() {
    let session = session();

    let methods = session.methods();

    let cuboid = methods.iter().find(|method| method.name == "box").unwrap();
    let arguments: Vec<&str> = cuboid.schema.params.iter().map(|param| param.name).collect();
    assert_eq!(cuboid.kind, MethodKind::Tool);
    assert_eq!(arguments, ["w", "d", "h", "at", "id", "inputs", "dry", "expect"]);
    for verb in ["methods", "open", "save", "mark", "rollback", "batch"] {
        assert!(
            methods.iter().any(|method| method.name == verb && method.kind == MethodKind::Session),
            "no `{verb}`"
        );
    }
}

#[test]
fn a_tool_call_defines_a_node_and_reports_its_faces_as_text_and_as_data() {
    let mut session = session();

    let reply = session.call("box", &plate());

    assert!(reply.ok, "{}", reply.text);
    assert_eq!(
        reply.text,
        "ok plate@1 box. +back +bottom +front +left +right +top. faces 0>6. volume =24000\n"
    );
    let Value::List(nodes) = entry(&reply.data, "nodes") else { panic!() };
    assert_eq!(names(entry(&nodes[0], "added")), ["back", "bottom", "front", "left", "right", "top"]);
    assert_eq!(entry(&reply.data, "id"), &Value::from("plate"));
}

#[test]
fn a_tool_call_without_an_id_is_given_an_unused_name() {
    let mut session = session();
    let unnamed = Params::new().with("w", 1.0).with("d", 1.0).with("h", 1.0);

    let (first, second) = (session.call("box", &unnamed), session.call("box", &unnamed));

    assert_eq!(
        (entry(&first.data, "id"), entry(&second.data, "id")),
        (&Value::from("box1"), &Value::from("box2"))
    );
}

#[test]
fn a_tool_that_changes_a_body_reads_the_node_it_defines_when_given_no_input() {
    let mut session = session();
    session.call("box", &plate());

    let reply = session.call("transform", &Params::new().with("id", "plate").with("move", [0.0, 0.0, 5.0]));

    assert_eq!(reply.text, "ok plate@2 transform. faces 6>6. volume =24000\n");
}

#[test]
fn a_dry_call_reports_the_change_and_keeps_nothing() {
    let mut session = session();

    let reply = session.call("box", &plate().with("dry", true));

    assert!(reply.ok && reply.text.starts_with("dry plate@1 box."), "{}", reply.text);
    assert!(session.kernel().body("plate").is_err());
}

#[test]
fn whatever_goes_wrong_comes_back_as_a_reply_that_says_why() {
    let mut session = session();

    let unknown = session.call("frobnicate", &Params::new());
    let incomplete = session.call("box", &Params::new().with("id", "plate").with("w", 60.0));
    let misnamed = session.call("box", &plate().with("height", 3.0));

    assert_eq!(
        (unknown.ok, unknown.text.as_str()),
        (false, "not found: there is no method `frobnicate`; `methods` lists them")
    );
    assert_eq!(
        (incomplete.ok, incomplete.text.as_str()),
        (false, "input: plate: `box` needs `d`: depth along y")
    );
    assert_eq!(entry(&incomplete.data, "error"), &Value::from("input"));
    assert_eq!(misnamed.text, "input: plate: `box` has no parameter `height`; it takes w, d, h, at");
}

#[test]
fn a_saved_document_opens_as_the_same_model() {
    let mut first = session();
    first.call("box", &plate());
    first.call(
        "cylinder",
        &Params::new().with("id", "peg").with("r", 3.0).with("h", 6.0).with("at", [0.0, 0.0, 10.0]),
    );
    let saved = first.call("save", &Params::new());

    let mut second = session();
    let opened = second.call("open", &Params::new().with("text", saved.text.as_str()));

    assert!(opened.ok, "{}", opened.text);
    assert_eq!(
        saved.text,
        "k3d 1\nlayer geo\nplate = box d=40 h=10 w=60\npeg = cylinder at=0,0,10 h=6 r=3\n"
    );
    assert_eq!(second.call("save", &Params::new()).text, saved.text);
}

#[test]
fn rolling_back_to_a_mark_drops_what_came_after_it() {
    let mut session = session();
    session.call("box", &plate());
    let mark = session.call("mark", &Params::new());
    session.call(
        "transform",
        &Params::new().with("id", "lifted").with("inputs", "plate").with("move", [0.0, 0.0, 5.0]),
    );

    let back = session.call("rollback", &Params::new());

    assert_eq!((mark.text.as_str(), back.text.as_str()), ("mark 1", "rolled back to mark 1"));
    assert!(session.kernel().body("lifted").is_err() && session.kernel().body("plate").is_ok());
    assert!(!session.call("rollback", &Params::new().with("mark", 7.0)).ok);
}

#[test]
fn a_batch_is_kept_whole_or_not_at_all() {
    let mut session = session();
    let peg = Params::new().with("id", "peg").with("r", 3.0).with("h", 6.0);
    let good = vec![call("box", plate()), call("cylinder", peg.clone())];
    let bad =
        vec![call("box", plate().with("id", "second")), call("cylinder", Params::new().with("id", "broken"))];

    let kept = session.call("batch", &Params::new().with("calls", good));
    let dropped = session.call("batch", &Params::new().with("calls", bad));

    assert!(kept.ok && !dropped.ok);
    assert!(dropped.text.ends_with("call 2 of 2 failed; nothing was kept\n"), "{}", dropped.text);
    assert!(session.kernel().body("peg").is_ok() && session.kernel().body("second").is_err());
}

#[test]
fn a_batch_may_contain_batches() {
    let mut session = session();
    let inner = call("batch", Params::new().with("calls", vec![call("box", plate())]));

    let reply = session.call("batch", &Params::new().with("calls", vec![inner, call("mark", Params::new())]));

    assert!(reply.ok, "{}", reply.text);
    assert!(session.kernel().body("plate").is_ok());
}

#[test]
fn an_expectation_given_with_a_call_decides_whether_the_change_is_kept() {
    let mut session = session();

    let kept = session
        .call("box", &plate().with("expect", vec![Value::from("faces=6"), Value::from("volume=24000")]));
    let refused = session.call("box", &plate().with("id", "other").with("expect", "faces=7"));

    assert!(kept.ok && !refused.ok);
    assert!(refused.text.ends_with("expect other@1 faces=7: FAILED, found =6\n"), "{}", refused.text);
    assert!(session.kernel().body("other").is_err());
}
