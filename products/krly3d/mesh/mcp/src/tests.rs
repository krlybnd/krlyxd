#![allow(clippy::unwrap_used, clippy::panic)]

use super::{events_result, serve, Server, MAX_MESSAGE_BYTES};
use krly3d_core::{Accuracy, Attempt, Id};
use krly3d_kernel::model::Statement;
use krly3d_kernel::ports::interpret::{MeshRequest, ToMesh};
use krly3d_kernel::schema::Params;
use krly3d_kernel::{Context, KernelBuilder, Plugin};
use krly3d_mesh::operation::{encode_event, MeshEvent, MeshEventBody};
use krly3d_mesh::{
    diagnostic, ArtifactId, Exactness, MeshArtifact, MeshData, MeshInput, MeshResult, Provenance, Quality,
    RequestId,
};
use krly3d_mesh_engine::runner::{MeshRunner, RunnerLimits};
use krly3d_mesh_engine::MeshEngine;
use krlyxd::{FakeClock, OperationId};
use serde_json::{json, Value};
use std::io::{self, Cursor, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const WATCHDOG: Duration = Duration::from_secs(5);
const HELLO: &str = r#"{"jsonrpc":"2.0","id":"hello","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;
const READY: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

fn runner() -> MeshRunner {
    MeshRunner::new(RunnerLimits::default(), Arc::new(FakeClock::default())).unwrap()
}

fn server() -> Server {
    Server::new(MeshEngine::from_k3d("k3d 1\nlayer geo\npart = box d=4 h=2 w=6\n").unwrap(), runner())
}

fn ready(server: &mut Server) {
    assert!(server.handle(HELLO).unwrap().get("result").is_some());
    assert!(server.handle(READY).is_none());
}

fn rpc(server: &mut Server, method: &str, params: Value) -> Value {
    server.handle(&json!({"jsonrpc":"2.0","id":17,"method":method,"params":params}).to_string()).unwrap()
}

fn call(server: &mut Server, name: &str, arguments: Value) -> Value {
    rpc(server, "tools/call", json!({"name":name,"arguments":arguments}))["result"].clone()
}

fn arguments(request: &str) -> Value {
    json!({"request_id":request,"protocol":{"major":2,"minor":0},"api":{"major":2,"minor":0},"input":{"node":"part"}})
}

fn payload(result: &Value) -> Value {
    assert_eq!(result["content"].as_array().unwrap().len(), 1);
    assert_eq!(result["content"][0]["type"], "text");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn start(server: &mut Server, request: &str) -> String {
    let result = call(server, "mesh", arguments(request));
    assert_eq!(result["isError"], false, "{result}");
    payload(&result)["operation_id"].as_str().unwrap().to_owned()
}

#[test]
fn lifecycle_negotiation_capabilities_and_descriptor_schema() {
    let mut server = server();
    assert_eq!(rpc(&mut server, "ping", json!({}))["result"], json!({}));
    assert_eq!(rpc(&mut server, "tools/list", json!({}))["error"]["code"], -32000);
    let response = server.handle(&HELLO.replace("2025-11-25", "2099-01-01")).unwrap();
    assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(response["result"]["capabilities"], json!({"tools":{"listChanged":false}}));
    assert!(response["result"]["capabilities"].get("tasks").is_none());
    assert_eq!(rpc(&mut server, "tools/list", json!({}))["error"]["code"], -32000);
    assert!(server.handle(READY).is_none());
    let listed = rpc(&mut server, "tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 4);
    assert_eq!(tools[0]["inputSchema"], krly3d_mesh::operation::descriptor().call_schema);
    assert!(tools.iter().all(|tool| tool["execution"]["taskSupport"] == "forbidden"));
    assert_eq!(rpc(&mut server, "tasks/list", json!({}))["error"]["code"], -32601);
    assert_eq!(rpc(&mut server, "tools/list", json!({"cursor":"bad"}))["error"]["code"], -32602);
    assert_eq!(server.handle(HELLO).unwrap()["error"]["code"], -32600);
}

#[test]
fn invalid_initialize_does_not_advance_phase() {
    let mut server = server();
    for params in [
        json!({}),
        json!({"protocolVersion":5}),
        json!({"protocolVersion":"2025-11-25","capabilities":[],"clientInfo":{"name":"x","version":"0"}}),
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":1,"version":"0"}}),
    ] {
        assert_eq!(rpc(&mut server, "initialize", params)["error"]["code"], -32602);
    }
    ready(&mut server);
}

#[test]
fn rejects_malformed_rpc_batches_ids_and_duplicate_keys_without_side_effects() {
    let mut server = server();
    ready(&mut server);
    for message in [
        "null",
        "[]",
        "[{}]",
        r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":1.5,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":true,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":1,"id":2,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"ping","m\u0065thod":"tools/call"}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"_meta":{"a":1,"a":2}}}"#,
    ] {
        assert_eq!(server.handle(message).unwrap()["error"]["code"], -32600, "{message}");
    }
    assert_eq!(server.handle("{").unwrap()["error"]["code"], -32700);
    let response = server.handle(r#"{"jsonrpc":"2.0","id":18446744073709551615,"method":"ping"}"#).unwrap();
    assert_eq!(response["id"].as_u64(), Some(u64::MAX));
    for message in [
        r#"{"jsonrpc":"2.0","method":"ping"}"#,
        r#"{"jsonrpc":"2.0","method":"anything"}"#,
        r#"{"jsonrpc":"2.0","id":3,"result":{}}"#,
    ] {
        assert!(server.handle(message).is_none());
    }
    let silent = json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"mesh","arguments":arguments("silent")}});
    assert!(server.handle(&silent.to_string()).is_none());
    // A request with that domain ID still succeeds: the notification ran no tool.
    start(&mut server, "silent");
}

#[test]
fn protocol_errors_and_domain_rejections_have_distinct_encodings() {
    let mut server = server();
    ready(&mut server);
    for params in [
        json!({}),
        json!({"name":3}),
        json!({"name":"unknown"}),
        json!({"name":"mesh","arguments":[]}),
        json!({"name":"mesh","arguments":null}),
        json!({"name":"mesh","task":{},"arguments":arguments("task")}),
        json!({"name":"mesh","_meta":null}),
    ] {
        assert_eq!(rpc(&mut server, "tools/call", params)["error"]["code"], -32602);
    }
    let mut bad = arguments("bad");
    bad["api"]["major"] = json!(1);
    let result = call(&mut server, "mesh", bad);
    assert_eq!(result["isError"], true);
    assert_eq!(payload(&result)["diagnostic"]["code"], "unsupported_version");
    for bad in [json!({}), json!({"operation_id":""}), json!({"operation_id":"x","other":true})] {
        let result = call(&mut server, "operation_events", bad);
        assert_eq!(result["isError"], true);
        assert_eq!(payload(&result)["diagnostic"]["code"], "input");
    }
    let result = call(&mut server, "operation_events", json!({"operation_id":"missing"}));
    assert_eq!(payload(&result)["diagnostic"]["code"], "not_found");
    for input in [
        json!({"node":"missing"}),
        json!({"node":"part","triangles":1}),
        json!({"node":"part","tolerance":{"value":0,"unit":"model"}}),
        json!({"node":"part","unexpected":true}),
    ] {
        let mut bad = arguments("reused-after-rejection");
        bad["input"] = input;
        assert_eq!(call(&mut server, "mesh", bad)["isError"], true);
    }
    start(&mut server, "reused-after-rejection");
}

#[test]
fn canonical_event_payload_keeps_every_field_float_token_and_error() {
    let provenance = Provenance {
        package_version: "0.1.0".into(),
        engine_revision: Id::new("engine"),
        source: Id::new("source"),
        input: MeshInput::new("part@1"),
    };
    let special = f64::from_bits(0x3fd5_5555_5555_5555);
    let result = MeshResult {
        points: 1,
        triangles: 1,
        volume: special,
        area: 88.0,
        open: 0,
        quality: Quality {
            exactness: Exactness::Sampled { samples: 1 },
            positional_uncertainty: None,
            confidence: None,
        },
        provenance: provenance.clone(),
        mesh: Some(MeshArtifact {
            id: ArtifactId::new("artifact"),
            revision: 1,
            data: MeshData { points: vec![[special, f64::MIN_POSITIVE, -0.0]], triangles: vec![[0, 0, 0]] },
        }),
    };
    let event = |seq, body| MeshEvent {
        request_id: RequestId::new("req"),
        operation_id: OperationId::new("op"),
        seq,
        body,
    };
    for terminal in [
        MeshEventBody::Completed(result),
        MeshEventBody::Failed(diagnostic("geometry", "bad \"shape\"\nfull diagnostic")),
        MeshEventBody::Cancelled,
    ] {
        let events = [event(0, MeshEventBody::Started(provenance.clone())), event(1, terminal)];
        let expected = format!(
            r#"{{"events":[{},{}]}}"#,
            encode_event(&events[0]).unwrap(),
            encode_event(&events[1]).unwrap()
        );
        let response = events_result(&events).unwrap();
        // Compare the actual text after an outer wire roundtrip, never a Value
        // roundtrip of the canonical event that might round mesh coordinates.
        let wire = serde_json::to_string(&response).unwrap();
        let response: Value = serde_json::from_str(&wire).unwrap();
        assert_eq!(response["content"][0]["text"], expected);
        assert_eq!(response["isError"], !matches!(events[1].body, MeshEventBody::Completed(_)));
    }
    assert_eq!(payload(&events_result(&[]).unwrap()), json!({"events":[]}));
}

struct Gate {
    entered: Sender<()>,
    stopped: Sender<()>,
    open: Mutex<bool>,
    changed: Condvar,
}

impl Plugin<ToMesh> for Gate {
    fn id(&self) -> Id {
        Id::new("brep")
    }

    fn call(&self, cx: &Context, request: &MeshRequest) -> Attempt<Arc<krly3d_kernel::mesh::Mesh>> {
        self.entered.send(()).unwrap();
        let until = Instant::now() + WATCHDOG;
        let mut open = self.open.lock().unwrap();
        while !*open {
            if let Err(error) = cx.check_control() {
                self.stopped.send(()).unwrap();
                return Attempt::Failed(error);
            }
            assert!(Instant::now() < until, "gate watchdog expired");
            open = self.changed.wait_timeout(open, Duration::from_millis(2)).unwrap().0;
        }
        drop(open);
        krly3d_std::interpret::mesh::BrepMesh.call(cx, request)
    }
}

struct Fixture {
    server: Option<Server>,
    gate: Arc<Gate>,
    entered: Receiver<()>,
    stopped: Receiver<()>,
}

// Arc is wrapped explicitly because Plugin is not blanket-implemented for Arc<T>.
struct Gated(Arc<Gate>);
impl Plugin<ToMesh> for Gated {
    fn id(&self) -> Id {
        self.0.id()
    }
    fn call(&self, cx: &Context, request: &MeshRequest) -> Attempt<Arc<krly3d_kernel::mesh::Mesh>> {
        self.0.call(cx, request)
    }
}

impl Fixture {
    fn new() -> Self {
        let (entered, entering) = mpsc::channel();
        let (stopped, stopping) = mpsc::channel();
        let gate = Arc::new(Gate { entered, stopped, open: Mutex::new(false), changed: Condvar::new() });
        let mut builder = KernelBuilder::new();
        builder.accuracy(Accuracy::for_scale(100.0).unwrap());
        builder.plugin::<ToMesh>(Gated(Arc::clone(&gate)));
        krly3d_std::sweep::register(&mut builder);
        krly3d_std::tool::register(&mut builder);
        krly3d_std::stage::register(&mut builder);
        let mut kernel = builder.build();
        kernel
            .apply(
                Statement::new(
                    "part",
                    "box",
                    Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0),
                    &[],
                )
                .unwrap(),
            )
            .unwrap();
        let server = Server::new(MeshEngine::new(kernel).unwrap(), runner());
        Self { server: Some(server), gate, entered: entering, stopped: stopping }
    }
    fn server(&mut self) -> &mut Server {
        self.server.as_mut().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // First drop the runner so its cancellation wins over opening the gate.
        self.server.take();
        *self.gate.open.lock().unwrap() = true;
        self.gate.changed.notify_all();
    }
}

