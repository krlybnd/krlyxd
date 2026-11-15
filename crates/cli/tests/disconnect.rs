//! A child process runs the production listener and real render/mesh queries. A gate
//! inside a probe/trim plugin synchronizes an in-flight call without timing guesses.
#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_cli::endpoint::Endpoint;
use krly3d_cli::peer::{Joined, Link, Peer};
use krly3d_cli::{dialect, json, listen};
use krly3d_core::{Attempt, CallControl, Id};
use krly3d_kernel::ports::probe::{Hit, Probe, ProbeRequest};
use krly3d_kernel::ports::trim::{Place, Trim, TrimRequest};
use krly3d_kernel::schema::Params;
use krly3d_kernel::{Context, KernelBuilder, Method, Plugin, Reply, Session};
use serde_json::{json, Value as Json};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct Gate {
    armed: Arc<AtomicBool>,
    progress: Arc<Mutex<UnixStream>>,
}

impl Gate {
    fn wait(&self, cx: &Context) {
        if !self.armed.swap(false, Ordering::SeqCst) {
            return;
        }
        let mut progress = self.progress.lock().unwrap();
        progress.write_all(b"E").unwrap();
        let until = Instant::now() + LIMIT;
        loop {
            if cx.check_control().is_err() {
                return;
            }
            assert!(Instant::now() < until, "gate was neither cancelled nor released");
            match progress.read(&mut [0_u8]) {
                Ok(1) => return,
                Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                }
                other => panic!("gate control link: {other:?}"),
            }
        }
    }
}

struct ProbeGate(Gate);

impl Plugin<Probe> for ProbeGate {
    fn id(&self) -> Id {
        Id::new("rays")
    }

    fn call(&self, cx: &Context, _: &ProbeRequest) -> Attempt<Vec<Vec<Hit>>> {
        self.0.wait(cx);
        // The real probe and raycaster handle the stop after this instrumentation.
        Attempt::declined("test gate; continue to the real probe")
    }
}

struct TrimGate(Gate);

impl Plugin<Trim> for TrimGate {
    fn id(&self) -> Id {
        Id::new("surface")
    }

    fn call(&self, cx: &Context, _: &TrimRequest) -> Attempt<Place> {
        self.0.wait(cx);
        Attempt::declined("test gate; continue to the real trim rule")
    }
}

/// A real Joined wrapper with a connected (unused) peer exercises main's peer path.
struct EmptyPeer;

impl Link for EmptyPeer {
    fn send(&mut self, _: &[u8]) -> io::Result<()> {
        Ok(())
    }

    fn receive(&mut self) -> io::Result<Vec<u8>> {
        Ok(br#"{"jsonrpc":"2.0","id":1,"result":{"ok":true,"text":"","data":[],"exact":"="}}"#.to_vec())
    }
}

struct Observed {
    joined: Joined<Session>,
    progress: Arc<Mutex<UnixStream>>,
}

impl Endpoint for Observed {
    fn name(&self) -> &str {
        self.joined.name()
    }

    fn instructions(&self) -> String {
        self.joined.instructions()
    }

    fn methods(&self) -> Vec<Method> {
        self.joined.methods()
    }

    fn call(&mut self, method: &str, args: &Params) -> Reply {
        self.joined.call(method, args)
    }

