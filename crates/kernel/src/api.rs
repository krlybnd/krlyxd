//! The kernel behind one door.
//!
//! A [`Session`] is everything a client needs. [`Session::methods`] says what can be
//! called and with which arguments; [`Session::call`] calls it. Arguments and results are
//! nested [`Value`]s, which map one to one onto JSON, so a command line, a JSON-RPC or MCP
//! server, or a binding to another language is only a way of carrying
//! `(method, arguments) -> Reply`. None of them needs to know a single tool by name.
//!
//! Three kinds of methods share the one namespace:
//!
//! * **Tools** — every registered tool under its own name (`box`, `extrude`, …). A tool
//!   call adds one statement to the document. Besides the tool's own parameters it takes
//!   `id` (the node to define), `inputs` (the nodes it reads), `dry` (report and keep
//!   nothing) and `expect` (statements that must hold).
//! * **Queries** — every registered query under its own name. A query changes nothing.
//! * **Session verbs** — `methods`, `open`, `save`, `mark`, `rollback`, `batch`.
//!
//! A call never panics and never returns a Rust error: whatever goes wrong comes back as a
//! [`Reply`] with `ok` false, the reason in `text` and its kind in `data`.

use crate::kernel::{Checkpoint, Kernel};
use crate::model::{Delta, Statement};
use crate::ports::query::{self, Query};
use crate::ports::read::read;
use crate::ports::tool::Tool;
use crate::ports::write::write;
use crate::schema::{Kind, Param, Params, Schema, Value};
use krly3d_core::{Error, Exactness, Id};
use std::collections::BTreeMap;

/// What a method does to the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodKind {
    /// Adds a statement to the document.
    Tool,
    /// Answers a question and changes nothing.
    Query,
    /// Acts on the session itself.
    Session,
}

impl MethodKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Query => "query",
            Self::Session => "session",
        }
    }
}

/// One callable method and everything a client needs to call it.
#[derive(Clone, Debug, PartialEq)]
pub struct Method {
    pub name: String,
    pub kind: MethodKind,
    /// What the method is for and the arguments it takes. For a tool, the arguments common
    /// to all tools (`id`, `inputs`, `dry`, `expect`) are included.
    pub schema: Schema,
}

/// The result of a call.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    /// Whether the call did what was asked.
    pub ok: bool,
    /// The result for a reader of text; for a failed call, why it failed.
    pub text: String,
    /// The result as data; for a failed call, `{ "error": kind }`.
    pub data: Value,
    /// How far the numbers in the reply can be trusted.
    pub exact: Exactness,
}

impl Reply {
    fn done(text: String, data: Value, exact: Exactness) -> Self {
        Self { ok: true, text, data, exact }
    }

    fn failed(error: Error) -> Self {
        let data = Value::map([("error", Value::from(error.kind.label()))]);
        Self { ok: false, text: error.to_string(), data, exact: Exactness::Exact }
    }

    /// The whole reply as one value, for transports that carry a single object.
    pub fn to_value(&self) -> Value {
        Value::map([
            ("ok", Value::from(self.ok)),
            ("text", Value::from(self.text.as_str())),
            ("data", self.data.clone()),
            ("exact", Value::from(self.exact.symbol().to_string())),
        ])
    }
}

/// A kernel with its document, driven through [`Session::call`].
pub struct Session {
    kernel: Kernel,
    marks: Vec<Checkpoint>,
}

impl Session {
    pub fn new(kernel: Kernel) -> Self {
        Self { kernel, marks: Vec::new() }
    }

    /// The kernel underneath, for a client written in Rust that wants typed access.
    pub fn kernel(&self) -> &Kernel {
        &self.kernel
    }

