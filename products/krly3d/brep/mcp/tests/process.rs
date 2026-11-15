#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_brep::planar::descriptor;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const WATCHDOG: Duration = Duration::from_secs(20);

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_krly3d-brep-mcp"))
}

// Process watchdog pattern adapted from products/krly3d/view/mcp/tests/process.rs.
struct Process {
    child: Child,
    input: Option<ChildStdin>,
    responses: Receiver<Value>,
}

impl Process {
    fn new() -> Self {
        let mut child =
            command().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
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

fn arguments() -> Value {
    json!({"protocol":"krly3d.brep.planar/1","request_id":"process-request",
        "input":{"polygons":[[[0,0],[6,0],[6,4],[0,4]]],"height":2,
            "length_unit":"model","source":"process-test"}})
}

#[test]
fn binary_no_init_then_handshake_list_errors_reuse_and_eof() {
    let mut process = Process::new();
    process.send(json!({"jsonrpc":"2.0","id":0,"method":"tools/list"}));
    assert_eq!(process.response()["error"]["code"], -32000);
    process.send(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"brep_planar","arguments":arguments()}}));
    assert_eq!(process.response()["error"]["code"], -32000);
    process.initialize();
    process.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let listed = process.response();
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(listed["result"]["tools"][0]["name"], "brep_planar");
    assert_eq!(listed["result"]["tools"][0]["inputSchema"], descriptor()["call_schema"]);
    let mut invalid = arguments();
    invalid["input"]["height"] = json!(-1);
    process.send(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"brep_planar","arguments":invalid}}));
    let response = process.response();
    assert_eq!(response["result"]["isError"], true);
    let diagnostic: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(diagnostic["diagnostic"]["code"].is_string());
    // Reuse both JSON-RPC and domain request IDs; neither retains operation state.
    for _ in 0..2 {
        process.send(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"brep_planar","arguments":arguments()}}));
        let response = process.response();
        assert_eq!(response["id"], 3);
        let result = &response["result"];
        assert_eq!(result["isError"], false, "{response}");
        assert_eq!(result["content"].as_array().unwrap().len(), 1);
        let payload: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["protocol"], "krly3d.brep.planar/1");
        assert_eq!(payload["request_id"], "process-request");
        assert_eq!(payload["node"], "part@1");
        assert!(payload["model"].as_str().unwrap().starts_with("k3d 1\n"));
    }
    process.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}));
    process.send(json!({"jsonrpc":"2.0","id":4,"method":"ping"}));
    assert_eq!(process.response(), json!({"jsonrpc":"2.0","id":4,"result":{}}));
    assert!(process.finish().success());
    assert!(process.stderr().is_empty());
}

#[test]
fn binary_recovers_from_parse_and_duplicate_errors_then_accepts_crlf() {
    let mut process = Process::new();
    process.write(b"\xff\n{\n");
    for _ in 0..2 {
        assert_eq!(process.response()["error"]["code"], -32700);
    }
    process.write(
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\",\"params\":{\"_meta\":{\"x\":1,\"x\":2}}}\n",
    );
    assert_eq!(process.response()["error"]["code"], -32600);
    process.write(b"{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\r\n");
    assert_eq!(process.response()["id"], 9);
    assert!(process.finish().success());
    assert!(process.stderr().is_empty());
}

#[test]
fn binary_accepts_exact_bound_and_rejects_oversize_and_unterminated_frames() {
    let mut process = Process::new();
    let mut exact = br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#.to_vec();
    exact.resize(1024 * 1024, b' ');
    exact.push(b'\n');
    process.write(&exact);
    assert_eq!(process.response()["result"], json!({}));
    let mut oversized = vec![b' '; 1024 * 1024 + 1];
    oversized.push(b'\n');
    process.write(&oversized);
    assert!(!process.finish().success());
    assert!(process.stderr().contains("exceeds 1 MiB"));
    assert!(process.responses.recv_timeout(WATCHDOG).is_err());
    let mut process = Process::new();
    process.write(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}");
    assert!(!process.finish().success());
    assert!(process.stderr().contains("unterminated"));
    assert!(process.responses.recv_timeout(WATCHDOG).is_err());
}

#[test]
fn empty_stdin_help_and_usage_keep_stdout_reserved_for_json_rpc() {
    let output = command().stdin(Stdio::null()).output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let help = command().arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(help.stdout.is_empty());
    let help = String::from_utf8(help.stderr).unwrap();
    assert!(help.contains("No in-flight request cancellation"));
    for args in [vec!["--model", "unused"], vec!["--help", "extra"], vec!["--request", "-"], vec!["--other"]]
    {
        let output = command().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().contains("Usage:"));
    }
}
