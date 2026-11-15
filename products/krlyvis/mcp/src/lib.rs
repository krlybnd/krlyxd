//! MCP 2025-11-25 over bounded NDJSON. Every drawing call reads one embedded image.
//! Execution is synchronous: drawing reads and output block input, including ping.
//! There is no in-flight request cancellation, background worker, or file output.
//!
//! JSON-RPC lifecycle adapted from products/krly3d/brep/mcp/src/lib.rs.

use krly_mcp_json as json;
mod transport;

pub use transport::{serve, serve_stdio, MAX_MESSAGE_BYTES};

use crate::json::{Failure, Fields};
use krlyvis_api::{decode_call, descriptor, encode_result, Diagnostic};
use krlyvis_engine::read_drawing;
use serde_json::{json, Value};

/// The supported MCP revision; other proposals negotiate this revision.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(PartialEq)]
enum Phase {
    New,
    Initializing,
    Ready,
}

/// Connection lifecycle only. No image or request state is retained by tools.
pub struct Server {
    phase: Phase,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        Self { phase: Phase::New }
    }

    /// Process a complete UTF-8 frame. Notifications never read drawings or get replies.
    /// Use `serve` to enforce the byte bound and newline framing on a stream.
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
        // No client requests are issued. Answering unsolicited responses loops.
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
            return None;
        };
        Some(match self.dispatch(&method, params) {
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
        if fields.string("protocolVersion")?.is_empty() {
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
            "serverInfo": {"name": "krlyvis-drawing-mcp", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "vis_drawing accepts the complete krlyvis.drawing/1 request envelope as arguments. Each stateless call reads its embedded base64 image and returns canonical result JSON as text, including evidence, provenance, quality and timing. Execution is synchronous: drawing reads and output block all input, including ping and notifications. No in-flight request cancellation, timeouts, progress, operation handles, or MCP tasks. No files are read or written. The API limits decoded images to 16 MiB. Incoming NDJSON frames are limited to 24 MiB before the newline."}))
    }

    fn call(&self, params: Option<&str>) -> Result<Value, Failure> {
        let fields = Fields::read(params.unwrap_or("{}"), &["name", "arguments", "_meta"])?;
        let name = fields.string("name")?;
        if name != "vis_drawing" {
            return Err(Failure::params(format!("Unknown tool: {name}")));
        }
        // Forward original JSON, preserving numeric tokens and object keys until
        // the API decoder. Never roundtrip tool arguments through Value.
        let args = fields.0.get("arguments").map_or("{}", |raw| raw.get());
        if !args.starts_with('{') {
            return Err(Failure::params("arguments must be an object"));
        }
        let result = decode_call(args).and_then(read_drawing).and_then(|result| encode_result(&result));
        Ok(match result {
            Ok(text) => text_result(text, false),
            Err(error) => tool_error(error),
        })
    }
}

fn tools() -> Value {
    json!({"tools": [{
        "name": "vis_drawing",
        "description": "Read an embedded png, jpeg or pnm drawing synchronously. Arguments are the full krlyvis.drawing/1 envelope, including protocol, request_id and input. Returns canonical result JSON text with placed evidence, image hash and source provenance, quality and timing. Blocks input; no in-flight request cancellation. Reads and writes no files.",
        "inputSchema": descriptor()["call_schema"],
        "annotations": {"readOnlyHint": true, "openWorldHint": false}
    }]})
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

#[cfg(test)]
mod tests;
