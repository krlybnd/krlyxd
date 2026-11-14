//! Another server reached over JSON-RPC, and a server joined from several.
//!
//! A peer is a program that speaks the plain dialect on its standard input and output,
//! such as `krlyvis`. It is started once and its methods are read once. [`Joined`] serves
//! the methods of one endpoint and of its peers side by side: `vis.analyse` goes to the
//! peer named `vis` as `analyse`, everything else to the endpoint itself. An agent working
//! with krly3d can thus ask krlyvis about a photograph in the same connection.

use crate::endpoint::Endpoint;
use crate::json;
use krly3d_core::Exactness;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::peer::{Peer as PeerFamily, PeerCall};
use krly3d_kernel::schema::{Kind, Param, Params, Schema, Value};
use krly3d_kernel::{describe, Method, MethodKind, Reply};
use krly3d_kernel::{Context, Plugin};
use serde_json::{json, Value as Json};
use std::io::{self, BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

/// Lines to and from the server behind a link, one message per line.
pub trait Link: Send {
    fn send(&mut self, message: &[u8]) -> io::Result<()>;

    fn receive(&mut self) -> io::Result<Vec<u8>>;
}

/// A peer program run as a child process, spoken to on its standard input and output.
/// Its standard error is this program's, so that what it complains of is seen.
pub struct Child {
    process: std::process::Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Child {
    /// Starts `program` without arguments, so that it serves its default dialect.
    pub fn spawn(program: &str) -> io::Result<Self> {
        let mut process = Command::new(program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = process.stdin.take();
        let output = process.stdout.take().ok_or_else(|| io::Error::other("the peer has no output"))?;
        Ok(Self { process, input, output: BufReader::new(output) })
    }
}

impl Link for Child {
    fn send(&mut self, message: &[u8]) -> io::Result<()> {
        let input = self.input.as_mut().ok_or_else(|| io::Error::other("the peer's input is closed"))?;
        input.write_all(message)?;
        input.write_all(b"\n")?;
        input.flush()
    }

    fn receive(&mut self) -> io::Result<Vec<u8>> {
        let mut line = Vec::new();
        if self.output.read_until(b'\n', &mut line)? == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the peer closed its output"));
        }
        Ok(line)
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        // Closing its input ends the peer's loop; then it is waited for, so that it does
        // not outlive this program as a zombie. A drop has no one to report a failure to.
        self.input.take();
        let _ = self.process.wait();
    }
}

/// A server behind a link, known by the name its methods are prefixed with.
pub struct Peer {
    name: String,
    link: Box<dyn Link>,
    methods: Vec<Method>,
    last_id: u64,
}

impl Peer {
    /// Asks the server behind `link` for its methods.
    pub fn connect(name: &str, link: Box<dyn Link>) -> Result<Self, String> {
        if name.is_empty() || name.contains('.') {
            return Err(format!("`{name}` cannot name a peer: use a word without dots"));
        }
        let mut peer = Self { name: name.to_string(), link, methods: Vec::new(), last_id: 0 };
        let listed =
            peer.ask("methods", &Params::new()).map_err(|reason| format!("peer `{name}`: {reason}"))?;
        let Value::List(entries) = &listed.data else {
            return Err(format!("peer `{name}` listed its methods as {:?}", listed.data));
        };
        peer.methods = entries
            .iter()
            .map(method_of)
            .collect::<Result<_, String>>()
            .map_err(|reason| format!("peer `{name}`: {reason}"))?;
        Ok(peer)
    }

    /// Starts `program` and connects to it.
    pub fn spawn(name: &str, program: &str) -> Result<Self, String> {
        let child = Child::spawn(program).map_err(|error| format!("peer `{name}`: {program}: {error}"))?;
        Self::connect(name, Box::new(child))
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// This peer, to be used by the joined server and by the kernel alike.
    pub fn shared(self) -> SharedPeer {
        Arc::new(Mutex::new(self))
    }

    pub fn methods(&self) -> &[Method] {
        &self.methods
    }

    /// Calls a method of the peer; a broken link comes back as a failed reply.
    pub fn call(&mut self, method: &str, args: &Params) -> Reply {
        self.ask(method, args).unwrap_or_else(|reason| Reply {
            ok: false,
            text: format!("peer `{}`: {reason}", self.name),
            data: Value::map([("error", Value::from("peer"))]),
            exact: Exactness::Exact,
        })
    }

    fn ask(&mut self, method: &str, args: &Params) -> Result<Reply, String> {
        self.last_id += 1;
        let params = json::to_json(&args.clone().into_value()).map_err(|misfit| misfit.to_string())?;
        let message = json!({ "jsonrpc": "2.0", "id": self.last_id, "method": method, "params": params });
        let line = serde_json::to_vec(&message).map_err(|error| error.to_string())?;
        let failed = |error: io::Error| format!("the connection failed: {error}");
        self.link.send(&line).map_err(failed)?;
        let answer = loop {
            let line = self.link.receive().map_err(failed)?;
            let message: Json =
                serde_json::from_slice(&line).map_err(|error| format!("the answer is not JSON: {error}"))?;
            if message.get("method").is_none() {
                break message;
            }
            self.refuse(&message).map_err(failed)?;
        };
        if answer.get("id") != Some(&Json::from(self.last_id)) {
            return Err(format!("the answer {answer} is not to request {}", self.last_id));
        }
        if let Some(result) = answer.get("result") {
            return reply_of(result);
        }
        let error = answer.get("error").ok_or("the answer has neither a result nor an error")?;
        // The plain dialect sends a failed call's whole reply as the error's data.
        match error.get("data").map(reply_of) {
            Some(Ok(reply)) => Ok(reply),
            _ => Err(error.get("message").and_then(Json::as_str).unwrap_or("the call failed").to_string()),
        }
    }
}

impl Peer {
    /// Answers a request the peer sent while it owed an answer; a notification, which
    /// wants none, is let go.
    fn refuse(&mut self, request: &Json) -> io::Result<()> {
        let Some(id) = request.get("id") else { return Ok(()) };
        let reason = format!(
            "krly3d is waiting for your answer to its request {} and takes no request until then",
            self.last_id
        );
        let refusal = json!({ "jsonrpc": "2.0", "id": id, "error": { "code": BUSY, "message": reason } });
        self.link.send(&serde_json::to_vec(&refusal).map_err(io::Error::other)?)
    }
}

/// The JSON-RPC error code of a request refused because the server is waiting on the asker.
const BUSY: i64 = -32000;

/// A peer used by the joined server and by the kernel, one question at a time.
pub type SharedPeer = Arc<Mutex<Peer>>;

fn lock(peer: &SharedPeer) -> std::sync::MutexGuard<'_, Peer> {
    // A panic while the peer was held leaves at worst a link out of step, which the next
    // answer's id check reports; the peer itself is still usable.
    peer.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The kernel's way to the peer: a plugin of its `peer` family under the peer's name.
pub struct PeerAdapter {
    name: String,
    peer: SharedPeer,
}

impl PeerAdapter {
    pub fn new(peer: &SharedPeer) -> Self {
        Self { name: lock(peer).name().to_string(), peer: Arc::clone(peer) }
    }
}

impl Plugin<PeerFamily> for PeerAdapter {
    fn id(&self) -> Id {
        Id::new(&self.name)
    }

    fn call(&self, _: &Context, request: &PeerCall) -> Attempt<Reply> {
        Attempt::Done(lock(&self.peer).call(&request.method, &request.params))
    }
}

/// A reply sent as the object `{ok, text, data, exact}`. How exact a numerical value is
/// does not travel, only that it is numerical; its tolerance reads as zero.
fn reply_of(json: &Json) -> Result<Reply, String> {
    let ok = json.get("ok").and_then(Json::as_bool).ok_or("a reply without `ok`")?;
    let text = json.get("text").and_then(Json::as_str).ok_or("a reply without `text`")?;
    let data = match json.get("data") {
        Some(data) => json::from_json(data).map_err(|misfit| misfit.to_string())?,
        None => Value::map([]),
    };
    let exact = match json.get("exact").and_then(Json::as_str) {
        Some("~") => Exactness::Numeric { tolerance: 0.0 },
        Some("?") => Exactness::Sampled { samples: 0 },
        _ => Exactness::Exact,
    };
    Ok(Reply { ok, text: text.to_string(), data, exact })
}

/// A method as `methods` lists it (see [`describe`]).
fn method_of(value: &Value) -> Result<Method, String> {
    let entries = map_of(value, "a method")?;
    let text = |key: &str| text_of(entries.get(key), key);
    let kind = match text("kind")? {
        "tool" => MethodKind::Tool,
        "query" => MethodKind::Query,
        "session" => MethodKind::Session,
        other => return Err(format!("`{other}` is not a kind of method")),
    };
    let inputs = match entries.get("inputs") {
        Some(Value::List(pair)) => match pair[..] {
            [Value::Number(least), Value::Number(most)] => (least as usize, most as usize),
            _ => return Err("`inputs` is not two numbers".to_string()),
        },
        _ => (0, 0),
    };
    let params = match entries.get("params") {
        Some(Value::List(params)) => params.iter().map(param_of).collect::<Result<_, _>>()?,
        _ => Vec::new(),
    };
    let schema = Schema { about: lasting(text("about")?), inputs, params };
    Ok(Method { name: text("name")?.to_string(), kind, schema })
}

fn param_of(value: &Value) -> Result<Param, String> {
    let entries = map_of(value, "a parameter")?;
    let named = text_of(entries.get("kind"), "kind")?;
    let kind = [
        Kind::Number,
        Kind::Count,
        Kind::Text,
        Kind::Triple,
        Kind::Numbers,
        Kind::Texts,
        Kind::Flag,
        Kind::Any,
    ]
    .into_iter()
    .find(|kind| kind.name() == named)
    .ok_or_else(|| format!("`{named}` is not a kind of parameter"))?;
    Ok(Param {
        name: lasting(text_of(entries.get("name"), "name")?),
        kind,
        required: entries.get("required") == Some(&Value::Bool(true)),
        default: entries.get("default").cloned(),
        about: lasting(text_of(entries.get("about"), "about")?),
    })
}

fn map_of<'a>(value: &'a Value, what: &str) -> Result<&'a std::collections::BTreeMap<String, Value>, String> {
    match value {
        Value::Map(entries) => Ok(entries),
        other => Err(format!("{what} is listed as {other:?}")),
    }
}

fn text_of<'a>(value: Option<&'a Value>, key: &str) -> Result<&'a str, String> {
    match value {
        Some(Value::Text(text)) => Ok(text),
        _ => Err(format!("`{key}` is missing or not a text")),
    }
}

