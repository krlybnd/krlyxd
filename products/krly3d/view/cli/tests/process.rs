#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_view_api::wire::{decode_call, descriptor, encode_result};
use krly3d_view_api::{Diagnostic, RenderInput};
use krly3d_view_renderer::ViewEngine;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const BOX: &str = "k3d 1\nlayer geo\npart = box w=6 d=4 h=2\n";
const SPHERE: &str = "k3d 1\nlayer geo\npart = sphere r=3\n";
const MODEL_LIMIT: usize = 16 * 1024 * 1024;
const REQUEST_LIMIT: usize = 1024 * 1024;
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(model: &str) -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("view-cli-{}-{serial}", std::process::id()));
        fs::create_dir(&path).unwrap();
        let fixture = Self(path);
        fs::write(fixture.model(), model).unwrap();
        fixture
    }

    fn model(&self) -> PathBuf {
        self.0.join("model with spaces.k3d")
    }

    fn request(&self) -> PathBuf {
        self.0.join("request with spaces.json")
    }

    fn command(&self) -> Command {
        let mut command = command();
        command.arg("--model").arg(self.model());
        command
    }

    fn file_command(&self) -> Command {
        let mut command = self.command();
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
    let mut command = Command::new(env!("CARGO_BIN_EXE_krly3d-view"));
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    command
}

fn capture(mut command: Command, input: Vec<u8>) -> Output {
    let mut process = Process(command.spawn().unwrap());
    let mut stdin = process.0.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        // Early rejection can close stdin before an oversized request finishes.
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
    json!({"protocol": {"major": 1, "minor": 0}, "api": {"major": 1, "minor": 0},
        "request_id": "view-process-test", "input": {"node": "part", "width": 40, "height": 30}})
}

fn run(fixture: &Fixture, request: &Value) -> Output {
    capture(fixture.command(), request.to_string().into_bytes())
}

fn object_line(bytes: &[u8]) -> Value {
    assert!(bytes.ends_with(b"\n"));
    assert_eq!(bytes.iter().filter(|&&byte| byte == b'\n').count(), 1, "expected one JSON line");
    let value: Value = serde_json::from_slice(bytes).unwrap();
    assert!(value.is_object());
    value
}

fn result(output: &Output) -> Value {
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
    object_line(&output.stdout)
}

fn rejected(output: &Output, code: Option<&str>) -> Value {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "rejection wrote stdout");
    let diagnostic = object_line(&output.stderr);
    assert_eq!(diagnostic.as_object().unwrap().len(), 2);
    assert!(!diagnostic["code"].as_str().unwrap().is_empty());
    assert!(!diagnostic["message"].as_str().unwrap().is_empty());
    if let Some(code) = code {
        assert_eq!(diagnostic["code"], code, "{diagnostic}");
    }
    diagnostic
}

fn assert_diagnostic(output: &Output, diagnostic: Diagnostic) {
    assert_eq!(
        rejected(output, Some(diagnostic.code.as_str())),
        json!({"code": diagnostic.code.as_str(), "message": diagnostic.message})
    );
}

#[test]
fn describe_prints_the_shared_v1_schemas_and_synchronous_capabilities() {
    let mut command = command();
    command.arg("--describe");
    let description = result(&capture(command, Vec::new()));
    assert_eq!(description, descriptor());
    assert!(description["call_schema"].is_object());
    assert!(description["output_schema"].is_object());
    assert_eq!(description["protocol"], json!({"major": 1, "minor": 0}));
    assert_eq!(description["api"], json!({"major": 1, "minor": 0}));
    assert_eq!(description["synchronous"], true);
    assert_ne!(description["cancellation"], true);
    assert_ne!(description["timeouts"], true);
}

#[test]
fn help_explains_input_bounds_single_result_and_os_sigint_handling() {
    for option in ["--help", "-h"] {
        let mut command = command();
        command.arg(option);
        let output = capture(command, Vec::new());
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        for text in ["krly3d-view --model PATH", "16 MiB", "1 MiB", "UTF-8", "SIGINT", "default OS"] {
            assert!(help.contains(text), "help missing {text}");
        }
    }
}