    /// Every method this session answers: the session verbs, then the tools, then the
    /// queries, each group in name order. The list follows what is registered, so a plugin
    /// added to the kernel appears here without further work.
    pub fn methods(&self) -> Vec<Method> {
        let cx = self.kernel.context();
        let mut methods: Vec<Method> = verbs()
            .into_iter()
            .map(|(name, schema)| Method { name: name.to_string(), kind: MethodKind::Session, schema })
            .collect();
        if let Some(tools) = cx.registry::<Tool>() {
            for (id, mut schema) in tools.ids().filter_map(|id| Some((id, tools.schema(id)?))) {
                schema.params.extend(statement_params());
                methods.push(Method { name: id.to_string(), kind: MethodKind::Tool, schema });
            }
        }
        if let Some(queries) = cx.registry::<Query>() {
            for (id, schema) in queries.ids().filter_map(|id| Some((id, queries.schema(id)?))) {
                methods.push(Method { name: id.to_string(), kind: MethodKind::Query, schema });
            }
        }
        methods
    }

    /// Calls `method` with the arguments `args`.
    ///
    /// The arguments are checked against the schema of the method: a missing or unknown
    /// argument, or one of the wrong kind, fails the call with a message that names it.
    pub fn call(&mut self, method: &str, args: &Params) -> Reply {
        self.dispatch(method, args).unwrap_or_else(Reply::failed)
    }

    fn dispatch(&mut self, method: &str, args: &Params) -> Result<Reply, Error> {
        let id = Id::new(method);
        if let Some((_, schema)) = verbs().into_iter().find(|(name, _)| *name == method) {
            return self.verb(method, &schema.admit(&id, args, 0)?);
        }
        let cx = self.kernel.context();
        if cx.registry::<Tool>().is_some_and(|tools| tools.contains(&id)) {
            return self.statement(&id, args);
        }
        if cx.registry::<Query>().is_some_and(|queries| queries.contains(&id)) {
            let (document, model) = self.kernel.snapshot();
            let answer = query::ask(cx, &id, args, &document, &model).into_result()?;
            return Ok(Reply::done(answer.text, answer.data, answer.exact));
        }
        Err(Error::not_found(format!("there is no method `{method}`; `methods` lists them")))
    }

    /// A tool call: one more statement in the document.
    fn statement(&mut self, tool: &Id, args: &Params) -> Result<Reply, Error> {
        let mut own = args.clone();
        let mut common = Params::new();
        for param in statement_params() {
            if let Some(value) = own.remove(param.name) {
                common.set(param.name, value);
            }
        }
        let common = Schema { about: "", inputs: (0, 0), params: statement_params().to_vec() }
            .admit(tool, &common, 0)?;
        let id = match common.text("id")? {
            "" => self.unused_name(tool),
            given => given.to_string(),
        };
        let mut inputs = common.texts("inputs")?;
        let reads_bodies = self
            .kernel
            .context()
            .registry::<Tool>()
            .and_then(|tools| tools.schema(tool))
            .is_some_and(|schema| schema.inputs.0 >= 1);
        if inputs.is_empty() && reads_bodies {
            // A tool that changes a body and is given none changes the node it defines.
            inputs.push(id.clone());
        }
        let inputs: Vec<&str> = inputs.iter().map(String::as_str).collect();
        let mut statement = Statement::new(&id, tool.as_str(), own, &inputs)?;
        statement.expect = common.texts("expect")?;
        let delta = if common.flag("dry")? {
            self.kernel.try_out(statement)?
        } else {
            self.kernel.apply(statement)?
        };
        Ok(delta_reply(&delta, Some(&id)))
    }

    /// `box1`, `box2`, …: the first name made from the tool's that no node has yet.
    fn unused_name(&self, tool: &Id) -> String {
        let taken: Vec<&str> = self.kernel.model().nodes().map(|(name, _)| name).collect();
        let stem: String = tool.as_str().chars().filter(|c| c.is_alphanumeric()).collect();
        (1..)
            .map(|number| format!("{stem}{number}"))
            .find(|name| !taken.contains(&name.as_str()))
            .unwrap_or(stem)
    }