/// A schema names its parameters with texts that last as long as the program, as the
/// kernel's own are written in its code. A peer's are read once, when it connects, so the
/// memory kept this way is bounded by the size of its method list.
fn lasting(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}

/// An endpoint and its peers, served as one.
pub struct Joined<E> {
    local: E,
    peers: Vec<SharedPeer>,
}

impl<E: Endpoint> Joined<E> {
    pub fn new(local: E) -> Self {
        Self { local, peers: Vec::new() }
    }

    /// Adds a peer, unless its name is already a prefix of the others' methods.
    pub fn join(&mut self, peer: SharedPeer) -> Result<(), String> {
        let name = lock(&peer).name().to_string();
        let taken = self.peers.iter().any(|other| lock(other).name() == name)
            || self.local.methods().iter().any(|method| method.name.starts_with(&format!("{name}.")));
        if taken {
            return Err(format!("the prefix `{name}.` is already taken"));
        }
        self.peers.push(peer);
        Ok(())
    }
}

impl<E: Endpoint> Endpoint for Joined<E> {
    fn name(&self) -> &str {
        self.local.name()
    }

    fn instructions(&self) -> String {
        let mut text = self.local.instructions();
        for peer in &self.peers {
            text.push_str(&format!(
                " Methods named `{0}.…` are answered by `{0}`, another server reached over JSON-RPC; \
                 `{0}.methods` lists what it offers.",
                lock(peer).name()
            ));
        }
        text
    }

