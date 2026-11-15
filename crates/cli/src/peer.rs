//! Another server reached over JSON-RPC, and a server joined from several.
//!
//! A peer is a program that speaks the plain dialect on its standard input and output,
//! such as `krlyvis`. It is started once and its methods are read once. [`Joined`] serves
//! the methods of one endpoint and of its peers side by side: `vis.analyse` goes to the
//! peer named `vis` as `analyse`, everything else to the endpoint itself. An agent working
//! with krly3d can thus ask krlyvis about a photograph in the same connection.

use crate::endpoint::Endpoint;
use crate::json;
use crate::rpc::INVALID_PARAMS;
use krly3d_core::Exactness;
use krly3d_core::{Attempt, CallControl, Error, Id};
use krly3d_kernel::ports::peer::{Peer as PeerFamily, PeerCall};
use krly3d_kernel::schema::{Kind, Param, Params, Schema, Value};
use krly3d_kernel::{describe, Method, MethodKind, Reply};
use krly3d_kernel::{Context, Plugin};
use serde_json::{json, Value as Json};
use std::collections::BTreeSet;
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
        peer.methods =
            methods_of(entries, &SCHEMA_STRINGS).map_err(|reason| format!("peer `{name}`: {reason}"))?;
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
            _ => reply_from_rpc_error(error).ok_or_else(|| {
                error.get("message").and_then(Json::as_str).unwrap_or("the call failed").to_string()
            }),
        }
    }
}

/// A peer may send a standard invalid-params error without the plain dialect's optional
/// whole-reply payload. Preserve its Session-generated help and error kind in that case.
fn reply_from_rpc_error(error: &Json) -> Option<Reply> {
    if error.get("code").and_then(Json::as_i64) != Some(INVALID_PARAMS) {
        return None;
    }
    Some(Reply {
        ok: false,
        text: error.get("message")?.as_str()?.to_string(),
        data: Value::map([("error", Value::from("input"))]),
        exact: Exactness::Exact,
    })
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

/// Borrowed, fully validated schema data. Nothing enters the process pool until every
/// method in the response has passed validation, including defaults and later entries.
struct MethodSchema<'a> {
    name: &'a str,
    kind: MethodKind,
    about: &'a str,
    inputs: (usize, usize),
    params: Vec<ParamSchema<'a>>,
}

struct ParamSchema<'a> {
    name: &'a str,
    kind: Kind,
    required: bool,
    default: Option<&'a Value>,
    about: &'a str,
}

fn method_of(value: &Value) -> Result<MethodSchema<'_>, String> {
    let entries = map_of(value, "a method")?;
    let text = |key: &str| text_of(entries.get(key), key);
    let name = text("name")?;
    if name.is_empty() {
        return Err("a method name cannot be empty".into());
    }
    let kind = match text("kind")? {
        "tool" => MethodKind::Tool,
        "query" => MethodKind::Query,
        "session" => MethodKind::Session,
        other => return Err(format!("`{other}` is not a kind of method")),
    };
    let inputs = match entries.get("inputs") {
        None => (0, 0),
        Some(Value::List(pair)) => match pair[..] {
            [Value::Number(least), Value::Number(most)]
                if input_count(least) && input_count(most) && least <= most =>
            {
                (least as usize, most as usize)
            }
            _ => return Err("`inputs` must be two ordered nonnegative integer counts".into()),
        },
        Some(_) => return Err("`inputs` is not a list".into()),
    };
    let params: Vec<_> = match entries.get("params") {
        None => Vec::new(),
        Some(Value::List(params)) => params.iter().map(param_of).collect::<Result<_, _>>()?,
        Some(_) => return Err("`params` is not a list".into()),
    };
    let mut names = BTreeSet::new();
    for param in &params {
        if !names.insert(param.name) {
            return Err(format!("parameter `{}` is listed twice", param.name));
        }
    }
    Ok(MethodSchema { name, kind, about: text("about")?, inputs, params })
}

fn input_count(number: f64) -> bool {
    // describe() encodes usize::MAX (the unbounded-input sentinel) as f64, which
    // rounds upward on 64-bit targets. Accept that representation; the cast back
    // deliberately saturates to usize::MAX, as the existing wire format requires.
    number.is_finite() && number >= 0.0 && number.fract() == 0.0 && number <= usize::MAX as f64
}

