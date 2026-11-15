//! MCP 2025-11-25 over bounded newline-delimited stdio.
//!
//! Mesh operation handles are domain objects, independent of MCP request IDs and
//! MCP tasks. All tool payloads are JSON text; event tokens come directly from the
//! domain encoder. No tools/call waits for mesh computation or worker cleanup.

use krly_mcp_json as json;
mod transport;

pub use transport::{serve, serve_stdio, MAX_MESSAGE_BYTES};

use crate::json::{Failure, Fields};
use krly3d_mesh::operation::{decode_operation_call, descriptor, encode_event, MeshEvent, MeshEventBody};
use krly3d_mesh::{diagnostic, Diagnostic};
use krly3d_mesh_engine::runner::MeshRunner;
use krly3d_mesh_engine::MeshEngine;
use krlyxd::OperationId;
use serde_json::{json, Value};

/// The sole supported MCP revision. Unsupported proposals negotiate this revision.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(PartialEq)]
enum Phase {
    New,
    Initializing,
    Ready,
}

/// Owns its runner: EOF and transport errors cancel work by dropping this session.
pub struct Server {
    engine: MeshEngine,
    runner: MeshRunner,
    phase: Phase,
}

impl Server {
    pub fn new(engine: MeshEngine, runner: MeshRunner) -> Self {
        Self { engine, runner, phase: Phase::New }
    }