    fn methods(&self) -> Vec<Method> {
        let mut methods = self.local.methods();
        for peer in &self.peers {
            let peer = lock(peer);
            methods.extend(
                peer.methods().iter().map(|method| Method {
                    name: format!("{}.{}", peer.name(), method.name),
                    ..method.clone()
                }),
            );
        }
        methods
    }

    fn call(&mut self, method: &str, args: &Params) -> Reply {
        if method == "methods" {
            return describe(&self.methods());
        }
        if let Some((prefix, rest)) = method.split_once('.') {
            if let Some(peer) = self.peers.iter().find(|peer| lock(peer).name() == prefix) {
                return lock(peer).call(rest, args);
            }
        }
        self.local.call(method, args)
    }
}

#[cfg(test)]
mod tests {
    use super::{Joined, Link, Peer, PeerAdapter, SharedPeer};
    use crate::dialect::plain::Plain;
    use crate::dialect::testing::session;
    use crate::dialect::Dialect;
    use crate::endpoint::Endpoint;
    use crate::rpc;
    use krly3d_kernel::ports::peer::{ask, Peer as PeerFamily};
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::{KernelBuilder, Session};
    use serde_json::{json, Value as Json};
    use std::collections::VecDeque;
    use std::io;
    use std::sync::{Arc, Mutex};

    /// A krly3d session behind the plain dialect, in this process: a peer without a
    /// child process. With `asks_first`, it sends a request of its own before each answer,
    /// as a peer that calls back would, and keeps what it was told in return.
    struct Loopback {
        session: Session,
        waiting: VecDeque<Vec<u8>>,
        asks_first: bool,
        told: Arc<Mutex<Vec<Json>>>,
    }

    impl Link for Loopback {
        fn send(&mut self, message: &[u8]) -> io::Result<()> {
            let json: Json = serde_json::from_slice(message).map_err(io::Error::other)?;
            if json.get("method").is_none() {
                self.told.lock().unwrap().push(json);
                return Ok(());
            }
            if self.asks_first {
                let callback = json!({ "jsonrpc": "2.0", "id": "back", "method": "geo.box", "params": {} });
                self.waiting.push_back(serde_json::to_vec(&callback).map_err(io::Error::other)?);
            }
            let session = &mut self.session;
            let answer = rpc::answer(message, &mut |method, params| Plain.answer(session, method, params));
            self.waiting.push_back(serde_json::to_vec(&answer).map_err(io::Error::other)?);
            Ok(())
        }