#[test]
fn bad_arguments_are_json_diagnostics_without_results() {
    let cases: &[&[&str]] = &[
        &[],
        &["--model"],
        &["--model", ""],
        &["--model", "-"],
        &["--model", "--help"],
        &["--model", "-h"],
        &["--request"],
        &["--request", "-"],
        &["--model", "model", "--request"],
        &["--model", "model", "--request", ""],
        &["--model", "model", "--request", "--describe"],
        &["--model", "a", "--model", "b"],
        &["--request", "-", "--request", "-", "--model", "a"],
        &["--help", "--describe"],
        &["--describe", "--help"],
        &["--help", "extra"],
        &["--describe", "--model", "a"],
        &["--model", "a", "--describe"],
        &["--request", "-", "--help"],
        &["--model", "a", "--timeout", "1"],
        &["--model", "a", "--cancel"],
        &["--model=a"],
        &["model.k3d"],
        &["--unknown"],
    ];
    for args in cases {
        let mut command = command();
        command.args(*args);
        rejected(&capture(command, Vec::new()), Some("input"));
    }
}

fn cameras() -> Vec<Value> {
    let mut cameras: Vec<_> = ["iso", "front", "back", "left", "right", "top", "bottom"]
        .into_iter()
        .map(|name| json!({"kind": "named", "name": name}))
        .collect();
    cameras.push(json!({"kind": "orthographic", "toward": [2.0, -3.0, -1.0], "up": [0.0, 0.0, 1.0]}));
    let pinhole = json!({"kind": "pinhole", "eye": [10.0, -14.0, 9.0],
        "toward": [-10.0, 14.0, -8.0], "up": [0.0, 0.0, 1.0], "focal": 35.0});
    cameras.push(pinhole.clone());
    let mut offcentre = pinhole;
    offcentre["centre"] = json!([17.25, 10.75]);
    cameras.push(offcentre);
    cameras
}

