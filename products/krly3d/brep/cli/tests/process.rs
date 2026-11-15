#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_brep::planar::{decode_call, descriptor, encode_result};
use krly3d_brep::Diagnostic;
use krly3d_brep_engine::build_planar;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const LIMIT: usize = 1024 * 1024;
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!("brep-cli-{}-{serial}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }

    fn request(&self) -> PathBuf {
        self.0.join("request with spaces.json")
    }

    fn command(&self) -> Command {
        let mut command = command();
        command.arg("--request").arg(self.request());
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Process(Child);

impl Process {
    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "CLI did not exit within thirty seconds");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_krly3d-brep"));
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    command
}

fn capture(mut command: Command, input: Vec<u8>) -> Output {
    let mut process = Process(command.spawn().unwrap());
    let mut stdin = process.0.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        // A rejected invocation may close stdin before the writer completes.
        let _ = stdin.write_all(&input);
    });
    let mut stdout = process.0.stdout.take().unwrap();
    let output = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let mut stderr = process.0.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let status = process.wait();
    writer.join().unwrap();
    Output { status, stdout: output.join().unwrap(), stderr: errors.join().unwrap() }
}

fn request() -> Value {
    json!({"protocol":"krly3d.brep.planar/1","request_id":"cli-request",
        "input":{"polygons":[[[0,0],[6,0],[6,4],[0,4]]],"height":2,
            "length_unit":"model","source":"cli-test"}})
}

fn object_line(bytes: &[u8]) -> Value {
    assert!(bytes.ends_with(b"\n"));
    assert_eq!(bytes.iter().filter(|&&byte| byte == b'\n').count(), 1);
    let value: Value = serde_json::from_slice(bytes).unwrap();
    assert!(value.is_object());
    value
}

fn result(output: &Output) -> Value {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stderr.is_empty());
    object_line(&output.stdout)
}

fn rejected(output: &Output, code: Option<&str>) -> Value {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostic = object_line(&output.stderr);
    assert_eq!(diagnostic.as_object().unwrap().len(), 2);
    assert!(!diagnostic["code"].as_str().unwrap().is_empty());
    assert!(!diagnostic["message"].as_str().unwrap().is_empty());
    if let Some(code) = code {
        assert_eq!(diagnostic["code"], code);
    }
    diagnostic
}

fn assert_diagnostic(output: &Output, diagnostic: Diagnostic) {
    assert_eq!(rejected(output, None), json!({"code":diagnostic.code.as_str(),"message":diagnostic.message}));
}

#[test]
fn describe_is_the_same_api_descriptor_and_help_states_the_sync_contract() {
    let mut describe = command();
    describe.arg("--describe");
    let value = result(&capture(describe, Vec::new()));
    assert_eq!(value, descriptor());
    assert!(value["call_schema"].is_object());
    for option in ["--help", "-h"] {
        let mut help = command();
        help.arg(option);
        let output = capture(help, Vec::new());
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        for fragment in
            ["--request PATH|-", "--describe", "UTF-8", "1 MiB", "no in-flight request cancellation"]
        {
            assert!(text.contains(fragment), "help missing {fragment}");
        }
    }
}

#[test]
fn stdin_explicit_dash_and_file_build_native_models_using_the_public_engine() {
    let text = request().to_string();
    let expected = build_planar(decode_call(&text).unwrap()).unwrap();
    let encoded: Value = serde_json::from_str(&encode_result(&expected).unwrap()).unwrap();
    let fixture = Fixture::new();
    fs::write(fixture.request(), &text).unwrap();
    let mut dash = command();
    dash.args(["--request", "-"]);
    for output in [
        capture(command(), text.as_bytes().to_vec()),
        capture(dash, text.as_bytes().to_vec()),
        capture(fixture.command(), Vec::new()),
    ] {
        let actual = result(&output);
        for field in ["protocol", "request_id", "model", "node", "source", "quality"] {
            assert_eq!(actual[field], encoded[field], "field {field}");
        }
        assert_eq!(actual["node"], "part@1");
        assert!(actual["model"].as_str().unwrap().starts_with("k3d 1\n"));
        assert_eq!(actual["source"], "cli-test");
        assert!(actual["provenance"].is_object());
        assert!(actual["quality"].is_object());
    }
}

#[test]
fn bad_invocations_never_write_results() {
    let cases: &[&[&str]] = &[
        &["--request"],
        &["--request", ""],
        &["--request", "--help"],
        &["--request", "-h"],
        &["--request", "-", "--request", "-"],
        &["--help", "extra"],
        &["--describe", "--help"],
        &["--request", "-", "--describe"],
        &["--unknown"],
        &["request.json"],
        &["--request=-"],
    ];
    for args in cases {
        let mut command = command();
        command.args(*args);
        rejected(&capture(command, Vec::new()), Some("input"));
    }
}

#[test]
fn invalid_json_and_geometry_preserve_public_diagnostics() {
    let mut bad_height = request();
    bad_height["input"]["height"] = json!(0);
    let mut bad_unit = request();
    bad_unit["input"]["length_unit"] = json!("mm");
    let mut bad_polygon = request();
    bad_polygon["input"]["polygons"] = json!([[[0, 0], [1, 1], [0, 1], [1, 0]]]);
    let mut unknown = request();
    unknown["input"]["output_path"] = json!("must-not-be-written.k3d");
    for text in [
        "{".to_owned(),
        "{} {}".to_owned(),
        bad_height.to_string(),
        bad_unit.to_string(),
        bad_polygon.to_string(),
        unknown.to_string(),
    ] {
        let diagnostic = decode_call(&text).and_then(build_planar).unwrap_err();
        assert_diagnostic(&capture(command(), text.into_bytes()), diagnostic);
    }
}

#[test]
fn input_limits_include_whitespace_and_apply_to_files_and_stdin() {
    let fixture = Fixture::new();
    let mut exact = request().to_string().into_bytes();
    exact.resize(LIMIT, b' ');
    result(&capture(command(), exact.clone()));
    fs::write(fixture.request(), &exact).unwrap();
    result(&capture(fixture.command(), Vec::new()));
    exact.push(b' ');
    rejected(&capture(command(), exact.clone()), Some("resource_limit"));
    fs::write(fixture.request(), exact).unwrap();
    rejected(&capture(fixture.command(), Vec::new()), Some("resource_limit"));
    for bytes in [vec![0xff], vec![]] {
        rejected(&capture(command(), bytes.clone()), Some("input"));
        fs::write(fixture.request(), bytes).unwrap();
        rejected(&capture(fixture.command(), Vec::new()), Some("input"));
    }
    fs::remove_file(fixture.request()).unwrap();
    rejected(&capture(fixture.command(), Vec::new()), Some("io"));
}
