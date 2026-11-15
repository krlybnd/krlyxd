#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_mesh::operation::{self, encode_event, MeshEvent, MeshEventBody, OperationId};
use krly3d_mesh::{ArtifactId, Id, MeshCall, MeshInput, RequestId};
use krly3d_mesh_engine::MeshEngine;
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BOX: &str = "k3d 1\nlayer geo\npart = box w=6 d=4 h=2\n";
const SPHERE: &str = "k3d 1\nlayer geo\npart = sphere r=5\n";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(model: &str) -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let fixture = Self(std::env::temp_dir().join(format!("mesh-cli-{}-{serial}", std::process::id())));
        fs::create_dir(&fixture.0).unwrap();
        fs::write(fixture.model(), model).unwrap();
        fixture
    }

    fn model(&self) -> PathBuf {
        self.0.join("model with spaces.k3d")
    }

    fn command(&self) -> Command {
        let mut command = command();
        command.arg("--model").arg(self.model());
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_krly3d-mesh"));
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    command
}

fn request() -> Value {
    json!({"request_id": "process-test", "protocol": {"major": 2, "minor": 0},
        "api": {"major": 2, "minor": 0}, "input": {"node": "part"}})
}

fn wait(process: &mut Process) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "CLI did not exit within ten seconds");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn capture(mut command: Command, input: Vec<u8>) -> Output {
    let mut process = Process(command.spawn().unwrap());
    let mut stdin = process.0.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        // Rejection and bounded input may close the pipe before all bytes arrive.
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
    let status = wait(&mut process);
    writer.join().unwrap();
    Output { status, stdout: output.join().unwrap(), stderr: errors.join().unwrap() }
}

fn run(fixture: &Fixture, call: &Value) -> Output {
    capture(fixture.command(), call.to_string().into_bytes())
}

fn events(output: &Output, terminal: &str, status: i32) -> Vec<Value> {
    assert_eq!(output.status.code(), Some(status), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stdout.ends_with(b"\n"));
    let events: Vec<Value> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 2, "{events:?}");
    for (seq, event) in events.iter().enumerate() {
        assert_eq!(event["seq"], seq);
        assert_eq!(event["request_id"], "process-test");
        assert_eq!(event["operation_id"], events[0]["operation_id"]);
        assert_eq!(event["protocol"], json!({"major": 2, "minor": 0}));
        assert_eq!(event["api"], json!({"major": 2, "minor": 0}));
    }
    assert_eq!(events[0]["body"]["kind"], "started");
    assert_eq!(events[1]["body"]["kind"], terminal);
    events
}

fn rejected(output: &Output) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "rejection emitted events: {:?}", output.stdout);
    assert!(!output.stderr.is_empty());
}

fn started(process: &mut Process) -> (String, BufReader<ChildStdout>) {
    let stdout = process.0.stdout.take().unwrap();
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let _ = sent.send((line, reader));
    });
    let (line, reader) = received.recv_timeout(Duration::from_secs(5)).unwrap();
    let event: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(event["body"]["kind"], "started");
    (line, reader)
}

fn interrupt(process: &Process) {
    assert!(Command::new("kill").args(["-INT", &process.0.id().to_string()]).status().unwrap().success());
}

#[test]
fn describe_uses_the_v2_descriptor_and_shared_schemas() {
    let mut command = command();
    command.arg("--describe");
    let output = capture(command, Vec::new());
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let description: Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected = operation::descriptor();
    assert_eq!(description["api"], json!({"major": 2, "minor": 0}));
    assert_eq!(description["protocol"], description["api"]);
    assert_eq!(description["package"], expected.package.as_str());
    assert_eq!(description["engine_revision"], expected.engine_revision.as_str());
    assert_eq!(description["call_schema"], expected.call_schema);
    assert_eq!(description["input_schema"], expected.input_schema);
    assert_eq!(description["output_schema"], expected.output_schema);
    assert_eq!(description["supported_events"], json!(expected.supported_events));
    assert_eq!(description["synchronous"], false);
    assert_eq!(description["cancellation"], true);
    assert_eq!(description["timeouts"], true);
    assert_eq!(description["coordinates"], expected.coordinates);
}