fn assert_parity(output: &Output, engine: &ViewEngine, request: &Value) {
    let actual = result(output);
    let rendered = engine.call(decode_call(&request.to_string()).unwrap()).unwrap();
    assert!(rendered.png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(!rendered.faces.is_empty());
    assert_eq!(u32::from_be_bytes(rendered.png[16..20].try_into().unwrap()), rendered.width);
    assert_eq!(u32::from_be_bytes(rendered.png[20..24].try_into().unwrap()), rendered.height);
    let mut expected: Value = serde_json::from_str(&encode_result(&rendered).unwrap()).unwrap();
    assert_eq!(actual["image"]["mime_type"], "image/png");
    assert_eq!(actual["image"]["encoding"], "base64");
    assert!(!actual["image"]["data"].as_str().unwrap().is_empty());
    // Session/source identity and measured timing naturally vary across processes.
    // Everything else, including exact base64 PNG bytes and coverage, must match.
    for field in ["session", "source"] {
        let identity = &actual["provenance"]["source"][field];
        assert!(!identity.as_str().unwrap().is_empty());
        expected["provenance"]["source"][field] = identity.clone();
    }
    for field in ["render_ms", "encoding_ms"] {
        assert!(actual["timing"][field].as_f64().unwrap() >= 0.0);
    }
    expected["timing"] = actual["timing"].clone();
    assert_eq!(actual, expected);
}

#[test]
fn all_camera_forms_match_actual_engine_png_and_metadata_on_both_backends() {
    let fixture = Fixture::new(BOX);
    let engine = ViewEngine::from_k3d(BOX).unwrap();
    for backend in ["raycast", "mesh"] {
        for camera in cameras() {
            let mut request = request();
            request["input"]["backend"] = json!(backend);
            request["input"]["camera"] = camera;
            request["input"]["tolerance"] = json!({"value": 0.1, "unit": "model"});
            assert_parity(&run(&fixture, &request), &engine, &request);
        }
    }
}

#[test]
fn file_requests_support_spaces_reverse_option_order_and_explicit_versions() {
    let fixture = Fixture::new(SPHERE);
    let engine = ViewEngine::from_k3d(SPHERE).unwrap();
    for backend in ["raycast", "mesh"] {
        let mut request = request();
        request["input"]["node"] = json!("part@0001");
        request["input"]["backend"] = json!(backend);
        fs::write(fixture.request(), request.to_string()).unwrap();
        let mut command = command();
        command.arg("--request").arg(fixture.request()).arg("--model").arg(fixture.model());
        // A file request must not consume or parse unrelated stdin.
        let output = capture(command, b"not a JSON request".to_vec());
        assert_eq!(result(&output)["provenance"]["input"]["node"], "part@1");
        assert_parity(&output, &engine, &request);
    }
}

#[test]
fn default_and_explicit_stdin_eof_use_render_input_defaults() {
    let fixture = Fixture::new(BOX);
    let engine = ViewEngine::from_k3d(BOX).unwrap();
    let mut request = request();
    request["input"] = json!({"node": "part"});
    let call = decode_call(&request.to_string()).unwrap();
    assert_eq!(call.input, RenderInput::new("part"));
    for explicit_dash in [false, true] {
        let mut command = fixture.command();
        if explicit_dash {
            command.args(["--request", "-"]);
        }
        assert_parity(&capture(command, request.to_string().into_bytes()), &engine, &request);
    }
}

#[test]
fn malformed_unknown_versioned_and_timeout_requests_preserve_codec_diagnostics() {
    let fixture = Fixture::new(BOX);
    let valid = request().to_string();
    let mut cases = vec![
        String::new(),
        "{".to_owned(),
        "[]".to_owned(),
        "null".to_owned(),
        format!("{valid}\n{valid}"),
        format!(r#"{{"request_id":"duplicate",{}"#, &valid[1..]),
    ];
    for field in ["protocol", "api"] {
        for (major, minor) in [(0, 0), (1, 1), (2, 0)] {
            let mut request = request();
            request[field] = json!({"major": major, "minor": minor});
            cases.push(request.to_string());
        }
    }
    for field in ["protocol", "api", "request_id", "input"] {
        let mut request = request();
        request.as_object_mut().unwrap().remove(field);
        cases.push(request.to_string());
    }
    for (field, value) in [("timeout_ms", json!(0)), ("unknown", json!(true)), ("request_id", json!(""))] {
        let mut request = request();
        request[field] = value;
        cases.push(request.to_string());
    }
    for (field, value) in [
        ("node", json!("")),
        ("width", json!(0)),
        ("height", json!(2049)),
        ("width", json!(-1)),
        ("width", json!(1.5)),
        ("backend", json!("gpu")),
        ("unknown", json!(true)),
        ("tolerance", json!({"value": 0.0, "unit": "model"})),
        ("tolerance", json!({"value": 0.1, "unit": "metre"})),
        ("camera", json!({"kind": "named", "name": "unknown"})),
        ("camera", json!({"kind": "orthographic", "toward": [0, 0, 0], "up": [0, 0, 1]})),
        ("camera", json!({"kind": "named", "name": "iso", "eye": [1, 2, 3]})),
        (
            "camera",
            json!({"kind": "pinhole", "eye": [0, -10, 3],
            "toward": [0, 1, 0], "up": [0, 0, 1], "focal": 0}),
        ),
    ] {
        let mut request = request();
        request["input"][field] = value;
        cases.push(request.to_string());
    }
    for text in cases {
        let expected = decode_call(&text).unwrap_err();
        assert_diagnostic(&capture(fixture.command(), text.into_bytes()), expected);
    }
}

#[test]
fn renderer_rejections_preserve_diagnostics_and_never_publish_a_result() {
    let fixture = Fixture::new(BOX);
    let engine = ViewEngine::from_k3d(BOX).unwrap();
    let mut cases = Vec::new();
    for node in ["missing", "part@99", "part@0", "part@two", "part#top"] {
        let mut request = request();
        request["input"]["node"] = json!(node);
        cases.push(request);
    }
    let mut parallel = request();
    parallel["input"]["camera"] = json!({"kind": "orthographic", "toward": [0, 1, 0], "up": [0, 2, 0]});
    cases.push(parallel);
    let mut tiny = request();
    tiny["input"]["backend"] = json!("mesh");
    tiny["input"]["tolerance"] = json!({"value": 1e-30, "unit": "model"});
    cases.push(tiny);
    for request in cases {
        let expected = engine.call(decode_call(&request.to_string()).unwrap()).unwrap_err();
        assert_diagnostic(&run(&fixture, &request), expected);
    }
}

#[test]
fn invalid_models_and_failed_expectations_preserve_loading_diagnostics() {
    for model in ["", "not k3d", "k3d 1\nlayer geo\npart = box w=6 d=4 h=2\n  expect volume=999\n"] {
        let fixture = Fixture::new(model);
        let expected = ViewEngine::from_k3d(model).err().unwrap();
        assert_diagnostic(&run(&fixture, &request()), expected);
    }
}

#[test]
fn missing_files_are_io_diagnostics_and_decode_precedes_model_loading() {
    let fixture = Fixture::new(BOX);
    rejected(&capture(fixture.file_command(), Vec::new()), Some("io"));
    fs::remove_file(fixture.model()).unwrap();
    rejected(&run(&fixture, &request()), Some("io"));
    let expected = decode_call("{").unwrap_err();
    assert_diagnostic(&capture(fixture.command(), b"{".to_vec()), expected);
}

fn padded_request(length: usize) -> Vec<u8> {
    let mut request = request().to_string().into_bytes();
    request.resize(length, b' ');
    request
}

#[test]
fn request_file_and_stdin_limits_are_inclusive_and_include_whitespace() {
    let fixture = Fixture::new(BOX);
    for length in [REQUEST_LIMIT - 1, REQUEST_LIMIT, REQUEST_LIMIT + 1] {
        let bytes = padded_request(length);
        fs::write(fixture.request(), &bytes).unwrap();
        for output in [capture(fixture.file_command(), Vec::new()), capture(fixture.command(), bytes)] {
            if length <= REQUEST_LIMIT {
                assert_eq!(result(&output)["request_id"], "view-process-test");
            } else {
                let error = rejected(&output, Some("resource_limit"));
                assert!(error["message"].as_str().unwrap().contains("1048576"));
            }
        }
    }
}

#[test]
fn model_file_limit_is_inclusive_and_includes_whitespace() {
    let fixture = Fixture::new(BOX);
    let mut bytes = BOX.as_bytes().to_vec();
    for length in [MODEL_LIMIT - 1, MODEL_LIMIT, MODEL_LIMIT + 1] {
        bytes.resize(length, b' ');
        fs::write(fixture.model(), &bytes).unwrap();
        let output = run(&fixture, &request());
        if length <= MODEL_LIMIT {
            assert_eq!(result(&output)["request_id"], "view-process-test");
        } else {
            let error = rejected(&output, Some("resource_limit"));
            assert!(error["message"].as_str().unwrap().contains("16777216"));
        }
    }
}

#[test]
fn model_request_file_and_stdin_require_utf8() {
    let fixture = Fixture::new(BOX);
    fs::write(fixture.request(), [0xff, 0xfe]).unwrap();
    for output in [capture(fixture.file_command(), Vec::new()), capture(fixture.command(), vec![0xff, 0xfe])]
    {
        let diagnostic = rejected(&output, Some("input"));
        assert!(diagnostic["message"].as_str().unwrap().contains("UTF-8"));
    }
    fs::write(fixture.model(), [0xff, 0xfe]).unwrap();
    let diagnostic = rejected(&run(&fixture, &request()), Some("input"));
    assert!(diagnostic["message"].as_str().unwrap().contains("UTF-8"));
}

#[cfg(unix)]
#[test]
fn non_utf8_model_and_request_paths_are_supported() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let fixture = Fixture::new(BOX);
    let model_path = fixture.0.join(OsString::from_vec(b"model-\xff.k3d".to_vec()));
    let request_path = fixture.0.join(OsString::from_vec(b"request-\xff.json".to_vec()));
    fs::write(&model_path, BOX).unwrap();
    fs::write(&request_path, request().to_string()).unwrap();
    let mut command = command();
    command.arg("--model").arg(model_path).arg("--request").arg(request_path);
    assert_eq!(result(&capture(command, Vec::new()))["request_id"], "view-process-test");
}

#[test]
fn operation_mode_emits_one_started_and_one_terminal_with_exit_status() {
    let fixture = Fixture::new(BOX);
    for timeout in [None, Some(0)] {
        let mut call = krly3d_view_api::operation::OperationCall::new(
            krly3d_view_api::RequestId::new("process-operation"),
            RenderInput::new("part"),
        );
        call.input.width = 16;
        call.input.height = 12;
        call.timeout_ms = timeout;
        let mut command = fixture.command();
        command.arg("--operations");
        let output =
            capture(command, krly3d_view_api::operation::encode_operation_call(&call).unwrap().into_bytes());
        let text = String::from_utf8(output.stdout).unwrap();
        let events: Vec<Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["body"]["kind"], "started");
        assert_eq!(events[0]["operation_id"], events[1]["operation_id"]);
        assert_eq!(events[1]["seq"], 1);
        if timeout.is_some() {
            assert!(!output.status.success());
            assert_eq!(events[1]["body"]["diagnostic"]["code"], "deadline_exceeded");
        } else {
            assert!(output.status.success());
            assert!(output.stderr.is_empty());
            assert_eq!(events[1]["body"]["kind"], "completed");
            assert_eq!(events[1]["body"]["result"]["width"], 16);
        }
    }
}