    fn call_with_control(&mut self, method: &str, args: &Params, control: Arc<CallControl>) -> Reply {
        let reply = self.joined.call_with_control(method, args, control);
        if matches!(method, "render" | "mesh" | "batch") {
            let mut progress = self.progress.lock().unwrap();
            serde_json::to_writer(&mut *progress, &json::reply_to_json(&reply).unwrap()).unwrap();
            progress.write_all(b"\n").unwrap();
        }
        reply
    }
}

#[test]
fn socket_server_fixture() {
    let Ok(socket) = std::env::var("KRLY_DISCONNECT_SOCKET") else { return };
    let method = std::env::var("KRLY_DISCONNECT_METHOD").unwrap();
    let dialect_name = std::env::var("KRLY_DISCONNECT_DIALECT").unwrap();
    let listener = listen::bind(std::path::Path::new(&socket)).unwrap();
    let progress = UnixStream::connect(std::env::var("KRLY_DISCONNECT_PROGRESS").unwrap()).unwrap();
    progress.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
    let progress = Arc::new(Mutex::new(progress));
    let armed = Arc::new(AtomicBool::new(false));
    let gate = Gate { armed: Arc::clone(&armed), progress: Arc::clone(&progress) };
    let mut builder = KernelBuilder::new();
    if method == "render" {
        builder.plugin::<Probe>(ProbeGate(gate));
    } else {
        builder.plugin::<Trim>(TrimGate(gate));
    }
    // Standard registration, with the instrumentation ahead of the real plugins.
    for register in [
        krly3d_std::sweep::register,
        krly3d_std::meet::register,
        krly3d_std::section::register,
        krly3d_std::content::register,
        krly3d_std::measure::register,
        krly3d_std::keys::register,
        krly3d_std::index::register,
        krly3d_std::query::register,
        krly3d_std::interpret::register,
        krly3d_std::tool::register,
        krly3d_std::trim::register,
        krly3d_std::probe::register,
        krly3d_std::stage::register,
        krly3d_std::read::register,
        krly3d_std::write::register,
        krly3d_view::register,
    ] {
        register(&mut builder);
    }
    let mut session = builder.session();
    let made = session.call("cone", &Params::new().with("id", "part").with("r", 5.0).with("h", 10.0));
    assert!(made.ok, "{}", made.text);
    assert!(session.call("mark", &Params::new()).ok);
    assert!(session.call("mark", &Params::new()).ok);
    let mut joined = Joined::new(session);
    joined.join(Peer::connect("remote", Box::new(EmptyPeer)).unwrap().shared()).unwrap();
    let mut endpoint = Observed { joined, progress };
    let mut dialect = dialect::named(&dialect_name).unwrap();
    armed.store(true, Ordering::SeqCst);
    listen::serve(&listener, dialect.as_mut(), &mut endpoint).unwrap();
}

struct Server {
    child: Child,
    socket: PathBuf,
    progress_path: PathBuf,
    progress: BufReader<UnixStream>,
    dialect: &'static str,
}

impl Server {
    fn start(method: &str, dialect: &'static str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = format!("k3d-disc-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let socket = std::env::temp_dir().join(format!("{base}.sock"));
        let progress_path = std::env::temp_dir().join(format!("{base}.progress"));
        let progress_listener = UnixListener::bind(&progress_path).unwrap();
        progress_listener.set_nonblocking(true).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "socket_server_fixture", "--nocapture"])
            .env("KRLY_DISCONNECT_SOCKET", &socket)
            .env("KRLY_DISCONNECT_PROGRESS", &progress_path)
            .env("KRLY_DISCONNECT_METHOD", method)
            .env("KRLY_DISCONNECT_DIALECT", dialect)
            .env("RAYON_NUM_THREADS", "2")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut server = Self {
            child,
            socket,
            progress_path,
            dialect,
            progress: BufReader::new(UnixStream::pair().unwrap().0),
        };
        let until = Instant::now() + LIMIT;
        loop {
            match progress_listener.accept() {
                Ok((progress, _)) => {
                    progress.set_read_timeout(Some(LIMIT)).unwrap();
                    server.progress = BufReader::new(progress);
                    return server;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < until, "fixture did not start");
                    assert!(server.child.try_wait().unwrap().is_none(), "fixture exited");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("progress connection: {error}"),
            }
        }
    }

    fn request(&self, method: &str, args: Json) -> Json {
        match self.dialect {
            "plain" => json!({"jsonrpc":"2.0","id":1,"method":method,"params":args}),
            _ => {
                json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":method,"arguments":args}})
            }
        }
    }

    fn connect(&self, request: Json) -> UnixStream {
        let mut stream = UnixStream::connect(&self.socket).unwrap();
        stream.set_read_timeout(Some(LIMIT)).unwrap();
        writeln!(stream, "{request}").unwrap();
        stream
    }

    fn entered(&mut self) {
        let mut entered = [0_u8];
        self.progress.read_exact(&mut entered).unwrap();
        assert_eq!(entered, *b"E", "the real operation did not reach its inner gate");
    }

    fn outcome(&mut self) -> Json {
        let mut line = String::new();
        self.progress.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    fn exchange(&self, method: &str, args: Json) -> Json {
        let client = self.connect(self.request(method, args));
        client.shutdown(Shutdown::Write).unwrap();
        read_reply(client)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_file(&self.progress_path);
    }
}

