//! The `krly3d` program as a user or an agent starts it: a real process, real pipes.

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use serde_json::{json, Value as Json};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn model() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/pegboard.k3d")
}

/// Runs the program with the arguments, feeds it the lines and waits for it to end.
fn run(args: &[&str], lines: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_krly3d"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for line in lines {
        writeln!(input, "{line}").unwrap();
    }
    drop(input);
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

fn answers(output: &Output) -> Vec<Json> {
    text(&output.stdout).lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

#[test]
fn a_call_from_the_shell_prints_the_text_of_the_reply() {
    let output = run(&["call", "box", "id=plate", "w=60", "d=40", "h=10", "at=0,0,5"], &[]);

    assert_eq!(
        text(&output.stdout),
        "ok plate@1 box. +back +bottom +front +left +right +top. faces 0>6. volume =24000\n"
    );
    assert_eq!((output.status.code(), text(&output.stderr)), (Some(0), ""));
}

#[test]
fn a_failed_call_from_the_shell_says_why_on_standard_error_and_exits_with_1() {
    let output = run(&["call", "box", "id=plate", "w=60"], &[]);

    assert_eq!(text(&output.stderr), "input: plate: `box` needs `d`: depth along y\n");
    assert_eq!((output.status.code(), text(&output.stdout)), (Some(1), ""));
}

#[test]
fn a_model_file_is_loaded_before_the_call_is_made() {
    let model = model();

    let output = run(&["--open", model.to_str().unwrap(), "call", "save"], &[]);

    assert_eq!(text(&output.stdout), std::fs::read_to_string(&model).unwrap());
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn a_command_line_that_cannot_be_followed_exits_with_2_and_says_why() {
    let unknown = run(&["--verbose"], &[]);
    let missing = run(&["--open", "no-such-model.k3d"], &[]);

    assert_eq!(text(&unknown.stderr), "krly3d: unknown argument `--verbose`; see --help\n");
    assert!(text(&missing.stderr).starts_with("krly3d: no-such-model.k3d: "), "{}", text(&missing.stderr));
    assert_eq!((unknown.status.code(), missing.status.code()), (Some(2), Some(2)));
    assert_eq!((text(&unknown.stdout), text(&missing.stdout)), ("", ""));
}

#[test]
fn help_names_the_ways_of_running_the_program() {
    let output = run(&["--help"], &[]);

    let help = text(&output.stdout);
    assert!(
        help.contains("--open <file>") && help.contains("--mcp") && help.contains("call <method>"),
        "{help}"
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn the_plain_dialect_is_served_on_standard_input_and_output_until_the_input_ends() {
    let model = model();
    let lines = [
        r#"{"jsonrpc":"2.0","id":1,"method":"cylinder","params":{"id":"post","r":2,"h":30}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"view","params":{"nodes":["post"],"view":"top","width":40,"height":30}}"#,
        "this is not json",
    ];

    let output = run(&["--open", model.to_str().unwrap()], &lines);

    let answers = answers(&output);
    assert_eq!(answers.len(), 3);
    assert_eq!(answers[0]["result"]["data"]["id"], json!("post"));
    assert_eq!(answers[1]["result"]["data"]["faces"][0]["name"], json!("post#top"));
    assert_eq!(answers[2]["error"]["code"], json!(-32700));
    assert_eq!((output.status.code(), text(&output.stderr)), (Some(0), ""));
}

#[test]
fn an_mcp_session_draws_a_box_an_extrusion_and_a_revolution_and_gets_the_picture_back() {
    let tool = |id: u32, name: &str, arguments: Json| {
        json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": { "name": name, "arguments": arguments } })
        .to_string()
    };
    let lines = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.to_string(),
        tool(3, "box", json!({ "id": "plate", "w": 60, "d": 40, "h": 10 })),
        tool(4, "profile", json!({ "id": "outline", "path": "M -10,-5 L 10,-5 L 10,5 L -10,5 Z", "at": [0, 0, 10] })),
        tool(5, "extrude", json!({ "id": "rib", "inputs": ["outline"], "h": 8 })),
        tool(6, "profile", json!({ "id": "section", "path": "M 4,0 L 6,0 L 6,3 L 4,3 Z", "plane": "xz", "at": [20, 10, 10] })),
        tool(7, "revolve", json!({ "id": "ring", "inputs": ["section"] })),
        tool(8, "render", json!({ "nodes": ["plate", "rib", "ring"], "width": 120, "height": 90 })),
    ];
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();

    let output = run(&["--mcp"], &lines);

    let answers = answers(&output);
    assert_eq!(answers.len(), 8, "{}", text(&output.stdout));
    assert_eq!(answers[0]["result"]["serverInfo"]["name"], json!("krly3d"));
    assert!(answers[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == json!("render")));
    for made in &answers[2..7] {
        assert_eq!(made["result"]["isError"], json!(false), "{made}");
    }
    let content = answers[7]["result"]["content"].as_array().unwrap();
    assert!(content[0]["text"].as_str().unwrap().starts_with("render iso 120x90 of plate, rib, ring: "));
    assert_eq!((&content[1]["type"], &content[1]["mimeType"]), (&json!("image"), &json!("image/png")));
    assert!(content[1]["data"].as_str().unwrap().starts_with("iVBORw0KGgo"));
    assert_eq!((output.status.code(), text(&output.stderr)), (Some(0), ""));
}

#[test]
fn a_listening_server_keeps_its_model_from_one_client_to_the_next() {
    let socket =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("listen-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let path = socket.to_str().unwrap();
    let mut server = Command::new(env!("CARGO_BIN_EXE_krly3d")).args(["--listen", path]).spawn().unwrap();
    // The server is ready once it can be connected to; it is given a few seconds at most.
    let ready = (0..200).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::os::unix::net::UnixStream::connect(&socket).is_ok()
    });

    let made = ready.then(|| {
        run(
            &["send", path],
            &[r#"{"jsonrpc":"2.0","id":1,"method":"box","params":{"id":"plate","w":6,"d":4,"h":1}}"#],
        )
    });
    let saved =
        ready.then(|| run(&["send", path], &[r#"{"jsonrpc":"2.0","id":2,"method":"save","params":{}}"#]));
    server.kill().unwrap();
    server.wait().unwrap();

    assert!(ready, "the server did not start listening");
    assert_eq!(answers(&made.unwrap())[0]["result"]["ok"], true);
    let saved = answers(&saved.unwrap());
    assert!(saved[0]["result"]["text"].as_str().unwrap().contains("plate = box d=4 h=1 w=6"), "{saved:?}");
}
