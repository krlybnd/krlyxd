#![allow(clippy::unwrap_used, clippy::panic)]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const WATCHDOG: Duration = Duration::from_secs(15);

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_krly3d-mesh-mcp"))
}

struct Process {
    child: Child,
    input: Option<ChildStdin>,
    responses: Receiver<Value>,
    next_id: u64,
}

impl Process {
    fn new(model: &str) -> Self {
        let mut child = command()
            .arg("--model")
            .arg(fixture(model))
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
                let line = line.unwrap();
                let response = serde_json::from_str(&line).unwrap();
                if send.send(response).is_err() {
                    break;
                }
            }
        });
        let mut process = Self { child, input, responses, next_id: 1 };
        let response = process.rpc("initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"process-test","version":"1"}}));
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
        process.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        process
    }
    fn send(&mut self, request: Value) {
        writeln!(self.input.as_mut().unwrap(), "{request}").unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
    }
    fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        let response = self.responses.recv_timeout(WATCHDOG).unwrap();
        assert_eq!(response["id"], id, "{response}");
        response
    }
    fn tool(&mut self, name: &str, args: Value) -> (bool, Value) {
        let response = self.rpc("tools/call", json!({"name":name,"arguments":args}));
        assert!(response.get("error").is_none(), "{response}");
        let content = response["result"]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        let payload = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
        (response["result"]["isError"].as_bool().unwrap(), payload)
    }
    fn finish(&mut self) {
        self.input.take();
        assert!(wait(&mut self.child).success());
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + WATCHDOG;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("MCP process did not exit");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn args(request: &str) -> Value {
    json!({"request_id":request,"protocol":{"major":2,"minor":0},"api":{"major":2,"minor":0},"input":{"node":"part"}})
}

fn start(process: &mut Process, args: Value) -> String {
    let (error, payload) = process.tool("mesh", args);
    assert!(!error, "{payload}");
    payload["operation_id"].as_str().unwrap().to_owned()
}

fn drain(process: &mut Process, id: &str) -> (bool, Vec<Value>) {
    let deadline = Instant::now() + WATCHDOG;
    let mut events = Vec::new();
    let mut error = false;
    while events.len() < 2 {
        let (failed, payload) = process.tool("operation_events", json!({"operation_id":id}));
        error |= failed;
        events.extend(payload["events"].as_array().unwrap().clone());
        assert!(Instant::now() < deadline, "missing terminal event");
        if events.len() < 2 {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    (error, events)
}

#[test]
fn real_box_defaults_triangles_metadata_errors_release_and_reuse() {
    let mut process = Process::new("box.k3d");
    assert_eq!(process.rpc("tools/list", json!({}))["result"]["tools"].as_array().unwrap().len(), 4);
    for triangles in [false, true] {
        let mut request = args("same-domain-id");
        if triangles {
            request["input"]["triangles"] = json!(true);
        }
        let id = start(&mut process, request.clone());
        assert!(process.tool("mesh", request).0);
        let (error, events) = drain(&mut process, &id);
        assert!(!error);
        assert_box(&events, triangles);
        let handle = json!({"operation_id":id});
        assert_eq!(process.tool("operation_events", handle.clone()), (false, json!({"events":[]})));
        assert!(!process.tool("operation_cancel", handle.clone()).0);
        release(&mut process, handle.clone());
        let (error, payload) = process.tool("operation_events", handle);
        assert!(error);
        assert_eq!(payload["diagnostic"]["code"], "not_found");
    }
    process.finish();
}

fn assert_box(events: &[Value], triangles: bool) {
    assert_eq!((events[0]["seq"].as_u64(), events[1]["seq"].as_u64()), (Some(0), Some(1)));
    assert_eq!(events[0]["body"]["kind"], "started");
    assert_eq!(events[1]["body"]["kind"], "completed");
    let result = &events[1]["body"]["result"];
    assert_eq!(result["volume"], 48.0);
    assert_eq!(result["area"], 88.0);
    assert_eq!(result["open"], 0);
    assert_eq!(result["points"], 8);
    assert_eq!(result["triangles"], 12);
    assert_eq!(result["quality"]["exactness"], json!({"kind":"sampled","samples":12}));
    assert_eq!(result["quality"]["confidence"], Value::Null);
    assert_eq!(result["quality"]["positional_uncertainty"], Value::Null);
    assert_eq!(events[0]["body"]["provenance"], result["provenance"]);
    assert_eq!(result["provenance"]["input"]["node"], "part@1");
    assert_eq!(result["provenance"]["input"]["tolerance"], json!({"value":0.05,"unit":"model"}));
    assert_eq!(result["mesh"].is_object(), triangles);
    if triangles {
        assert_eq!(result["mesh"]["data"]["triangles"].as_array().unwrap().len(), 12);
    }
}

fn release(process: &mut Process, handle: Value) {
    let deadline = Instant::now() + WATCHDOG;
    loop {
        let (error, payload) = process.tool("operation_release", handle.clone());
        if !error {
            assert_eq!(payload["released"], true);
            break;
        }
        assert!(Instant::now() < deadline);
    }
}

#[test]
fn zero_deadline_is_accepted_and_failed_event_is_an_error_outcome() {
    let mut process = Process::new("box.k3d");
    let mut request = args("deadline");
    request["timeout_ms"] = json!(0);
    let id = start(&mut process, request);
    let (error, events) = drain(&mut process, &id);
    assert!(error);
    assert_eq!(events[1]["body"]["kind"], "failed");
    assert_eq!(events[1]["body"]["diagnostic"]["code"], "deadline_exceeded");
    process.finish();
}

#[test]
fn ping_cancel_and_eof_work_during_real_sphere_meshing() {
    let mut process = Process::new("sphere.k3d");
    let mut request = args("busy");
    request["input"]["tolerance"] = json!({"value":0.000001,"unit":"model"});
    let id = start(&mut process, request);
    assert_eq!(process.rpc("ping", json!({}))["result"], json!({}));
    assert!(!process.tool("operation_cancel", json!({"operation_id":id})).0);
    let (error, events) = drain(&mut process, &id);
    assert!(error);
    assert_eq!(events[1]["body"]["kind"], "cancelled");
    let mut request = args("eof");
    request["input"]["tolerance"] = json!({"value":0.000001,"unit":"model"});
    start(&mut process, request);
    process.finish();
}

#[test]
#[cfg(unix)]
fn closed_stdout_is_detected_while_stdin_remains_open() {
    let mut child = command()
        .arg("--model")
        .arg(fixture("sphere.k3d"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    writeln!(input,"{}",json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0"}}})).unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","method":"notifications/initialized"})).unwrap();
    let mut request = args("broken");
    request["input"]["tolerance"] = json!({"value":0.000001,"unit":"model"});
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"mesh","arguments":request}})
    )
    .unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert!(serde_json::from_str::<Value>(&line).unwrap()["result"].is_object());
    drop(output);
    // Keep stdin alive: the Unix stdout watcher must terminate the server.
    assert!(!wait(&mut child).success());
    drop(input);
}