    /// Process one complete frame. Notifications never execute tools or get replies.
    pub fn handle(&mut self, text: &str) -> Option<Value> {
        if let Err(error) = crate::json::validate(text) {
            return Some(error.response(Value::Null));
        }
        let fields = match Fields::read(text, &["jsonrpc", "id", "method", "params", "result", "error"]) {
            Ok(fields) => fields,
            Err(_) => return Some(Failure::new(-32600, "Invalid request").response(Value::Null)),
        };
        let id = match fields.0.get("id") {
            Some(raw) => match serde_json::from_str::<Value>(raw.get()) {
                Ok(id @ Value::String(_)) => Some(id),
                Ok(id @ Value::Number(_)) if id.is_i64() || id.is_u64() => Some(id),
                _ => return Some(Failure::new(-32600, "Invalid request ID").response(Value::Null)),
            },
            None => None,
        };
        if !matches!(fields.string("jsonrpc").as_deref(), Ok("2.0")) {
            return Some(
                Failure::new(-32600, "Invalid JSON-RPC version").response(id.unwrap_or(Value::Null)),
            );
        }
        // The server issues no requests. Ignore unsolicited client responses;
        // answering a response would create an error-response loop.
        if !fields.0.contains_key("method")
            && (fields.0.contains_key("result") || fields.0.contains_key("error"))
        {
            return None;
        }
        let method = match fields.string("method") {
            Ok(method) if !fields.0.contains_key("result") && !fields.0.contains_key("error") => method,
            _ => return Some(Failure::new(-32600, "Invalid method").response(id.unwrap_or(Value::Null))),
        };
        let params = fields.0.get("params").map(|raw| raw.get());
        let Some(id) = id else {
            if method == "notifications/initialized"
                && self.phase == Phase::Initializing
                && Fields::read(params.unwrap_or("{}"), &["_meta"]).is_ok()
            {
                self.phase = Phase::Ready;
            }
            // Unknown notifications, including MCP cancellation of already-answered
            // calls, cannot cancel domain operations. Use operation_cancel for that.
            return None;
        };
        let result = self.dispatch(&method, params);
        Some(match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => error.response(id),
        })
    }

    fn dispatch(&mut self, method: &str, params: Option<&str>) -> Result<Value, Failure> {
        match method {
            "ping" => {
                Fields::read(params.unwrap_or("{}"), &["_meta"])?;
                Ok(json!({}))
            }
            "initialize" => self.initialize(params),
            "tools/list" | "tools/call" if self.phase != Phase::Ready => {
                Err(Failure::new(-32000, "Complete MCP initialization first"))
            }
            "tools/list" => {
                let fields = Fields::read(params.unwrap_or("{}"), &["cursor", "_meta"])?;
                if fields.0.contains_key("cursor") {
                    return Err(Failure::params("this tool list has no pagination cursor"));
                }
                Ok(tools())
            }
            "tools/call" => self.call(params),
            _ => Err(Failure::new(-32601, "Method not found")),
        }
    }

    fn initialize(&mut self, params: Option<&str>) -> Result<Value, Failure> {
        if self.phase != Phase::New {
            return Err(Failure::new(-32600, "Session already initialized"));
        }
        let fields = Fields::read(
            params.unwrap_or("{}"),
            &["protocolVersion", "capabilities", "clientInfo", "_meta"],
        )?;
        let version = fields.string("protocolVersion")?;
        if version.is_empty() {
            return Err(Failure::params("protocolVersion must not be empty"));
        }
        let capabilities: Value = serde_json::from_str(fields.required("capabilities")?.get())
            .map_err(|_| Failure::params("invalid capabilities"))?;
        if !capabilities.is_object() {
            return Err(Failure::params("capabilities must be an object"));
        }
        let info: Value = serde_json::from_str(fields.required("clientInfo")?.get())
            .map_err(|_| Failure::params("invalid clientInfo"))?;
        if !info["name"].is_string() || !info["version"].is_string() {
            return Err(Failure::params("clientInfo requires string name and version"));
        }
        self.phase = Phase::Initializing;
        Ok(json!({"protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "krly3d-mesh-mcp", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "mesh accepts the v2 operation call and returns operation_id immediately. Poll operation_events for available canonical events; an empty events array means no event is available now. Failed/cancelled events set isError. Cancel with operation_cancel. Release finished operations with operation_release; retry release if a worker is still exiting. Mesh handles are not MCP tasks. All tool payloads are JSON text."}))
    }

    fn call(&self, params: Option<&str>) -> Result<Value, Failure> {
        // Task-augmented execution is deliberately not advertised or accepted.
        let fields = Fields::read(params.unwrap_or("{}"), &["name", "arguments", "_meta"])?;
        let name = fields.string("name")?;
        if !["mesh", "operation_events", "operation_cancel", "operation_release"].contains(&name.as_str()) {
            return Err(Failure::params(format!("Unknown tool: {name}")));
        }
        let args = fields.0.get("arguments").map_or("{}", |raw| raw.get());
        if !args.starts_with('{') {
            return Err(Failure::params("arguments must be an object"));
        }
        let result = match name.as_str() {
            "mesh" => decode_operation_call(args)
                .and_then(|call| self.runner.start(&self.engine, call))
                .map(|id| text_result(json!({"operation_id": id.as_str()}).to_string(), false)),
            _ => self.operation(&name, args),
        };
        Ok(result.unwrap_or_else(tool_error))
    }

    fn operation(&self, name: &str, args: &str) -> Result<Value, Diagnostic> {
        let fields = Fields::read(args, &["operation_id"])
            .map_err(|_| diagnostic("input", "expected only operation_id"))?;
        let id = fields
            .string("operation_id")
            .map_err(|_| diagnostic("input", "operation_id must be a nonempty string"))?;
        if id.is_empty() || id.len() > 64 {
            return Err(diagnostic("input", "operation_id must contain 1 to 64 UTF-8 bytes"));
        }
        let id = OperationId::new(&id);
        match name {
            "operation_events" => {
                let mut events = Vec::new();
                // next() never waits for a producer. The v2 runner has at most two
                // events, so this drain is bounded even if both arrive mid-call.
                while let Some(event) = self.runner.next(&id)? {
                    events.push(event);
                }
                events_result(&events)
            }
            "operation_cancel" => {
                self.runner.cancel(&id)?;
                Ok(text_result(
                    json!({"operation_id": id.as_str(), "cancel_requested": true}).to_string(),
                    false,
                ))
            }
            _ => {
                self.runner.release(&id)?;
                Ok(text_result(json!({"operation_id": id.as_str(), "released": true}).to_string(), false))
            }
        }
    }
}

fn events_result(events: &[MeshEvent]) -> Result<Value, Diagnostic> {
    let is_error =
        events.iter().any(|event| matches!(event.body, MeshEventBody::Failed(_) | MeshEventBody::Cancelled));
    let encoded = events.iter().map(encode_event).collect::<Result<Vec<_>, _>>()?;
    Ok(text_result(format!(r#"{{"events":[{}]}}"#, encoded.join(",")), is_error))
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn tool_error(error: Diagnostic) -> Value {
    text_result(
        json!({"diagnostic": {"code": error.code.as_str(), "message": error.message}}).to_string(),
        true,
    )
}

fn tools() -> Value {
    let handle = json!({"type": "object", "additionalProperties": false,
        "required": ["operation_id"], "properties": {
            "operation_id": {"type": "string", "minLength": 1, "maxLength": 64}}});
    let tool = |name, description, schema| {
        json!({"name": name, "description": description,
        "inputSchema": schema, "execution": {"taskSupport": "forbidden"},
        "annotations": {"readOnlyHint": true, "openWorldHint": false}})
    };
    json!({"tools": [
        tool("mesh", "Start a mesh operation immediately. Text JSON returns operation_id. Arguments are the mesh v2 call; request_id is independent of the MCP request ID.", descriptor().call_schema.clone()),
        tool("operation_events", "Drain currently available events without waiting. Text JSON is {events:[canonical v2 events]}; empty means none available. Failed/cancelled events set isError.", handle.clone()),
        tool("operation_cancel", "Request cooperative cancellation; repeated cancel is harmless. Text JSON acknowledges cancel_requested; inspect events for the winning terminal outcome.", handle.clone()),
        tool("operation_release", "Release a finished operation and discard unread events. Returns a tool error while its worker is still exiting; retry later. Text JSON acknowledges released.", handle),
    ]})
}

#[cfg(test)]
mod tests;
