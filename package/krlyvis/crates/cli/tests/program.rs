//! The `krlyvis` program as a client sees it: the shell form, MCP, and krly3d asking it
//! about a picture over JSON-RPC while modelling.

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_cli::endpoint::Endpoint;
use krly3d_cli::peer::{Joined, Peer};
use krly3d_kernel::schema::Params;
use krlyvis_kernel::ports::sample::Canvas;
use krlyvis_kernel::Pixel;
use serde_json::{json, Value as Json};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const PROGRAM: &str = env!("CARGO_BIN_EXE_krlyvis");

/// A grey quadrilateral on a dark ground, written as a binary PGM file.
fn picture(name: &str) -> PathBuf {
    let mut canvas = Canvas::new((300, 250), 40.0);
    canvas.fill(
        &[Pixel::new(60.3, 50.7), Pixel::new(250.2, 80.1), Pixel::new(230.6, 200.4), Pixel::new(40.1, 170.9)],
        200.0,
    );
    let image = canvas.image().unwrap();
    let mut bytes = format!("P5 {} {} 255\n", image.width(), image.height()).into_bytes();
    bytes.extend(image.values().iter().map(|value| value.round().clamp(0.0, 255.0) as u8));
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn krly3d() -> krly3d_kernel::Session {
    krly3d_std::standard().session()
}

#[test]
fn krly3d_asks_krlyvis_about_a_picture_over_json_rpc_and_goes_on_modelling() {
    let path = picture("peer.pgm");
    let mut joined = Joined::new(krly3d());
    joined.join(Peer::spawn("vis", PROGRAM).unwrap().shared()).unwrap();

    let seen = joined.call("vis.analyse", &Params::new().with("path", path.to_str().unwrap()));
    let drawn = joined
        .call("box", &Params::new().with("id", "plate").with("w", 60.0).with("d", 40.0).with("h", 10.0));
    let listed = joined.call("methods", &Params::new());

    assert!(seen.ok, "{}", seen.text);
    assert!(seen.text.starts_with("image 300x250 · "), "{}", seen.text);
    assert!(drawn.ok, "{}", drawn.text);
    assert!(
        listed.text.lines().any(|line| line.starts_with("vis.analyse (query) [path] [node]")),
        "{}",
        listed.text
    );
}

#[test]
fn a_failed_question_to_krlyvis_comes_back_as_a_failed_reply() {
    let mut joined = Joined::new(krly3d());
    joined.join(Peer::spawn("vis", PROGRAM).unwrap().shared()).unwrap();

    let reply = joined.call("vis.analyse", &Params::new().with("path", "/nonexistent/photo.pgm"));

    assert!(!reply.ok);
    assert!(reply.text.starts_with("not found: /nonexistent/photo.pgm"), "{}", reply.text);
}

#[test]
fn the_shell_form_prints_the_report() {
    let path = picture("shell.pgm");

    let output = Command::new(PROGRAM)
        .args(["call", "analyse", &format!("path={}", path.display()), "show=2"])
        .output()
        .unwrap();

    let text = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success());
    assert!(text.starts_with("image 300x250 · "), "{text}");
    assert!(text.contains("shorter segments not shown"), "{text}");
}

#[test]
fn mcp_lists_analyse_as_a_read_only_tool() {
    let mut child =
        Command::new(PROGRAM).arg("--mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(
        input,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#
    )
    .unwrap();
    writeln!(input, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#).unwrap();
    drop(input);

    let output = child.wait_with_output().unwrap();
    let answers: Vec<Json> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    assert_eq!(answers[0]["result"]["serverInfo"]["name"], json!("krlyvis"));
    let tools = answers[1]["result"]["tools"].as_array().unwrap();
    let analyse = tools.iter().find(|tool| tool["name"] == json!("analyse")).unwrap();
    assert_eq!(analyse["annotations"]["readOnlyHint"], json!(true));
    assert!(
        analyse["inputSchema"]["properties"]["path"].is_object()
            && analyse["inputSchema"]["properties"]["node"].is_object()
    );
}