#[test]
fn file_request_matches_engine_results_and_canonical_event_bytes() {
    let fixture = Fixture::new(BOX);
    let mut call = request();
    call["input"]["triangles"] = json!(true);
    call["input"]["node"] = json!("part@1");
    let path = fixture.0.join("request with spaces.json");
    fs::write(&path, call.to_string()).unwrap();
    let mut command = fixture.command();
    command.arg("--request").arg(path);
    let output = capture(command, Vec::new());
    let decoded = events(&output, "completed", 0);
    assert!(output.stderr.is_empty());

    let engine = MeshEngine::from_k3d(BOX).unwrap();
    let mut input = MeshInput::new("part@1");
    input.triangles = true;
    let mut result = engine.call(MeshCall::new(RequestId::new("process-test"), input)).unwrap();
    // Only session-generated identities differ across processes.
    result.provenance.source = Id::new(decoded[0]["body"]["provenance"]["source"].as_str().unwrap());
    result.mesh.as_mut().unwrap().id =
        ArtifactId::new(decoded[1]["body"]["result"]["mesh"]["id"].as_str().unwrap());
    assert_eq!((result.volume, result.area, result.open), (48.0, 88.0, 0));
    let operation_id = OperationId::new(decoded[0]["operation_id"].as_str().unwrap());
    let expected = [MeshEventBody::Started(result.provenance.clone()), MeshEventBody::Completed(result)];
    let mut canonical = String::new();
    for (seq, body) in expected.into_iter().enumerate() {
        let event = MeshEvent {
            request_id: RequestId::new("process-test"),
            operation_id: operation_id.clone(),
            seq: seq as u64,
            body,
        };
        canonical.push_str(&encode_event(&event).unwrap());
        canonical.push('\n');
    }
    assert_eq!(output.stdout, canonical.as_bytes());
}

#[test]
fn stdin_eof_completes_with_shared_defaults_and_no_triangles() {
    let fixture = Fixture::new(BOX);
    for explicit_dash in [false, true] {
        let mut command = fixture.command();
        if explicit_dash {
            command.args(["--request", "-"]);
        }
        let output = capture(command, request().to_string().into_bytes());
        let decoded = events(&output, "completed", 0);
        assert!(output.stderr.is_empty());
        let result = &decoded[1]["body"]["result"];
        assert_eq!(result["volume"], 48.0);
        assert_eq!(result["area"], 88.0);
        assert_eq!(result["mesh"], Value::Null);
        assert_eq!(result["provenance"]["input"]["node"], "part@1");
        assert_eq!(result["provenance"]["input"]["tolerance"], json!({"value": 0.05, "unit": "model"}));
        assert_eq!(result["quality"]["exactness"]["kind"], "sampled");
        assert_eq!(result["quality"]["positional_uncertainty"], Value::Null);
        assert_eq!(result["quality"]["confidence"], Value::Null);
    }
}

#[test]
fn sphere_preserves_mesh_statistics_and_units() {
    let fixture = Fixture::new(SPHERE);
    let mut call = request();
    call["input"]["tolerance"] = json!({"value": 0.1, "unit": "model"});
    let output = run(&fixture, &call);
    let decoded = events(&output, "completed", 0);
    let result = &decoded[1]["body"]["result"];
    let pi = std::f64::consts::PI;
    assert_eq!(result["open"], 0);
    assert!((result["volume"].as_f64().unwrap() - 4.0 * pi * 125.0 / 3.0).abs() <= 4.0 * pi * 25.0 * 0.1);
}