#[cfg(unix)]
fn full_output() -> (std::os::unix::net::UnixStream, std::os::fd::OwnedFd) {
    let (reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
    writer.set_nonblocking(true).unwrap();
    loop {
        match writer.write(&[b'x'; 8192]) {
            Ok(count) => assert!(count > 0),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("cannot fill output socket: {error}"),
        }
    }
    writer.set_nonblocking(false).unwrap();
    (reader, writer.into())
}

#[test]
#[cfg(unix)]
fn eof_exits_with_stdout_fully_backpressured() {
    let (_unread, output) = full_output();
    let mut child = command()
        .arg("--model")
        .arg(fixture("sphere.k3d"))
        .stdin(Stdio::piped())
        .stdout(Stdio::from(output))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input,"{}",json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0"}}})).unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","method":"notifications/initialized"})).unwrap();
    let mut request = args("unread");
    request["input"]["tolerance"] = json!({"value":0.000001,"unit":"model"});
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"mesh","arguments":request}})
    )
    .unwrap();
    // The first response cannot even start writing. EOF must still drop the
    // runner, end the grace period, and exit without taking stdout's lock.
    drop(input);
    let before = Instant::now();
    assert!(!wait(&mut child).success());
    assert!(before.elapsed() < Duration::from_secs(3));
}

#[test]
#[cfg(unix)]
fn full_stderr_cannot_delay_shutdown_diagnostics() {
    let (_unread, output) = full_output();
    let mut child = command()
        .arg("--unknown")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(output))
        .spawn()
        .unwrap();
    let before = Instant::now();
    assert!(!wait(&mut child).success());
    assert!(before.elapsed() < Duration::from_secs(3));
}

#[test]
fn model_read_is_bounded_and_expectation_failures_are_rejected() {
    let path = std::env::temp_dir().join(format!("mesh-mcp-model-{}.k3d", std::process::id()));
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(16 * 1024 * 1024 + 1).unwrap();
    drop(file);
    let output = command().arg("--model").arg(&path).output().unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains("model exceeds 16 MiB"));
    std::fs::write(&path, b"k3d 1\nlayer geo\npart = box d=4 h=2 w=6\n  expect faces=7\n").unwrap();
    let output = command().arg("--model").arg(&path).output().unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn invalid_cli_models_and_oversized_protocol_input_fail_without_stdout_noise() {
    for arguments in [vec![], vec!["--model"], vec!["--unknown"], vec!["--help", "extra"]] {
        let output = command().args(arguments).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let output = command().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    let output = command().args(["--model", "/nonexistent/mesh-fixture.k3d"]).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let mut child = command()
        .arg("--model")
        .arg(fixture("box.k3d"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let _ = input.write_all(&vec![b'x'; krly3d_mesh_mcp::MAX_MESSAGE_BYTES + 1]);
    drop(input);
    assert!(!wait(&mut child).success());
}