    fn verb(&mut self, verb: &str, args: &Params) -> Result<Reply, Error> {
        match verb {
            "methods" => Ok(self.described()),
            "open" => {
                let document =
                    read(self.kernel.context(), args.text("format")?, args.text("text")?).into_result()?;
                Ok(delta_reply(&self.kernel.open(document)?, None))
            }
            "save" => {
                let format = args.text("format")?;
                let text = write(self.kernel.context(), format, self.kernel.document()).into_result()?;
                let data =
                    Value::map([("format", Value::from(format)), ("text", Value::from(text.as_str()))]);
                Ok(Reply::done(text, data, Exactness::Exact))
            }
            "mark" => {
                self.marks.push(self.kernel.checkpoint());
                let mark = self.marks.len();
                Ok(Reply::done(
                    format!("mark {mark}"),
                    Value::map([("mark", Value::from(mark))]),
                    Exactness::Exact,
                ))
            }
            "rollback" => {
                let stated = args.number("mark")?;
                let mark = if stated == 0.0 { self.marks.len() } else { stated as usize };
                let checkpoint = mark.checked_sub(1).and_then(|index| self.marks.get(index)).cloned();
                let checkpoint = checkpoint.ok_or_else(|| {
                    Error::not_found(format!("there is no mark {mark}; {} were set", self.marks.len()))
                })?;
                self.kernel.rollback(checkpoint);
                self.marks.truncate(mark);
                Ok(Reply::done(
                    format!("rolled back to mark {mark}"),
                    Value::map([("mark", Value::from(mark))]),
                    Exactness::Exact,
                ))
            }
            "batch" => self.batch(args.list("calls")?),
            other => Err(Error::not_found(format!("the session has no verb `{other}`"))),
        }
    }

    /// Calls in order; the first that fails undoes the ones before it.
    fn batch(&mut self, calls: &[Value]) -> Result<Reply, Error> {
        let before = self.kernel.checkpoint();
        let marks = self.marks.len();
        let (mut replies, mut text) = (Vec::with_capacity(calls.len()), String::new());
        for (index, call) in calls.iter().enumerate() {
            let (method, args) = one_call(call)
                .map_err(|error| Error::input(format!("call {}: {}", index + 1, error.message)))?;
            let reply = self.call(&method, &args);
            text.push_str(&reply.text);
            if !reply.text.ends_with('\n') {
                text.push('\n');
            }
            let failed = !reply.ok;
            replies.push(reply);
            if failed {
                self.kernel.rollback(before);
                self.marks.truncate(marks);
                text.push_str(&format!("call {} of {} failed; nothing was kept\n", index + 1, calls.len()));
                break;
            }
        }
        let ok = replies.iter().all(|reply| reply.ok);
        let exact = replies.iter().fold(Exactness::Exact, |exact, reply| exact.and(reply.exact));
        let data = Value::map([("replies", Value::List(replies.iter().map(Reply::to_value).collect()))]);
        Ok(Reply { ok, text, data, exact })
    }

    fn described(&self) -> Reply {
        describe(&self.methods())
    }
}

/// The answer to `methods` for a list of methods: one line per method, and every method
/// with its arguments as data. A transport that joins several servers lists them all the
/// same way.
pub fn describe(methods: &[Method]) -> Reply {
    let mut text = String::new();
    for method in methods {
        let params: Vec<String> = method
            .schema
            .params
            .iter()
            .map(|param| if !param.required { format!("[{}]", param.name) } else { param.name.to_string() })
            .collect();
        text.push_str(&format!(
            "{} ({}) {} — {}\n",
            method.name,
            method.kind.name(),
            params.join(" "),
            method.schema.about
        ));
    }
    Reply::done(text, Value::List(methods.iter().map(method_value).collect()), Exactness::Exact)
}

/// The arguments every tool call takes besides the tool's own.
fn statement_params() -> [Param; 4] {
    [
        Param::optional("id", Kind::Text, "", "name of the node to define; made up from the tool's name when left out"),
        Param::optional(
            "inputs",
            Kind::Texts,
            Vec::new(),
            "nodes the tool reads, as name or name@version; a tool that changes a body reads the node it defines when left out",
        ),
        Param::optional("dry", Kind::Flag, false, "report what would change and keep nothing"),
        Param::optional("expect", Kind::Texts, Vec::new(), "statements that must hold for the change to be kept"),
    ]
}