#[test]
fn strict_decode_and_validation_reject_without_started() {
    let fixture = Fixture::new(BOX);
    let mut cases = Vec::new();
    for version in [0, 1, 3] {
        let mut call = request();
        call["api"]["major"] = json!(version);
        cases.push(call);
    }
    for node in ["missing", "part@99"] {
        let mut call = request();
        call["input"]["node"] = json!(node);
        cases.push(call);
    }
    let mut unknown = request();
    unknown["input"]["surprise"] = json!(true);
    cases.push(unknown);
    let mut units = request();
    units["input"]["tolerance"] = json!({"value": 0.05, "unit": "metre"});
    cases.push(units);
    let mut tolerance = request();
    tolerance["input"]["tolerance"] = json!({"value": 0.0, "unit": "model"});
    cases.push(tolerance);
    for call in cases {
        rejected(&run(&fixture, &call));
    }
    let valid = request().to_string();
    for text in [
        String::new(),
        "{".into(),
        format!("{valid}\n{valid}"),
        format!(r#"{{"request_id":"duplicate",{}"#, &valid[1..]),
    ] {
        rejected(&capture(fixture.command(), text.into_bytes()));
    }
}

#[test]
fn immediate_deadline_is_accepted_then_fails() {
    let fixture = Fixture::new(BOX);
    let mut call = request();
    call["timeout_ms"] = json!(0);
    let output = run(&fixture, &call);
    let decoded = events(&output, "failed", 1);
    assert_eq!(decoded[1]["body"]["diagnostic"]["code"], "deadline_exceeded");
    assert!(String::from_utf8_lossy(&output.stderr).contains("deadline_exceeded"));
}

#[test]
fn file_errors_invalid_models_and_failed_expectations_reject() {
    for model in ["not k3d", "k3d 1\nlayer geo\npart = box w=6 d=4 h=2\n  expect volume=1\n"] {
        rejected(&run(&Fixture::new(model), &request()));
    }
    let fixture = Fixture::new(BOX);
    let mut command = fixture.command();
    command.args(["--request", "/no-such-mesh-request.json"]);
    rejected(&capture(command, Vec::new()));
    fs::remove_file(fixture.model()).unwrap();
    rejected(&run(&fixture, &request()));
}

#[test]
fn input_bounds_apply_to_stdin_request_files_and_models() {
    let fixture = Fixture::new(BOX);
    let valid = request().to_string();
    let mut bytes = valid.into_bytes();
    bytes.resize(1024 * 1024, b' ');
    events(&capture(fixture.command(), bytes.clone()), "completed", 0);
    bytes.push(b' ');
    rejected(&capture(fixture.command(), bytes.clone()));
    let path = fixture.0.join("large.json");
    fs::write(&path, bytes).unwrap();
    let mut command = fixture.command();
    command.arg("--request").arg(path);
    rejected(&capture(command, Vec::new()));
    let model = fs::OpenOptions::new().write(true).open(fixture.model()).unwrap();
    model.set_len(16 * 1024 * 1024 + 1).unwrap();
    rejected(&run(&fixture, &request()));
}

#[test]
fn invalid_utf8_is_rejected_for_both_inputs() {
    let fixture = Fixture::new(BOX);
    rejected(&capture(fixture.command(), vec![0xff]));
    fs::write(fixture.model(), [0xff]).unwrap();
    rejected(&run(&fixture, &request()));
}

#[test]
fn usage_errors_are_exit_one_with_no_events() {
    for args in [
        vec![],
        vec!["--model"],
        vec!["--model", "-"],
        vec!["--request", "-"],
        vec!["--bogus"],
        vec!["--describe", "--model", "x"],
        vec!["--model", "x", "--model", "y"],
        vec!["--model", "x", "--request", "-", "--request", "-"],
    ] {
        let mut command = command();
        command.args(args);
        rejected(&capture(command, Vec::new()));
    }
    let mut command = command();
    command.arg("--help");
    let output = capture(command, Vec::new());
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--describe"));
}

#[test]
fn sigint_after_started_emits_one_canonical_cancelled_terminal() {
    let fixture = Fixture::new(SPHERE);
    let mut call = request();
    call["input"]["tolerance"] = json!({"value": 0.000001, "unit": "model"});
    let mut process = Process(fixture.command().spawn().unwrap());
    process.0.stdin.take().unwrap().write_all(call.to_string().as_bytes()).unwrap();
    let (first, mut reader) = started(&mut process);
    interrupt(&process);
    let mut rest = String::new();
    let reader = std::thread::spawn(move || {
        reader.read_to_string(&mut rest).unwrap();
        rest
    });
    let status = wait(&mut process);
    let output = Output {
        status,
        stdout: format!("{first}{}", reader.join().unwrap()).into_bytes(),
        stderr: Vec::new(),
    };
    let decoded = events(&output, "cancelled", 130);
    let expected = MeshEvent {
        request_id: RequestId::new("process-test"),
        operation_id: OperationId::new(decoded[0]["operation_id"].as_str().unwrap()),
        seq: 1,
        body: MeshEventBody::Cancelled,
    };
    assert_eq!(
        std::str::from_utf8(&output.stdout).unwrap().lines().nth(1).unwrap(),
        encode_event(&expected).unwrap()
    );
}

#[test]
fn sigint_interrupts_an_unfinished_stdin_request() {
    let fixture = Fixture::new(BOX);
    let mut process = Process(fixture.command().spawn().unwrap());
    // This exceeds pipe capacity, so completion proves the CLI is reading input
    // and its SIGINT handler is installed. Keep stdin open after the write.
    process.0.stdin.as_mut().unwrap().write_all(&vec![b' '; 256 * 1024]).unwrap();
    interrupt(&process);
    assert_eq!(wait(&mut process).code(), Some(130));
    let mut stdout = String::new();
    process.0.stdout.take().unwrap().read_to_string(&mut stdout).unwrap();
    assert!(stdout.is_empty());
}

#[test]
fn closing_stdout_after_started_cancels_real_work_and_exits() {
    let fixture = Fixture::new(SPHERE);
    let mut call = request();
    call["input"]["tolerance"] = json!({"value": 0.000001, "unit": "model"});
    let mut process = Process(fixture.command().spawn().unwrap());
    process.0.stdin.take().unwrap().write_all(call.to_string().as_bytes()).unwrap();
    let (_, reader) = started(&mut process);
    drop(reader);
    assert_eq!(wait(&mut process).code(), Some(1));
    let mut stderr = String::new();
    process.0.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    assert!(stderr.contains("stdout"));
    // A fresh process can still use the same model normally.
    events(&run(&fixture, &request()), "completed", 0);
}

#[cfg(target_os = "linux")]
#[test]
fn sigint_exits_even_when_a_terminal_event_is_blocked_on_stdout() {
    use std::os::fd::AsRawFd;

    let fixture = Fixture::new(SPHERE);
    let mut call = request();
    call["input"]["triangles"] = json!(true);
    call["input"]["tolerance"] = json!({"value": 0.1, "unit": "model"});
    // Establish that this inexpensive real artifact cannot fit in our pipe,
    // even after consuming its first byte. No mesh timing assumption is needed.
    let completed = run(&fixture, &call);
    events(&completed, "completed", 0);
    let terminal_bytes = std::str::from_utf8(&completed.stdout).unwrap().lines().nth(1).unwrap().len();
    let mut process = Process(fixture.command().spawn().unwrap());
    let fd = process.0.stdout.as_ref().unwrap().as_raw_fd();
    // SAFETY: fd is a live, still-empty pipe owned by process.stdout. Shrink its
    // capacity below the final artifact size before allowing the request to run.
    let capacity = unsafe { libc::fcntl(fd, libc::F_SETPIPE_SZ, 4096) };
    assert!(capacity > 0);
    assert!(terminal_bytes > capacity as usize + 1);
    process.0.stdin.take().unwrap().write_all(call.to_string().as_bytes()).unwrap();
    let stdout = process.0.stdout.take().unwrap();
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || {
        // Capacity one prevents read-ahead into the terminal event. Drain
        // Started, then consume just the first terminal byte and stop reading.
        let mut reader = BufReader::with_capacity(1, stdout);
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        let mut byte = [0];
        reader.read_exact(&mut byte).unwrap();
        let _ = sent.send((first, byte, reader));
    });
    let (first, byte, _unread) = received.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&first).unwrap()["body"]["kind"], "started");
    assert_eq!(byte, [b'{']);
    interrupt(&process);
    assert_eq!(wait(&mut process).code(), Some(130));
}

