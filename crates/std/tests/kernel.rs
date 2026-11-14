//! The kernel end to end: statements in, bodies and deltas out, text in between.

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::{Accuracy, ErrorKind};
use krly3d_kernel::model::Statement;
use krly3d_kernel::ports::measure::measure;
use krly3d_kernel::ports::read::read;
use krly3d_kernel::ports::write::{contract, write};
use krly3d_kernel::schema::Params;
use krly3d_kernel::Kernel;
use std::sync::Arc;

fn kernel() -> Kernel {
    let mut builder = krly3d_std::standard();
    builder.accuracy(Accuracy::for_scale(100.0).unwrap());
    builder.build()
}

fn plate() -> Statement {
    Statement::new("plate", "box", Params::new().with("w", 60.0).with("d", 40.0).with("h", 10.0), &[])
        .unwrap()
}

fn shifted(node: &str, from: &str) -> Statement {
    Statement::new(node, "transform", Params::new().with("move", [0.0, 0.0, 5.0]), &[from]).unwrap()
}

const MODEL: &str = "k3d 1
layer geo
plate = box d=40 h=10 w=60
hole = cylinder at=20,15,0 h=10 r=4
  expect faces=3
row = pattern hole n=3 step=-20,0,0
layer ai
plate#top note=\"datum face\"
";

#[test]
fn a_statement_becomes_a_body_and_a_delta_that_names_its_faces() {
    let mut kernel = kernel();

    let delta = kernel.apply(plate()).unwrap();

    assert_eq!(
        delta.text(),
        "ok plate@1 box. +back +bottom +front +left +right +top. faces 0>6. volume =24000\n"
    );
    assert_eq!(kernel.body("plate").unwrap().faces.len(), 6);
}

#[test]
fn defining_a_name_again_makes_a_new_version_and_keeps_the_old_one() {
    let mut kernel = kernel();
    kernel.apply(plate()).unwrap();

    let delta = kernel.apply(shifted("plate", "plate")).unwrap();

    assert_eq!(delta.text(), "ok plate@2 transform. faces 6>6. volume =24000\n");
    let height =
        |node: &str| kernel.body(node).unwrap().vertices.iter().map(|point| point.z).fold(f64::MIN, f64::max);
    assert_eq!((height("plate@1"), height("plate@2"), height("plate")), (10.0, 15.0, 15.0));
}

#[test]
fn a_dry_run_reports_the_change_and_keeps_nothing() {
    let kernel = kernel();

    let delta = kernel.try_out(plate()).unwrap();

    assert!(delta.text().starts_with("dry plate@1 box."), "{}", delta.text());
    assert_eq!(kernel.body("plate").unwrap_err().kind, ErrorKind::NotFound);
    assert_eq!(kernel.document().statements().count(), 0);
}

#[test]
fn rolling_back_returns_to_the_checkpoint() {
    let mut kernel = kernel();
    kernel.apply(plate()).unwrap();
    let checkpoint = kernel.checkpoint();
    kernel.apply(shifted("lifted", "plate")).unwrap();

    kernel.rollback(checkpoint);

    assert!(kernel.body("lifted").is_err());
    assert_eq!(kernel.document().statements().count(), 1);
}

#[test]
fn a_node_is_built_once_however_often_its_document_is_built() {
    let mut kernel = kernel();
    kernel.apply(plate()).unwrap();
    let first = kernel.body("plate").unwrap();
    let document = kernel.document().clone();

    kernel.apply(shifted("lifted", "plate")).unwrap();
    kernel.open(document).unwrap();

    assert!(Arc::ptr_eq(&first, &kernel.body("plate").unwrap()));
    assert_eq!(kernel.built_nodes(), 2);
}

#[test]
fn a_failing_statement_names_its_node_and_changes_nothing() {
    let mut kernel = kernel();

    let unknown_input = kernel.apply(shifted("lifted", "plate")).unwrap_err();
    let bad_parameter = kernel
        .apply(Statement::new("plate", "box", Params::new().with("w", 60.0), &[]).unwrap())
        .unwrap_err();

    assert_eq!(unknown_input.message, "lifted: `plate` is not defined before it is used");
    assert_eq!(bad_parameter.message, "plate: `box` needs `d`: depth along y");
    assert_eq!(kernel.document().statements().count(), 0);
}

