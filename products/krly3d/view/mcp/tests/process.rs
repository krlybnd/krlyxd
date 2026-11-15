#![allow(clippy::unwrap_used, clippy::panic)]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const WATCHDOG: Duration = Duration::from_secs(15);
const MODEL: &str = "k3d 1\nlayer geo\npart = box w=6 d=4 h=2\n";
const MAX_MODEL_BYTES: usize = 16 * 1024 * 1024;

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_krly3d-view-mcp"))
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/box.k3d")
}

struct Process {
    child: Child,
    input: Option<ChildStdin>,
    responses: Receiver<Value>,
}

impl Process {
    fn new(model: &Path) -> Self {
        let mut child = command()
            .arg("--model")
            .arg(model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = child.stdout.take().unwrap();
        let (send, responses) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let response = serde_json::from_str(&line.unwrap()).unwrap();
                if send.send(response).is_err() {
                    break;
                }
            }
        });
        Self { child, input, responses }
    }

    fn write(&mut self, text: &[u8]) {
        let input = self.input.as_mut().unwrap();
        input.write_all(text).unwrap();
        input.flush().unwrap();
    }

    fn send(&mut self, value: Value) {
        self.write(format!("{value}\n").as_bytes());
    }

    fn response(&self) -> Value {
        self.responses.recv_timeout(WATCHDOG).unwrap()
    }

    fn initialize(&mut self) {
        self.send(json!({"jsonrpc":"2.0","id":"hello","method":"initialize","params":{
            "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"process-test","version":"1"}
        }}));
        let response = self.response();
        assert_eq!(response["id"], "hello");
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(response["result"]["capabilities"], json!({"tools":{"listChanged":false}}));
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    }

    fn finish(&mut self) -> ExitStatus {
        self.input.take();
        let deadline = Instant::now() + WATCHDOG;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "MCP process did not exit");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn stderr(&mut self) -> String {
        let mut text = String::new();
        self.child.stderr.as_mut().unwrap().read_to_string(&mut text).unwrap();
        text
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn arguments(backend: &str) -> Value {
    json!({"protocol":{"major":1,"minor":0},"api":{"major":1,"minor":0},
        "request_id":"render-request","input":{"node":"part","width":64,"height":48,"backend":backend}})
}

#[test]
fn binary_handshake_list_real_renders_errors_and_eof() {
    let mut process = Process::new(&fixture());
    process.initialize();
    process.send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    let listed = process.response();
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 5);
    assert_eq!(listed["result"]["tools"][0]["name"], "view_render");
    for backend in ["raycast", "mesh"] {
        process.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"view_render","arguments":arguments(backend)}}));
        let response = process.response();
        assert_eq!(response["id"], 2);
        let result = &response["result"];
        assert_eq!(result["isError"], false, "{response}");
        let payload: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["request_id"], "render-request");
        assert_eq!(payload["width"], 64);
        assert_eq!(payload["height"], 48);
        assert_eq!(payload["provenance"]["input"]["backend"], backend);
        assert_eq!(payload["provenance"]["input"]["node"], "part@1");
        assert!(!payload["faces"].as_array().unwrap().is_empty());
        assert_eq!(result["content"][1]["type"], "image");
        assert_eq!(result["content"][1]["mimeType"], "image/png");
        assert_eq!(result["content"][1]["data"], payload["image"]["data"]);
        assert!(payload["image"]["data"].as_str().unwrap().starts_with("iVBORw0KGgo"));
    }
    let mut missing = arguments("raycast");
    missing["input"]["node"] = json!("missing");
    process.send(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"view_render","arguments":missing}}));
    assert_eq!(process.response()["result"]["isError"], true);
    process.send(json!({"jsonrpc":"2.0","id":4,"method":"no-such-method"}));
    assert_eq!(process.response()["error"]["code"], -32601);
    process.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}));
    process.send(json!({"jsonrpc":"2.0","id":5,"method":"ping"}));
    assert_eq!(process.response(), json!({"jsonrpc":"2.0","id":5,"result":{}}));
    assert!(process.finish().success());
    assert!(process.stderr().is_empty());
}

#[test]
fn binary_parse_errors_recover_and_large_frames_exit_with_clean_stdout() {
    let mut process = Process::new(&fixture());
    process.write(b"\xff\n{\n");
    for _ in 0..2 {
        assert_eq!(process.response()["error"]["code"], -32700);
    }
    process.send(json!({"jsonrpc":"2.0","id":9,"method":"ping"}));
    assert_eq!(process.response()["id"], 9);
    let mut oversized = vec![b' '; 1024 * 1024 + 1];
    oversized.push(b'\n');
    process.write(&oversized);
    assert!(!process.finish().success());
    assert!(process.stderr().contains("exceeds 1 MiB"));
    assert!(process.responses.recv_timeout(WATCHDOG).is_err());
}

#[test]
fn binary_rejects_unterminated_frames() {
    let mut process = Process::new(&fixture());
    process.write(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}");
    assert!(!process.finish().success());
    assert!(process.stderr().contains("unterminated"));
    assert!(process.responses.recv_timeout(WATCHDOG).is_err());
}

#[test]
fn cli_help_and_usage_keep_stdout_reserved_for_protocol() {
    let help = command().arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(help.stdout.is_empty());
    assert!(String::from_utf8(help.stderr).unwrap().contains("--model PATH"));
    for args in [
        vec![],
        vec!["--model"],
        vec!["--help", "extra"],
        vec!["--other"],
        vec!["--model", "unused", "extra"],
    ] {
        let output = command().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().contains("Usage:"));
    }
}

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempModel(PathBuf);

impl TempModel {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "krly3d-view-mcp-{}-{}.k3d",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for TempModel {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn cli_model_size_boundary_utf8_invalid_model_and_open_errors() {
    let model = TempModel::new();
    let missing = command().arg("--model").arg(&model.0).output().unwrap();
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8(missing.stderr).unwrap().contains("cannot open model"));
    for (bytes, message) in [(b"\xff".as_slice(), "not UTF-8"), (b"not k3d", "krly3d-view-mcp:")] {
        std::fs::write(&model.0, bytes).unwrap();
        let output = command().arg("--model").arg(&model.0).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().contains(message));
    }
    let mut exact = MODEL.as_bytes().to_vec();
    exact.resize(MAX_MODEL_BYTES, b' ');
    std::fs::write(&model.0, exact).unwrap();
    let output = command().arg("--model").arg(&model.0).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stdout.is_empty());
    let file = std::fs::OpenOptions::new().write(true).open(&model.0).unwrap();
    file.set_len(MAX_MODEL_BYTES as u64 + 1).unwrap();
    let output = command().arg("--model").arg(&model.0).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains("model exceeds 16 MiB"));
}