#[test]
fn full_stderr_cannot_block_terminal_events_rejection_or_sigint_exit() {
    use std::fs::File;
    use std::os::fd::{AsRawFd, FromRawFd};

    let fixture = Fixture::new(BOX);
    for outcome in ["failed", "rejected", "cancelled"] {
        let mut descriptors = [0; 2];
        // SAFETY: pipe initializes exactly two integers; each resulting owned
        // descriptor is transferred to a File exactly once after success.
        assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
        let (_unread, mut writer) =
            unsafe { (File::from_raw_fd(descriptors[0]), File::from_raw_fd(descriptors[1])) };
        let fd = writer.as_raw_fd();
        // SAFETY: fd is a live pipe. These calls change only its status flags.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) }, 0);
        loop {
            match writer.write(b"x") {
                Ok(count) => assert!(count > 0),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("cannot fill stderr: {error}"),
            }
        }
        // SAFETY: restore the original flags before inheriting this descriptor.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFL, flags) }, 0);
        let mut command = fixture.command();
        command.stderr(Stdio::from(writer));
        let mut process = Process(command.spawn().unwrap());
        if outcome == "cancelled" {
            process.0.stdin.as_mut().unwrap().write_all(&vec![b' '; 256 * 1024]).unwrap();
            interrupt(&process);
        } else {
            let mut call = request();
            if outcome == "failed" {
                call["timeout_ms"] = json!(0);
            } else {
                call["input"]["node"] = json!("missing");
            }
            process.0.stdin.take().unwrap().write_all(call.to_string().as_bytes()).unwrap();
        }
        let status = wait(&mut process);
        let mut stdout = Vec::new();
        process.0.stdout.take().unwrap().read_to_end(&mut stdout).unwrap();
        if outcome == "failed" {
            events(&Output { status, stdout, stderr: Vec::new() }, "failed", 1);
        } else {
            assert_eq!(status.code(), Some(if outcome == "cancelled" { 130 } else { 1 }));
            assert!(stdout.is_empty());
        }
    }
}