fn read_reply(client: UnixStream) -> Json {
    let mut line = String::new();
    BufReader::new(client).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

fn workload(method: &str, large: bool) -> Json {
    if method == "render" {
        let size = if large { 2048 } else { 32 };
        json!({"width":size,"height":size})
    } else {
        json!({"node":"part","tolerance":if large {0.00001} else {0.5}})
    }
}

fn disconnect(method: &str, dialect: &'static str, batch: bool) {
    let mut server = Server::start(method, dialect);
    let before = server.exchange("save", json!({}));
    let request = if batch {
        server.request(
            "batch",
            json!({"calls":[
                {"method":"rollback","args":{"mark":1}},
                {"method":"box","args":{"id":"temporary","w":1,"d":1,"h":1}},
                {"method":method,"args":workload(method, true)}
            ]}),
        )
    } else {
        server.request(method, workload(method, true))
    };
    let abandoned = server.connect(request);
    server.entered();
    let next = server.connect(server.request("save", json!({})));
    next.shutdown(Shutdown::Write).unwrap();

    let started = Instant::now();
    drop(abandoned);
    let outcome = server.outcome();
    assert_eq!(outcome["ok"], false, "{outcome}");
    if batch {
        assert_eq!(outcome["data"]["replies"][2]["data"]["error"], "cancelled");
    } else {
        assert_eq!(outcome["data"]["error"], "cancelled");
    }
    assert_eq!(read_reply(next), before, "cancelled work must preserve the document");
    assert!(started.elapsed() < LIMIT, "the next client waited for abandoned work");
    // A consumed mark and a model edit inside the cancelled batch must both recover.
    let rolled = server.exchange("rollback", json!({"mark":2}));
    assert!(rolled.get("error").is_none(), "{rolled}");
    assert_eq!(server.exchange("save", json!({})), before);
}

#[test]
fn plain_full_disconnect_during_real_render_releases_the_next_client() {
    disconnect("render", "plain", false);
}

#[test]
fn plain_full_disconnect_during_real_mesh_releases_the_next_client() {
    disconnect("mesh", "plain", false);
}

#[test]
fn mcp_full_disconnect_during_real_render_releases_the_next_client() {
    disconnect("render", "mcp", false);
}

#[test]
fn mcp_full_disconnect_during_real_mesh_releases_the_next_client() {
    disconnect("mesh", "mcp", false);
}

#[test]
fn cancelled_batch_restores_the_model_and_consumed_marks() {
    disconnect("render", "plain", true);
}

#[test]
fn write_half_close_keeps_in_flight_render_and_mesh_replies_in_both_dialects() {
    for method in ["render", "mesh"] {
        for dialect in ["plain", "mcp"] {
            let mut server = Server::start(method, dialect);
            let client = server.connect(server.request(method, workload(method, false)));
            client.shutdown(Shutdown::Write).unwrap();
            server.entered();
            server.progress.get_mut().write_all(b"R").unwrap();

            let outcome = server.outcome();
            assert_eq!(outcome["ok"], true, "{method}/{dialect}: {outcome}");
            let reply = read_reply(client);
            assert!(reply.get("error").is_none(), "{reply}");
            if dialect == "plain" {
                assert_eq!(reply["result"], outcome);
            } else {
                assert_eq!(reply["result"]["isError"], false);
            }
        }
    }
}