#[test]
fn blocked_real_mesh_does_not_block_start_empty_drain_ping_cancel_or_release() {
    let mut fixture = Fixture::new();
    ready(fixture.server());
    let before = Instant::now();
    let id = start(fixture.server(), "held");
    assert!(before.elapsed() < Duration::from_secs(1));
    fixture.entered.recv_timeout(WATCHDOG).unwrap();
    let args = json!({"operation_id":id});
    let first = call(fixture.server(), "operation_events", args.clone());
    assert_eq!(payload(&first)["events"][0]["body"]["kind"], "started");
    let before = Instant::now();
    let empty = call(fixture.server(), "operation_events", args.clone());
    assert_eq!(payload(&empty), json!({"events":[]}));
    assert!(before.elapsed() < Duration::from_secs(1));
    assert_eq!(rpc(fixture.server(), "ping", json!({}))["result"], json!({}));
    assert_eq!(call(fixture.server(), "operation_release", args.clone())["isError"], true);
    // MCP cancellation of the already answered start request is independent.
    fixture
        .server()
        .handle(r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":17}}"#);
    assert_eq!(payload(&call(fixture.server(), "operation_events", args.clone())), json!({"events":[]}));
    for _ in 0..2 {
        assert_eq!(call(fixture.server(), "operation_cancel", args.clone())["isError"], false);
    }
    let terminal = call(fixture.server(), "operation_events", args.clone());
    assert_eq!(terminal["isError"], true);
    assert_eq!(payload(&terminal)["events"][0]["body"]["kind"], "cancelled");
    fixture.stopped.recv_timeout(WATCHDOG).unwrap();
    let until = Instant::now() + WATCHDOG;
    loop {
        let release = call(fixture.server(), "operation_release", args.clone());
        if release["isError"] == false {
            assert_eq!(payload(&release)["released"], true);
            break;
        }
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    assert_eq!(payload(&call(fixture.server(), "operation_events", args))["diagnostic"]["code"], "not_found");
    start(fixture.server(), "held"); // explicit release frees the domain request ID
}

struct BrokenOutput;
impl Write for BrokenOutput {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn eof_and_broken_output_drop_runner_and_stop_owned_real_mesh_work() {
    for broken in [false, true] {
        let mut fixture = Fixture::new();
        ready(fixture.server());
        start(fixture.server(), "disconnect");
        fixture.entered.recv_timeout(WATCHDOG).unwrap();
        let server = fixture.server.take().unwrap();
        if broken {
            let input = Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":91,\"method\":\"ping\"}\n");
            assert_eq!(serve(server, input, BrokenOutput).unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        } else {
            serve(server, Cursor::new([]), Vec::new()).unwrap();
        }
        fixture.stopped.recv_timeout(WATCHDOG).unwrap();
    }
}

#[test]
fn bounded_input_utf8_framing_and_flush_errors() {
    let output = Capture::default();
    serve(
        server(),
        Cursor::new(b"\xff\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n"),
        output.clone(),
    )
    .unwrap();
    let messages: Vec<Value> = String::from_utf8(output.0.lock().unwrap().clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(messages[0]["error"]["code"], -32700);
    assert_eq!(messages[1]["result"], json!({}));
    for bytes in [vec![b' '; MAX_MESSAGE_BYTES + 1], b"{}".to_vec()] {
        assert_eq!(
            serve(server(), Cursor::new(bytes), Vec::new()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    let mut bytes = vec![b' '; MAX_MESSAGE_BYTES];
    bytes.push(b'\n');
    serve(server(), Cursor::new(bytes), Vec::new()).unwrap();
}

struct BlockedOutput {
    entered: Option<Sender<()>>,
    release: Receiver<()>,
}

impl Write for BlockedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some(entered) = self.entered.take() {
            entered.send(()).unwrap();
            // Deliberately simulate an indefinitely blocked pipe syscall. The
            // coordinator must cancel work and return without joining us.
            self.release.recv_timeout(WATCHDOG).unwrap();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct DelayedEof {
    prefix: Cursor<Vec<u8>>,
    eof: Receiver<()>,
}

impl io::Read for DelayedEof {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let available = io::BufRead::fill_buf(self)?;
        let count = available.len().min(output.len());
        output[..count].copy_from_slice(&available[..count]);
        io::BufRead::consume(self, count);
        Ok(count)
    }
}

impl io::BufRead for DelayedEof {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.prefix.position() as usize >= self.prefix.get_ref().len() {
            self.eof.recv_timeout(WATCHDOG).unwrap();
        }
        io::BufRead::fill_buf(&mut self.prefix)
    }
    fn consume(&mut self, count: usize) {
        io::BufRead::consume(&mut self.prefix, count);
    }
}

#[test]
fn eof_cancels_owned_work_while_output_syscall_is_blocked() {
    let mut fixture = Fixture::new();
    ready(fixture.server());
    start(fixture.server(), "blocked-output");
    fixture.entered.recv_timeout(WATCHDOG).unwrap();
    let (entered, writing) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let (eof, closed) = mpsc::channel();
    let input = DelayedEof {
        prefix: Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":99,\"method\":\"ping\"}\n".to_vec()),
        eof: closed,
    };
    let output = BlockedOutput { entered: Some(entered), release: wait };
    let server = fixture.server.take().unwrap();
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || done.send(serve(server, input, output)).unwrap());
    writing.recv_timeout(WATCHDOG).unwrap();
    eof.send(()).unwrap();
    fixture.stopped.recv_timeout(WATCHDOG).unwrap();
    assert_eq!(finished.recv_timeout(WATCHDOG).unwrap().unwrap_err().kind(), io::ErrorKind::TimedOut);
    release.send(()).unwrap();
}

#[test]
fn duplicate_mesh_fields_and_large_integer_tokens_are_rejected_before_acceptance() {
    let mut server = server();
    ready(&mut server);
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"mesh","arguments":arguments("raw")}}).to_string();
    let duplicate = request.replace(r#""node":"part""#, r#""node":"part","node":"missing""#);
    assert_eq!(server.handle(&duplicate).unwrap()["error"]["code"], -32600);
    let huge_timeout =
        request.replace(r#""request_id":"raw""#, r#""timeout_ms":9007199254740993,"request_id":"raw""#);
    let rejected = server.handle(&huge_timeout).unwrap();
    assert_eq!(rejected["result"]["isError"], true);
    start(&mut server, "raw");
}