fn param_of(value: &Value) -> Result<ParamSchema<'_>, String> {
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
    let name = text_of(entries.get("name"), "name")?;
    if name.is_empty() {
        return Err("a parameter name cannot be empty".into());
    }
    let required = match entries.get("required") {
        None => false,
        Some(Value::Bool(required)) => *required,
        Some(_) => return Err("`required` is not a flag".into()),
    };
    let default = entries.get("default");
    if let Some(value) = default {
        // Reuse the kernel's kind validation without interning any peer-provided text.
        Schema { about: "", inputs: (0, 0), params: vec![Param::required("default", kind, "")] }
            .admit(&Id::new(name), &Params::new().with("default", value.clone()), 0)
            .map_err(|error| error.to_string())?;
    }
    Ok(ParamSchema { name, kind, required, default, about: text_of(entries.get("about"), "about")? })
}

fn methods_of(entries: &[Value], strings: &Mutex<SchemaStrings>) -> Result<Vec<Method>, String> {
    let schemas = entries.iter().map(method_of).collect::<Result<Vec<_>, _>>()?;
    let mut names = BTreeSet::new();
    let mut needed = BTreeSet::new();
    for schema in &schemas {
        if !names.insert(schema.name) {
            return Err(format!("method `{}` is listed twice", schema.name));
        }
        needed.insert(schema.about);
        for param in &schema.params {
            needed.insert(param.name);
            needed.insert(param.about);
        }
    }
    let mut strings = strings.lock().unwrap_or_else(PoisonError::into_inner);
    strings.intern(&needed)?;
    schemas
        .into_iter()
        .map(|schema| {
            Ok(Method {
                name: schema.name.to_string(),
                kind: schema.kind,
                schema: Schema {
                    about: strings.get(schema.about)?,
                    inputs: schema.inputs,
                    params: schema
                        .params
                        .into_iter()
                        .map(|param| {
                            Ok(Param {
                                name: strings.get(param.name)?,
                                kind: param.kind,
                                required: param.required,
                                default: param.default.cloned(),
                                about: strings.get(param.about)?,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                },
            })
        })
        .collect()
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

/// The schema API still requires static strings. These are interned in a bounded
/// process-lifetime pool, NOT owned by a peer/schema and NOT reclaimed on disconnect.
/// Identical strings across peers/connections share one allocation. Both payload bytes
/// and entry overhead are bounded globally; malformed lists and quota failures add none.
/// Replacing this pool with owned schemas requires a separate change to the schema API.
const SCHEMA_STRING_BYTES: usize = 4 * 1024 * 1024;
const SCHEMA_STRING_ENTRIES: usize = 32 * 1024;
static SCHEMA_STRINGS: Mutex<SchemaStrings> =
    Mutex::new(SchemaStrings::new(SCHEMA_STRING_BYTES, SCHEMA_STRING_ENTRIES));

struct SchemaStrings {
    strings: BTreeSet<&'static str>,
    bytes: usize,
    max_bytes: usize,
    max_entries: usize,
}

impl SchemaStrings {
    const fn new(max_bytes: usize, max_entries: usize) -> Self {
        Self { strings: BTreeSet::new(), bytes: 0, max_bytes, max_entries }
    }

    fn intern(&mut self, needed: &BTreeSet<&str>) -> Result<(), String> {
        // Preflight the WHOLE list while holding the same lock as insertion. A failed
        // admission never consumes quota, even if some of its strings would fit.
        let missing: Vec<_> = needed.iter().copied().filter(|text| !self.strings.contains(text)).collect();
        let bytes = missing.iter().try_fold(self.bytes, |bytes, text| bytes.checked_add(text.len()));
        if bytes.is_none_or(|bytes| bytes > self.max_bytes)
            || missing.len() > self.max_entries.saturating_sub(self.strings.len())
        {
            return Err(format!(
                "peer schema string pool exhausted (limits: {} bytes, {} entries)",
                self.max_bytes, self.max_entries
            ));
        }
        for text in missing {
            self.bytes += text.len();
            self.strings.insert(Box::leak(text.to_string().into_boxed_str()));
        }
        Ok(())
    }

    fn get(&self, text: &str) -> Result<&'static str, String> {
        // Only called for strings admitted by intern(), with the pool still locked.
        self.strings.get(text).copied().ok_or_else(|| "peer schema string was not interned".into())
    }
}

/// Maximum batch nesting inspected or dispatched here, including peer batch payloads.
const BATCH_DEPTH: usize = 32;

/// An endpoint and its peers, served as one. Mixed batches run in order and stop at
/// the first failure, like Session batches, but cannot roll back effects across peers.
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

    fn dispatch(&mut self, method: &str, args: &Params, control: Option<&Arc<CallControl>>) -> Reply {
        self.try_dispatch(method, args, control).unwrap_or_else(failed)
    }

    fn try_dispatch(
        &mut self,
        method: &str,
        args: &Params,
        control: Option<&Arc<CallControl>>,
    ) -> Result<Reply, Error> {
        if let Some(control) = control {
            control.check()?;
        }
        if method == "batch" {
            // Inspect borrowed values before admission/dispatch can clone deep data.
            // Reject excessive nesting before any call has effects, even remotely.
            if let Some(Value::List(calls)) = args.get("calls") {
                self.check_batch_depth(calls, 1)?;
            }
            let schema = Schema {
                about: "ordered calls; mixed batches cannot roll back completed calls",
                inputs: (0, 0),
                params: vec![Param::required(
                    "calls",
                    Kind::Any,
                    "a list of { method, args } maps; a batch may contain batches",
                )],
            };
            schema.admit(&Id::new("batch"), args, 0)?;
            let calls = args.list("calls")?;
            if self.needs_joined(calls) {
                return self.batch(calls, control);
            }
            // Let an entirely local batch retain the endpoint's own rollback behavior.
        }
        if method == "methods" {
            return Ok(describe(&self.methods()));
        }
        if let Some((prefix, rest)) = method.split_once('.') {
            if let Some(peer) = self.peers.iter().find(|peer| lock(peer).name() == prefix) {
                if rest == "batch" {
                    self.check_batch_depth(args.list("calls")?, 1)?;
                }
                // Synchronous remote I/O has no in-flight cancellation protocol yet.
                // A stop prevents dispatch and later calls, but cannot interrupt this.
                return Ok(lock(peer).call(rest, args));
            }
        }
        Ok(match control {
            Some(control) => self.local.call_with_control(method, args, Arc::clone(control)),
            None => self.local.call(method, args),
        })
    }

    fn check_batch_depth(&self, calls: &[Value], depth: usize) -> Result<(), Error> {
        if depth > BATCH_DEPTH {
            return Err(Error::input(format!("batch nesting exceeds {BATCH_DEPTH} levels")));
        }
        for call in calls {
            let Value::Map(call) = call else { continue };
            let Some(Value::Text(method)) = call.get("method") else { continue };
            let is_batch = method == "batch"
                || method.split_once('.').is_some_and(|(prefix, rest)| {
                    rest == "batch" && self.peers.iter().any(|peer| lock(peer).name() == prefix)
                });
            if let (true, Some(Value::Map(args))) = (is_batch, call.get("args")) {
                if let Some(Value::List(nested)) = args.get("calls") {
                    self.check_batch_depth(nested, depth + 1)?;
                }
            }
        }
        Ok(())
    }

    fn needs_joined(&self, calls: &[Value]) -> bool {
        calls.iter().any(|call| {
            let Value::Map(call) = call else { return false };
            let Some(Value::Text(method)) = call.get("method") else { return false };
            method == "methods"
                || method
                    .split_once('.')
                    .is_some_and(|(prefix, _)| self.peers.iter().any(|peer| lock(peer).name() == prefix))
                || (method == "batch"
                    && matches!(call.get("args"), Some(Value::Map(args))
                        if matches!(args.get("calls"), Some(Value::List(calls)) if self.needs_joined(calls))))
        })
    }

    fn batch(&mut self, calls: &[Value], control: Option<&Arc<CallControl>>) -> Result<Reply, Error> {
        let (mut replies, mut text) = (Vec::with_capacity(calls.len()), String::new());
        for (index, call) in calls.iter().enumerate() {
            let (method, args) = one_call(call)
                .map_err(|error| Error::input(format!("call {}: {}", index + 1, error.message)))?;
            let reply = self.dispatch(&method, &args, control);
            text.push_str(&reply.text);
            if !reply.text.ends_with('\n') {
                text.push('\n');
            }
            let stopped = !reply.ok;
            replies.push(reply);
            if stopped {
                text.push_str(&format!(
                    "call {} of {} failed; completed calls were not rolled back across endpoints\n",
                    index + 1,
                    calls.len()
                ));
                break;
            }
        }
        let ok = replies.iter().all(|reply| reply.ok);
        let exact = replies.iter().fold(Exactness::Exact, |exact, reply| exact.and(reply.exact));
        let data = Value::map([("replies", Value::List(replies.iter().map(Reply::to_value).collect()))]);
        Ok(Reply { ok, text, data, exact })
    }
}

/// Keep Session's per-entry validation and error shape, including optional arguments.
fn one_call(call: &Value) -> Result<(String, Params), Error> {
    let Value::Map(entries) = call else {
        return Err(Error::input("a call is a map with `method` and `args`"));
    };
    let method = match entries.get("method") {
        Some(Value::Text(method)) => method.clone(),
        _ => return Err(Error::input("a call needs a `method` given as text")),
    };
    match entries.get("args") {
        None => Ok((method, Params::new())),
        Some(Value::Map(args)) => Ok((method, Params::from_map(args.clone()))),
        Some(_) => Err(Error::input("the `args` of a call are a map from argument name to value")),
    }
}

fn failed(error: Error) -> Reply {
    Reply {
        ok: false,
        text: error.to_string(),
        data: Value::map([("error", Value::from(error.kind.label()))]),
        exact: Exactness::Exact,
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
        text.push_str(
            " Mixed batches run in order and stop at the first failure; completed calls \
            cannot be rolled back across endpoints. Batch nesting is limited to 32 levels. \
            In-flight peer calls cannot yet be cancelled.",
        );
        text
    }

    fn methods(&self) -> Vec<Method> {
        let mut methods = self.local.methods();
        if let Some(batch) = methods.iter_mut().find(|method| method.name == "batch") {
            batch.schema.about = "calls in order; stop on failure; mixed batches do not roll back across endpoints (maximum nesting: 32)";
        }
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
        self.dispatch(method, args, None)
    }

    fn call_with_control(&mut self, method: &str, args: &Params, control: Arc<CallControl>) -> Reply {
        self.dispatch(method, args, Some(&control))
    }
}

#[cfg(test)]
mod tests {
    use super::{methods_of, Joined, Link, Peer, PeerAdapter, SchemaStrings, SharedPeer, BATCH_DEPTH};
    use crate::dialect::plain::Plain;
    use crate::dialect::testing::session;
    use crate::dialect::Dialect;
    use crate::endpoint::Endpoint;
    use crate::rpc;
    use krly3d_core::{Attempt, CallControl, Exactness, Id};
    use krly3d_kernel::ports::peer::{ask, Peer as PeerFamily};
    use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
    use krly3d_kernel::schema::{Params, Schema, Value};
    use krly3d_kernel::{Context, KernelBuilder, Plugin, Reply, Session};
    use serde_json::{json, Value as Json};
    use std::collections::{BTreeSet, VecDeque};
    use std::io::{self, BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

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

    struct WithoutErrorData(Loopback);

    impl Link for WithoutErrorData {
        fn send(&mut self, message: &[u8]) -> io::Result<()> {
            self.0.send(message)
        }

        fn receive(&mut self) -> io::Result<Vec<u8>> {
            let mut message: Json = serde_json::from_slice(&self.0.receive()?).map_err(io::Error::other)?;
            if let Some(error) = message.get_mut("error").and_then(Json::as_object_mut) {
                error.remove("data");
            }
            serde_json::to_vec(&message).map_err(io::Error::other)
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
    fn joined_local_calls_forward_control_even_with_a_peer() {
        let mut joined = Joined::new(session());
        joined.join(peer()).unwrap();
        let control = Arc::new(krly3d_core::CallControl::new());
        control.cancel();

        let reply = joined.call_with_control("mark", &Params::new(), control);

        assert!(!reply.ok);
        assert_eq!(reply.data, krly3d_kernel::schema::Value::map([("error", "cancelled".into())]));
        assert_eq!(joined.call("mark", &Params::new()).text, "mark 1");
        assert!(joined.call("other.box", &plate()).ok);
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
    fn a_peer_invalid_params_error_without_reply_data_matches_the_session() {
        let mut peer = Peer::connect("other", Box::new(WithoutErrorData(loopback(false).0))).unwrap();

        let reply = peer.call("box", &Params::new());

        assert_eq!(reply, session().call("box", &Params::new()));
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

    /// Exercise real line I/O over TCP as well as the in-process plain-dialect link.
    struct SocketLink {
        input: TcpStream,
        output: BufReader<TcpStream>,
    }

    impl Link for SocketLink {
        fn send(&mut self, message: &[u8]) -> io::Result<()> {
            self.input.write_all(message)?;
            self.input.write_all(b"\n")?;
            self.input.flush()
        }

        fn receive(&mut self) -> io::Result<Vec<u8>> {
            let mut line = Vec::new();
            if self.output.read_until(b'\n', &mut line)? == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            }
            Ok(line)
        }
    }

    fn socket_peer() -> (SharedPeer, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let input = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        input.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        input.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        let output = BufReader::new(input.try_clone().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut session = session();
            let mut line = Vec::new();
            while reader.read_until(b'\n', &mut line).unwrap() != 0 {
                let answer =
                    rpc::answer(&line, &mut |method, params| Plain.answer(&mut session, method, params));
                serde_json::to_writer(&mut stream, &answer).unwrap();
                stream.write_all(b"\n").unwrap();
                stream.flush().unwrap();
                line.clear();
            }
        });
        let peer = Peer::connect("other", Box::new(SocketLink { input, output })).unwrap().shared();
        (peer, server)
    }

    fn one(method: &str, args: Params) -> Value {
        Value::map([("method", method.into()), ("args", args.into_value())])
    }

    fn batch(calls: Vec<Value>) -> Params {
        Params::new().with("calls", calls)
    }

    fn replies(reply: &Reply) -> &[Value] {
        let Value::Map(data) = &reply.data else { panic!("expected batch data: {reply:?}") };
        let Some(Value::List(replies)) = data.get("replies") else { panic!("expected replies: {reply:?}") };
        replies
    }

    #[test]
    fn mixed_nested_batches_route_through_the_joined_endpoint_over_tcp_loopback() {
        let (peer, server) = socket_peer();
        let mut joined = Joined::new(session());
        joined.join(peer).unwrap();
        let args = batch(vec![
            one("other.box", plate()),
            one("box", plate().with("id", "local")),
            one("batch", batch(vec![one("other.save", Params::new()), one("save", Params::new())])),
            Value::map([("method", "methods".into())]),
        ]);

        let reply = joined.call("batch", &args);

        assert!(reply.ok, "{reply:?}");
        assert_eq!(replies(&reply).len(), 4);
        assert!(reply.text.contains("plate = box"));
        assert!(reply.text.contains("local = box"));
        assert!(reply.text.contains("other.box (tool)"));
        assert!(joined.call("save", &Params::new()).text.contains("local = box"));
        assert!(!joined.call("save", &Params::new()).text.contains("plate = box"));
        assert!(joined.call("other.save", &Params::new()).text.contains("plate = box"));
        drop(joined);
        server.join().unwrap();
    }

    #[test]
    fn mixed_batches_stop_at_local_or_remote_failure_and_report_retained_effects() {
        for method in ["missing", "other.missing", "box", "other.box"] {
            let mut joined = Joined::new(session());
            joined.join(peer()).unwrap();
            let failure_args = Params::new().with("w", "wide");
            let mut expected = session();
            let expected = expected.call(method.strip_prefix("other.").unwrap_or(method), &failure_args);
            let reply = joined.call(
                "batch",
                &batch(vec![
                    one("box", plate()),
                    one("other.box", plate()),
                    one(method, failure_args),
                    one("mark", Params::new()),
                    one("other.mark", Params::new()),
                ]),
            );

            assert!(!reply.ok, "{method}: {reply:?}");
            assert_eq!(replies(&reply).len(), 3);
            assert_eq!(replies(&reply)[2], expected.to_value());
            assert!(reply.text.contains("not rolled back across endpoints"));
            assert!(!reply.text.contains("nothing was kept"));
            assert!(joined.call("save", &Params::new()).text.contains("plate = box"));
            assert!(joined.call("other.save", &Params::new()).text.contains("plate = box"));
            assert_eq!(joined.call("mark", &Params::new()).text, "mark 1");
            assert_eq!(joined.call("other.mark", &Params::new()).text, "mark 1");
        }
    }

    #[test]
    fn a_nested_mixed_failure_stops_its_outer_batch() {
        let mut joined = Joined::new(session());
        joined.join(peer()).unwrap();
        let reply = joined.call(
            "batch",
            &batch(vec![
                one("mark", Params::new()),
                one(
                    "batch",
                    batch(vec![
                        one("other.mark", Params::new()),
                        one("other.missing", Params::new()),
                        one("other.mark", Params::new()),
                    ]),
                ),
                one("mark", Params::new()),
            ]),
        );

        assert!(!reply.ok);
        assert_eq!(replies(&reply).len(), 2);
        let Value::Map(nested) = &replies(&reply)[1] else { panic!("expected nested reply") };
        let Some(Value::Map(data)) = nested.get("data") else { panic!("expected nested data") };
        assert!(matches!(data.get("replies"), Some(Value::List(items)) if items.len() == 2));
        assert_eq!(joined.call("mark", &Params::new()).text, "mark 2");
        assert_eq!(joined.call("other.mark", &Params::new()).text, "mark 2");
    }

    #[test]
    fn malformed_mixed_entries_match_sessions_error_shape_and_stop_later_calls() {
        let malformed = [
            Value::from("not a call"),
            Value::map([]),
            Value::map([("method", 1.0.into())]),
            Value::map([("method", "mark".into()), ("args", false.into())]),
        ];
        for entry in malformed {
            let expected = session().call("batch", &batch(vec![one("save", Params::new()), entry.clone()]));
            let mut joined = Joined::new(session());
            joined.join(peer()).unwrap();
            let reply = joined.call(
                "batch",
                &batch(vec![
                    one("other.mark", Params::new()),
                    entry,
                    one("mark", Params::new()),
                    one("other.mark", Params::new()),
                ]),
            );

            assert_eq!(reply, expected);
            assert_eq!(joined.call("mark", &Params::new()).text, "mark 1");
            assert_eq!(joined.call("other.mark", &Params::new()).text, "mark 2");
        }
    }

    #[test]
    fn local_batches_keep_sessions_rollback_and_argument_validation() {
        let mut joined = Joined::new(session());
        joined.join(peer()).unwrap();
        let cases = [
            Params::new(),
            Params::new().with("calls", false),
            batch(vec![]).with("extra", true),
            batch(vec![]),
            batch(vec![one("mark", Params::new()), one("missing", Params::new())]),
            batch(vec![one("mark", Params::new()), Value::from("invalid")]),
            batch(vec![one("batch", batch(vec![one("mark", Params::new()), one("missing", Params::new())]))]),
        ];
        for args in cases {
            assert_eq!(joined.call("batch", &args), session().call("batch", &args));
        }
        assert_eq!(joined.call("mark", &Params::new()).text, "mark 1");
    }

    fn nested_batch(depth: usize, leaf: &str) -> Params {
        let mut args = batch(vec![one(leaf, Params::new())]);
        for _ in 1..depth {
            args = batch(vec![one("batch", args)]);
        }
        args
    }

    #[test]
    fn nesting_limit_covers_local_mixed_and_prefixed_batches_before_effects() {
        for leaf in ["mark", "other.mark"] {
            let mut joined = Joined::new(session());
            joined.join(peer()).unwrap();
            let reply = joined.call("batch", &nested_batch(BATCH_DEPTH, leaf));
            assert!(reply.ok, "{reply:?}");
            let args = batch(vec![one("mark", Params::new()), one("batch", nested_batch(BATCH_DEPTH, leaf))]);
            let reply = joined.call("batch", &args);
            assert_eq!(reply.data, Value::map([("error", "input".into())]));
            assert!(reply.text.contains("batch nesting exceeds"));
            assert_eq!(
                joined.call("mark", &Params::new()).text,
                if leaf == "mark" { "mark 2" } else { "mark 1" }
            );
        }
        let mut joined = Joined::new(session());
        joined.join(peer()).unwrap();
        let reply = joined.call("other.batch", &nested_batch(BATCH_DEPTH + 1, "mark"));
        assert!(!reply.ok);
        assert_eq!(joined.call("other.mark", &Params::new()).text, "mark 1");
        let args =
            batch(vec![one("mark", Params::new()), one("other.batch", nested_batch(BATCH_DEPTH, "mark"))]);
        assert!(!joined.call("batch", &args).ok);
        assert_eq!(joined.call("mark", &Params::new()).text, "mark 1");
    }

    #[test]
    fn a_cancelled_control_prevents_remote_dispatch_and_mixed_batch_effects() {
        let mut joined = Joined::new(session());
        joined.join(peer()).unwrap();
        let control = Arc::new(CallControl::new());
        control.cancel();
        for method in ["other.mark", "batch", "methods"] {
            let args = batch(vec![one("other.mark", Params::new()), one("mark", Params::new())]);
            let reply = joined.call_with_control(method, &args, Arc::clone(&control));
            assert!(!reply.ok);
            assert_eq!(reply.data, Value::map([("error", "cancelled".into())]));
        }
        assert_eq!(joined.call("mark", &Params::new()).text, "mark 1");
        assert_eq!(joined.call("other.mark", &Params::new()).text, "mark 1");
    }

    struct StopAfterSend {
        inner: Loopback,
        control: Arc<CallControl>,
    }

    impl Link for StopAfterSend {
        fn send(&mut self, message: &[u8]) -> io::Result<()> {
            self.inner.send(message)?;
            let message: Json = serde_json::from_slice(message).map_err(io::Error::other)?;
            if message["method"] == "mark" {
                self.control.cancel();
            }
            Ok(())
        }

        fn receive(&mut self) -> io::Result<Vec<u8>> {
            self.inner.receive()
        }
    }

    #[test]
    fn cancellation_after_remote_success_keeps_that_reply_and_stops_the_next_call() {
        let control = Arc::new(CallControl::new());
        let link = StopAfterSend { inner: loopback(false).0, control: Arc::clone(&control) };
        let peer = Peer::connect("other", Box::new(link)).unwrap().shared();
        let mut joined = Joined::new(session());
        joined.join(peer).unwrap();
        let args = batch(vec![
            one("other.mark", Params::new()),
            one("mark", Params::new()),
            one("other.mark", Params::new()),
        ]);

        let reply = joined.call_with_control("batch", &args, control);

        assert!(!reply.ok);
        assert_eq!(replies(&reply).len(), 2);
        assert!(
            matches!(&replies(&reply)[0], Value::Map(data) if data.get("ok") == Some(&Value::Bool(true)))
        );
        assert!(matches!(&replies(&reply)[1], Value::Map(data)
            if data.get("data") == Some(&Value::map([("error", "cancelled".into())]))));
        assert_eq!(joined.call("mark", &Params::new()).text, "mark 1");
        assert_eq!(joined.call("other.mark", &Params::new()).text, "mark 2");
    }

    struct StopInQuery {
        control: Arc<CallControl>,
    }

    impl Plugin<Query> for StopInQuery {
        fn id(&self) -> Id {
            Id::new("stop")
        }

        fn schema(&self) -> Option<Schema> {
            Some(Schema { about: "cancel during a query", inputs: (0, 0), params: vec![] })
        }

        fn call(&self, _: &Context, _: &QueryRequest) -> Attempt<Answer> {
            self.control.cancel();
            Attempt::Done(Answer {
                text: "late success".into(),
                data: Value::map([]),
                exact: Exactness::Exact,
            })
        }
    }

    #[test]
    fn nested_mixed_batches_forward_control_to_the_local_query_and_skip_later_calls() {
        let control = Arc::new(CallControl::new());
        let mut builder = KernelBuilder::new();
        builder.plugin::<Query>(StopInQuery { control: Arc::clone(&control) });
        let mut joined = Joined::new(builder.session());
        joined.join(peer()).unwrap();
        let args = batch(vec![
            one("other.mark", Params::new()),
            one(
                "batch",
                batch(vec![
                    one("other.mark", Params::new()),
                    one("stop", Params::new()),
                    one("other.mark", Params::new()),
                ]),
            ),
            one("mark", Params::new()),
        ]);

        let reply = joined.call_with_control("batch", &args, control);

        assert!(!reply.ok);
        assert_eq!(replies(&reply).len(), 2);
        assert!(reply.text.contains("cancelled: cancelled"));
        assert!(!reply.text.contains("late success"));
        assert_eq!(joined.call("mark", &Params::new()).text, "mark 1");
        assert_eq!(joined.call("other.mark", &Params::new()).text, "mark 3");
    }

    #[test]
    fn transport_failure_in_a_batch_is_a_failed_entry_and_stops_dispatch() {
        let remote = peer();
        remote.lock().unwrap().link = Box::new(Broken);
        let mut joined = Joined::new(session());
        joined.join(remote).unwrap();
        let reply = joined.call(
            "batch",
            &batch(vec![
                one("mark", Params::new()),
                one("other.mark", Params::new()),
                one("mark", Params::new()),
            ]),
        );

        assert!(!reply.ok);
        assert_eq!(replies(&reply).len(), 2);
        assert!(matches!(&replies(&reply)[1], Value::Map(data)
            if data.get("data") == Some(&Value::map([("error", "peer".into())]))));
        assert_eq!(joined.call("mark", &Params::new()).text, "mark 2");
    }

    #[test]
    fn batch_routes_to_multiple_peers_without_combining_their_sessions() {
        let mut joined = Joined::new(session());
        joined.join(peer()).unwrap();
        joined.join(Peer::connect("second", Box::new(loopback(false).0)).unwrap().shared()).unwrap();
        let reply = joined.call(
            "batch",
            &batch(vec![
                one("other.mark", Params::new()),
                one("second.mark", Params::new()),
                one("mark", Params::new()),
            ]),
        );

        assert!(reply.ok, "{reply:?}");
        assert_eq!(replies(&reply).len(), 3);
        assert_eq!(reply.text, "mark 1\nmark 1\nmark 1\n");
        for method in ["mark", "other.mark", "second.mark"] {
            assert_eq!(joined.call(method, &Params::new()).text, "mark 2");
        }
    }

    #[test]
    fn repeated_connections_share_every_interned_schema_string() {
        let first = peer();
        for _ in 0..8 {
            let next = peer();
            let first = first.lock().unwrap();
            let next = next.lock().unwrap();
            for (first, next) in first.methods().iter().zip(next.methods()) {
                assert!(std::ptr::eq(first.schema.about, next.schema.about));
                for (first, next) in first.schema.params.iter().zip(&next.schema.params) {
                    assert!(std::ptr::eq(first.name, next.name));
                    assert!(std::ptr::eq(first.about, next.about));
                }
            }
        }
    }

    fn schema_entry() -> Value {
        Value::map([
            ("name", "example".into()),
            ("kind", "query".into()),
            ("about", "example schema".into()),
            ("inputs", vec![0.0.into(), 0.0.into()].into()),
            (
                "params",
                vec![Value::map([
                    ("name", "amount".into()),
                    ("kind", "number".into()),
                    ("required", false.into()),
                    ("default", 2.0.into()),
                    ("about", "an amount".into()),
                ])]
                .into(),
            ),
        ])
    }

    fn field(value: &mut Value, key: &str, replacement: Value) {
        let Value::Map(entries) = value else { panic!("a schema map") };
        entries.insert(key.into(), replacement);
    }

    #[test]
    fn invalid_method_lists_consume_no_pool_quota_including_late_schema_errors() {
        let pool = Mutex::new(SchemaStrings::new(1024, 32));
        let mut invalids = Vec::new();
        for (key, value) in [
            ("name", false.into()),
            ("about", false.into()),
            ("kind", "unknown".into()),
            ("inputs", vec![(-1.0).into(), 0.0.into()].into()),
            ("inputs", vec![0.5.into(), 1.0.into()].into()),
            ("inputs", vec![2.0.into(), 1.0.into()].into()),
            ("inputs", false.into()),
            ("params", false.into()),
        ] {
            let mut invalid = schema_entry();
            field(&mut invalid, key, value);
            invalids.push(invalid);
        }
        for (key, value) in [
            ("name", false.into()),
            ("about", false.into()),
            ("kind", "unknown".into()),
            ("required", "yes".into()),
            ("default", "not a number".into()),
        ] {
            let mut invalid = schema_entry();
            let Value::Map(method) = &mut invalid else { panic!("method map") };
            let Some(Value::List(params)) = method.get_mut("params") else { panic!("params list") };
            field(&mut params[0], key, value);
            invalids.push(invalid);
        }
        for invalid in invalids {
            assert!(methods_of(&[schema_entry(), invalid], &pool).is_err());
            let pool = pool.lock().unwrap();
            assert_eq!(pool.bytes, 0);
            assert!(pool.strings.is_empty());
        }
        assert!(methods_of(&[schema_entry(), schema_entry()], &pool).is_err());
        assert_eq!(pool.lock().unwrap().bytes, 0);
    }

    #[test]
    fn pool_byte_and_entry_exhaustion_never_partially_interns_or_grows_past_quotas() {
        for (max_bytes, max_entries) in [(5, 10), (100, 2)] {
            let mut pool = SchemaStrings::new(max_bytes, max_entries);
            pool.intern(&BTreeSet::from(["same"])).unwrap();
            let saved = pool.get("same").unwrap();
            for _ in 0..8 {
                pool.intern(&BTreeSet::from(["same"])).unwrap();
                assert!(std::ptr::eq(saved, pool.get("same").unwrap()));
                let error = pool.intern(&BTreeSet::from(["same", "a", "new"])).unwrap_err();
                assert!(error.contains("schema string pool exhausted"));
                assert_eq!(pool.bytes, 4);
                assert_eq!(pool.strings.len(), 1);
                assert!(pool.bytes <= max_bytes && pool.strings.len() <= max_entries);
            }
        }
        let pool = Mutex::new(SchemaStrings::new(1, 1));
        assert!(methods_of(&[schema_entry()], &pool).unwrap_err().contains("pool exhausted"));
        assert_eq!(pool.lock().unwrap().bytes, 0);
        assert!(pool.lock().unwrap().strings.is_empty());
    }

    #[test]
    fn identical_strings_within_and_between_schema_lists_share_an_allocation() {
        let pool = Mutex::new(SchemaStrings::new(1024, 32));
        let mut entry = schema_entry();
        field(&mut entry, "about", "amount".into());
        let first = methods_of(&[entry.clone()], &pool).unwrap();
        let before = pool.lock().unwrap().bytes;
        let next = methods_of(&[entry], &pool).unwrap();
        assert!(std::ptr::eq(first[0].schema.about, first[0].schema.params[0].name));
        assert!(std::ptr::eq(first[0].schema.about, next[0].schema.about));
        assert_eq!(pool.lock().unwrap().bytes, before);
        assert_eq!(pool.lock().unwrap().strings.len(), 2);
    }
}