/// The session's own verbs and their arguments.
fn verbs() -> Vec<(&'static str, Schema)> {
    let verb = |about: &'static str, params: Vec<Param>| Schema { about, inputs: (0, 0), params };
    let format = || Param::optional("format", Kind::Text, "k3d", "name of the text format");
    vec![
        ("methods", verb("every method with its arguments", Vec::new())),
        (
            "open",
            verb(
                "replace the document by one given as text",
                vec![Param::required("text", Kind::Text, "the document"), format()],
            ),
        ),
        ("save", verb("the document as text", vec![format()])),
        ("mark", verb("remember the present state; returns the number of the mark", Vec::new())),
        (
            "rollback",
            verb(
                "return to a marked state",
                vec![Param::optional(
                    "mark",
                    Kind::Number,
                    0.0,
                    "number of the mark; the latest when left out",
                )],
            ),
        ),
        (
            "batch",
            verb(
                "several calls as one: the first that fails undoes the ones before it",
                vec![Param::required(
                    "calls",
                    Kind::Any,
                    "a list of { method, args } maps; a batch may contain batches",
                )],
            ),
        ),
    ]
}

/// One entry of a batch: `{ "method": name, "args": { … } }`, the arguments optional.
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

fn delta_reply(delta: &Delta, defined: Option<&str>) -> Reply {
    let nodes = delta.nodes.iter().map(|node| {
        let mut entries: BTreeMap<String, Value> = BTreeMap::new();
        entries.insert("node".into(), Value::from(node.node.as_str()));
        entries.insert("version".into(), Value::from(node.version as usize));
        entries.insert("tool".into(), Value::from(node.tool.as_str()));
        entries.insert("added".into(), Value::texts(node.added.iter().map(String::as_str)));
        entries.insert("removed".into(), Value::texts(node.removed.iter().map(String::as_str)));
        entries
            .insert("faces".into(), Value::List(vec![Value::from(node.faces.0), Value::from(node.faces.1)]));
        if let Some(volume) = node.volume {
            entries.insert("volume".into(), Value::from(volume.value));
        }
        Value::Map(entries)
    });
    let mut data: BTreeMap<String, Value> = BTreeMap::new();
    data.insert("nodes".into(), Value::List(nodes.collect()));
    data.insert("gone".into(), Value::texts(delta.gone.iter().map(String::as_str)));
    data.insert("committed".into(), Value::from(delta.committed));
    if let Some(id) = defined {
        data.insert("id".into(), Value::from(id));
    }
    let verdicts = delta.verdicts.iter().map(|verdict| {
        Value::map([
            ("node", Value::from(verdict.node.as_str())),
            ("claim", Value::from(verdict.claim.as_str())),
            ("holds", Value::from(verdict.holds)),
            ("found", Value::from(verdict.found.as_str())),
        ])
    });
    data.insert("verdicts".into(), Value::List(verdicts.collect()));
    let exact = delta
        .nodes
        .iter()
        .filter_map(|node| node.volume)
        .fold(Exactness::Exact, |exact, volume| exact.and(volume.exact));
    // An edit refused for an expectation that does not hold did not do what was asked.
    Reply { ok: !delta.refused(), text: delta.text(), data: Value::Map(data), exact }
}

fn method_value(method: &Method) -> Value {
    let params = method.schema.params.iter().map(|param| {
        let mut entries: BTreeMap<String, Value> = BTreeMap::new();
        entries.insert("name".into(), Value::from(param.name));
        entries.insert("kind".into(), Value::from(param.kind.name()));
        entries.insert("about".into(), Value::from(param.about));
        entries.insert("required".into(), Value::from(param.required));
        if let Some(default) = &param.default {
            entries.insert("default".into(), default.clone());
        }
        Value::Map(entries)
    });
    Value::map([
        ("name", Value::from(method.name.as_str())),
        ("kind", Value::from(method.kind.name())),
        ("about", Value::from(method.schema.about)),
        (
            "inputs",
            Value::List(vec![Value::from(method.schema.inputs.0), Value::from(method.schema.inputs.1)]),
        ),
        ("params", Value::List(params.collect())),
    ])
}