#[test]
fn a_model_written_as_text_builds_what_the_text_says() {
    let mut kernel = kernel();
    let document = read(kernel.context(), "k3d", MODEL).into_result().unwrap();

    let delta = kernel.open(document).unwrap();

    assert_eq!(delta.nodes.len(), 3);
    let row = kernel.body("row").unwrap();
    assert_eq!(row.faces.len(), 9);
    let volume = measure(kernel.context(), "volume", &row).into_result().unwrap().value;
    assert!((volume - 3.0 * 160.0 * std::f64::consts::PI).abs() < 1e-9);
    assert_eq!(kernel.document().statements().nth(1).unwrap().expect, ["faces=3"]);
}

#[test]
fn writing_and_reading_back_gives_the_same_document_and_the_same_text() {
    let kernel = kernel();
    let document = read(kernel.context(), "k3d", MODEL).into_result().unwrap();

    contract::check(kernel.context(), "k3d", &document).unwrap();

    assert_eq!(write(kernel.context(), "k3d", &document).into_result().unwrap(), MODEL);
}

#[test]
fn a_saved_and_reloaded_model_is_the_same_model() {
    let mut first = kernel();
    first.open(read(first.context(), "k3d", MODEL).into_result().unwrap()).unwrap();
    let saved = write(first.context(), "k3d", first.document()).into_result().unwrap();

    let mut second = kernel();
    second.open(read(second.context(), "k3d", &saved).into_result().unwrap()).unwrap();

    for node in ["plate", "hole", "row"] {
        let (a, b) = (first.body(node).unwrap(), second.body(node).unwrap());
        let names = |body: &krly3d_kernel::topo::Body| {
            body.faces.iter().map(|face| face.name.clone()).collect::<Vec<_>>()
        };
        assert_eq!(names(&a), names(&b));
        assert_eq!(a.vertices, b.vertices);
        let volume = |body| measure(second.context(), "volume", body).into_result().unwrap().value;
        assert_eq!(volume(&a), volume(&b));
    }
}

#[test]
fn a_text_that_is_not_k3d_is_refused_with_its_line() {
    let kernel = kernel();
    let cases = [
        ("", "the text is empty; a k3d file starts with `k3d 1`"),
        ("k3d 2\n", "line 1: expected `k3d 1`, got `k3d 2`"),
        ("k3d 1\nplate = box w=1\n", "line 2: a `layer` line must come first"),
        ("k3d 1\nlayer geo\nplate box w=1\n", "line 3: expected `name = tool …`"),
        ("k3d 1\nlayer geo\n\nplate = box w=\"1\n", "line 4: a double quote is not closed"),
        ("k3d 1\nlayer geo\nexpect faces=6\n", "line 3: `expect` has no statement before it"),
    ];
    for (text, message) in cases {
        let error = read(kernel.context(), "k3d", text).into_result().unwrap_err();

        assert_eq!((error.kind, error.message.as_str()), (ErrorKind::Format, message));
    }
}

#[test]
fn an_expectation_that_holds_is_reported_and_the_edit_is_kept() {
    let mut kernel = kernel();
    let mut statement = plate();
    statement.expect = vec!["faces=6 volume=24000".into(), "adds=top,bottom".into()];

    let delta = kernel.apply(statement).unwrap();

    let reported =
        "expect plate@1 faces=6: ok\nexpect plate@1 volume=24000: ok\nexpect plate@1 adds=top,bottom: ok\n";
    assert!(delta.committed && delta.text().ends_with(reported), "{}", delta.text());
}

#[test]
fn an_edit_whose_expectation_fails_is_refused_and_says_what_was_found() {
    let mut kernel = kernel();
    let mut statement = plate();
    statement.expect = vec!["faces=8".into()];

    let delta = kernel.apply(statement).unwrap();

    assert!(delta.refused() && delta.text().starts_with("refused plate@1 box."), "{}", delta.text());
    assert!(delta.text().ends_with("expect plate@1 faces=8: FAILED, found =6\n"), "{}", delta.text());
    assert!(kernel.body("plate").is_err());
}