        fn receive(&mut self) -> io::Result<Vec<u8>> {
            self.waiting.pop_front().ok_or_else(|| io::Error::other("nothing was sent"))
        }
    }

    /// A link whose other end has gone.
    struct Broken;

    impl Link for Broken {
        fn send(&mut self, _: &[u8]) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone"))
        }

        fn receive(&mut self) -> io::Result<Vec<u8>> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone"))
        }
    }

    fn loopback(asks_first: bool) -> (Loopback, Arc<Mutex<Vec<Json>>>) {
        let told = Arc::new(Mutex::new(Vec::new()));
        (Loopback { session: session(), waiting: VecDeque::new(), asks_first, told: Arc::clone(&told) }, told)
    }

    fn peer() -> SharedPeer {
        Peer::connect("other", Box::new(loopback(false).0)).unwrap().shared()
    }

    fn plate() -> Params {
        Params::new().with("id", "plate").with("w", 60.0).with("d", 40.0).with("h", 10.0)
    }

    #[test]
    fn a_peer_lists_the_methods_of_the_server_behind_it_with_their_schemas() {
        let peer = peer();

        let peer = peer.lock().unwrap();
        let listed = peer.methods().iter().find(|method| method.name == "box").unwrap();

        assert_eq!(listed, session().methods().iter().find(|method| method.name == "box").unwrap());
    }

    #[test]
    fn a_joined_server_sends_a_prefixed_method_to_its_peer_and_the_rest_to_itself() {
        let mut joined = Joined::new(session());
        joined.join(peer()).unwrap();

        let remote = joined.call("other.box", &plate());
        let local = joined.call("save", &Params::new());
        let remote_save = joined.call("other.save", &Params::new());

        assert!(remote.ok, "{}", remote.text);
        assert_eq!(local.text, "k3d 1\n", "the local model stays empty");
        assert_eq!(remote_save.text, "k3d 1\nlayer geo\nplate = box d=40 h=10 w=60\n");
    }

    #[test]
    fn methods_lists_the_peers_methods_under_their_prefix() {
        let mut joined = Joined::new(session());
        joined.join(peer()).unwrap();

        let reply = joined.call("methods", &Params::new());

        assert!(reply.text.lines().any(|line| line.starts_with("box (tool)")));
        assert!(reply.text.lines().any(|line| line.starts_with("other.box (tool)")));
        assert!(joined.instructions().contains("`other.methods`"));
    }

    #[test]
    fn a_failed_call_of_the_peer_keeps_its_reason_and_kind() {
        let mut joined = Joined::new(session());
        joined.join(peer()).unwrap();

        let reply = joined.call("other.box", &Params::new().with("w", "wide"));

        assert!(!reply.ok);
        assert!(reply.text.contains("`w` must be a number"), "{}", reply.text);
        assert_eq!(reply.data, krly3d_kernel::schema::Value::map([("error", "input".into())]));
    }

    #[test]
    fn a_peer_that_cannot_be_reached_is_refused_when_it_connects() {
        let refused = Peer::connect("gone", Box::new(Broken));

        assert!(refused.is_err_and(|reason| reason.contains("the connection failed")));
    }

    #[test]
    fn two_peers_cannot_share_a_name() {
        let mut joined = Joined::new(session());
        joined.join(peer()).unwrap();

        assert!(joined.join(peer()).is_err());
        assert!(Peer::connect("a.b", Box::new(Broken)).is_err());
    }

    #[test]
    fn a_request_from_the_peer_while_it_owes_an_answer_is_refused_and_the_answer_still_comes() {
        let (link, told) = loopback(true);
        let mut peer = Peer::connect("other", Box::new(link)).unwrap();

        let reply = peer.call("box", &plate());

        assert!(reply.ok, "{}", reply.text);
        let told = told.lock().unwrap();
        let refusal = told.iter().find(|message| message["id"] == "back").unwrap();
        assert_eq!(refusal["error"]["code"], -32000);
        assert!(refusal["error"]["message"].as_str().unwrap().contains("waiting for your answer"));
    }

    #[test]
    fn the_kernel_asks_a_joined_peer_through_its_adapter() {
        let peer = peer();
        let mut builder = KernelBuilder::new();
        builder.plugin::<PeerFamily>(PeerAdapter::new(&peer));
        let cx = builder.context();

        let reply = ask(&cx, "other", "box", plate()).into_result().unwrap();

        assert!(reply.text.starts_with("ok plate@1 box"), "{}", reply.text);
    }
}