#[test]
fn a_signed_expectation_is_a_change_against_the_body_the_tool_was_given() {
    let mut kernel = kernel();
    kernel.apply(plate()).unwrap();
    let mut lifted = shifted("lifted", "plate");
    lifted.expect = vec!["faces=+0 volume=+0±1e-9 keeps=top,bottom".into(), "removes=top".into()];

    let delta = kernel.apply(lifted).unwrap();

    let holds: Vec<bool> = delta.verdicts.iter().map(|verdict| verdict.holds).collect();
    assert_eq!(holds, [true, true, true, false]);
    assert_eq!(delta.verdicts[3].found, "not so for top");
}

#[test]
fn an_expectation_that_cannot_be_read_fails_the_edit_and_names_its_node() {
    let mut kernel = kernel();
    let mut statement = plate();
    statement.expect = vec!["volume".into()];

    let error = kernel.apply(statement).unwrap_err();

    assert!(error.message.starts_with("plate@1: `volume` is not a claim"), "{error}");
}

fn rod(name: &str) -> Statement {
    Statement::new(name, "cylinder", Params::new().with("r", 0.8).with("h", 20.7846096908), &[]).unwrap()
}

#[test]
fn a_strut_turned_about_the_wrong_axis_is_refused_by_where_it_was_meant_to_end() {
    let mut kernel = kernel();
    kernel.apply(rod("rod")).unwrap();
    // Meant to run from (-8,8,38) to (-20,20,50); the axis (1,1,0) turns it the other way.
    let params = Params::new()
        .with("about", [1.0, 1.0, 0.0])
        .with("turn", 54.7356103172)
        .with("move", [-8.0, 8.0, 38.0]);
    let mut strut = Statement::new("strut", "transform", params, &["rod"]).unwrap();
    strut.expect = vec!["contains=-8,8,38 contains=-20,20,50".into()];

    let delta = kernel.apply(strut).unwrap();

    assert!(delta.refused(), "{}", delta.text());
    assert!(
        delta.text().ends_with("expect strut@1 contains=-20,20,50: FAILED, found outside\n"),
        "{}",
        delta.text()
    );
}

#[test]
fn a_strut_drawn_from_point_to_point_ends_where_it_was_meant_to_and_meets_the_frame() {
    let mut kernel = kernel();
    let corner = Params::new().with("w", 2.0).with("d", 2.0).with("h", 2.0).with("at", [-20.0, 20.0, 49.0]);
    kernel.apply(Statement::new("corner", "box", corner, &[]).unwrap()).unwrap();
    let params = Params::new().with("r", 0.8).with("at", [-8.0, 8.0, 38.0]).with("to", [-20.0, 20.0, 50.0]);
    let mut strut = Statement::new("strut", "cylinder", params, &[]).unwrap();
    strut.expect = vec!["contains=-8,8,38 contains=-20,20,50 meets=corner".into()];

    let delta = kernel.apply(strut).unwrap();

    assert!(delta.committed, "{}", delta.text());
}

#[test]
fn bodies_apart_do_not_meet() {
    let mut kernel = kernel();
    kernel.apply(plate()).unwrap();
    let ball = Params::new().with("r", 2.0).with("at", [0.0, 0.0, 30.0]);
    let mut statement = Statement::new("ball", "sphere", ball, &[]).unwrap();
    statement.expect = vec!["meets=plate".into()];

    let delta = kernel.apply(statement).unwrap();

    assert!(delta.refused() && !delta.verdicts[0].holds, "{}", delta.text());
}

#[test]
fn a_bent_bar_that_dips_into_a_ball_meets_it_though_neither_has_a_vertex() {
    let mut kernel = kernel();
    kernel.apply(Statement::new("ball", "sphere", Params::new().with("r", 3.0), &[]).unwrap()).unwrap();
    let points: Vec<krly3d_kernel::schema::Value> =
        [2.0, 0.0, 0.0, 6.0, 0.0, 0.0, 6.0, 0.0, 6.0].iter().map(|v| (*v).into()).collect();
    let bar = Params::new()
        .with("r", 0.5)
        .with("bend", 1.5)
        .with("points", krly3d_kernel::schema::Value::List(points));
    let mut statement = Statement::new("bar", "pipe", bar, &[]).unwrap();
    statement.expect = vec!["meets=ball".into()];

    let delta = kernel.apply(statement).unwrap();

    assert!(delta.committed, "{}", delta.text());
}
