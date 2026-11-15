//! Read-only MCP with compatible synchronous rendering and bounded view operations.
//! Operation tools keep input responsive while computing; output writes may block.

use krly_mcp_json as json;
mod transport;

pub use transport::{serve, serve_stdio, MAX_MESSAGE_BYTES};

use crate::json::{Failure, Fields};
use krly3d_view_api::operation::{self, OperationId};
use krly3d_view_api::wire::{decode_call, descriptor, encode_result};
use krly3d_view_api::{diagnostic, Diagnostic, RenderResult};
use krly3d_view_renderer::{runner::ViewRunner, ViewEngine};
use serde::Deserialize;
use serde_json::{json, Value};

/// The sole supported MCP revision. Other proposals negotiate this revision.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(PartialEq)]
enum Phase {
    New,
    Initializing,
    Ready,
}

/// One immutable model owner and a bounded background operation runner.
pub struct Server {
    engine: ViewEngine,
    phase: Phase,
    runner: ViewRunner,
}

impl Server {
    pub fn new(engine: ViewEngine) -> Self {
        Self { engine, phase: Phase::New, runner: ViewRunner::standard() }
    }

    /// Process one complete frame. Notifications never render or get replies.
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
        // We issue no client requests. Never answer a response with a response.
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
            "serverInfo": {"name": "krly3d-view-mcp", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "view_render accepts the complete view v1 RenderCall envelope as arguments and renders one node synchronously. Text content is canonical RenderResult JSON; image content is its PNG. Rendering blocks this connection, including ping and notifications. Use view_start, view_next, view_cancel and view_release for protocol/API 2.0 background operations with deadlines. EOF cancels retained operations. No progress, partial images, or MCP request-cancellation notification support."}))
    }

    fn operation(&self, name: &str, args: &str) -> Result<Value, Diagnostic> {
        let value = if name == "view_start" {
            let id = self.runner.start(&self.engine, operation::decode_operation_call(args)?)?;
            json!({"operation_id":id.as_str()})
        } else {
            let fields = Fields::read(args, &["operation_id"])
                .map_err(|_| diagnostic("input", "expected only operation_id"))?;
            let id = fields
                .string("operation_id")
                .map_err(|_| diagnostic("input", "operation_id must be a string"))?;
            let id = OperationId::new(&id);
            match name {
                "view_next" => {
                    let event = self.runner.next(&id)?;
                    // Keep canonical event tokens intact in the text response.
                    let text = match event {
                        Some(event) => operation::encode_event(&event)?,
                        None => "null".to_owned(),
                    };
                    return Ok(json!({"content":[{"type":"text","text":text}],"isError":false}));
                }
                "view_cancel" => {
                    self.runner.cancel(&id)?;
                    json!({"cancel_requested":true})
                }
                "view_release" => {
                    self.runner.release(&id)?;
                    json!({"released":true})
                }
                _ => return Err(diagnostic("input", "unknown view operation tool")),
            }
        };
        Ok(json!({"content":[{"type":"text","text":value.to_string()}],"isError":false}))
    }

    fn call(&self, params: Option<&str>) -> Result<Value, Failure> {
        let fields = Fields::read(params.unwrap_or("{}"), &["name", "arguments", "_meta"])?;
        let name = fields.string("name")?;
        if name != "view_render" {
            if matches!(name.as_str(), "view_start" | "view_next" | "view_cancel" | "view_release") {
                let args = fields.0.get("arguments").map_or("{}", |raw| raw.get());
                return Ok(self.operation(&name, args).unwrap_or_else(tool_error));
            }
            return Err(Failure::params(format!("Unknown tool: {name}")));
        }
        // Borrow the original JSON, preserving nested keys and numeric tokens all
        // the way to the domain codec. No serde_json::Value argument roundtrip.
        let args = fields.0.get("arguments").map_or("{}", |raw| raw.get());
        if !args.starts_with('{') {
            return Err(Failure::params("arguments must be an object"));
        }
        let result = decode_call(args)
            .and_then(|call| self.engine.call(call))
            .and_then(|result| render_result(&result));
        Ok(result.unwrap_or_else(tool_error))
    }
}

fn tools() -> Value {
    let mut tools = json!({"tools": [{
        "name": "view_render",
        "description": "Render one node synchronously. Arguments are the full view v1 RenderCall envelope, including protocol, api, request_id, and input. Returns canonical result JSON text and a PNG image. Rendering blocks the request loop.",
        "inputSchema": descriptor()["call_schema"],
        "annotations": {"readOnlyHint": true, "openWorldHint": false}
    }]});
    let id_schema = json!({"type":"object","additionalProperties":false,"required":["operation_id"],
        "properties":{"operation_id":{"type":"string","minLength":1}}});
    if let Some(list) = tools["tools"].as_array_mut() {
        for (name, description, schema) in [
            ("view_start", "Start a bounded view protocol/API 2.0 operation; timeout_ms starts at submission. Returns operation_id.", operation::descriptor()["call_schema"].clone()),
            ("view_next", "Drain one canonical event without waiting; null means no queued event. Started then exactly one terminal. Completed contains result PNG as base64.", id_schema.clone()),
            ("view_cancel", "Request cooperative cancellation; the first terminal outcome wins.", id_schema.clone()),
            ("view_release", "Forget a terminal operation after worker exit; frees request_id for reuse. Retry if still computing.", id_schema),
        ] {
            list.push(json!({"name":name,"description":description,"inputSchema":schema,
                "annotations":{"readOnlyHint":true,"openWorldHint":false}}));
        }
    }
    tools
}

#[derive(Deserialize)]
struct EncodedResult<'a> {
    #[serde(borrow)]
    image: EncodedImage<'a>,
}

#[derive(Deserialize)]
struct EncodedImage<'a> {
    mime_type: &'a str,
    encoding: &'a str,
    data: &'a str,
}

fn render_result(result: &RenderResult) -> Result<Value, Diagnostic> {
    let text = encode_result(result)?;
    // Only borrow the encoded PNG; leave the canonical result text untouched.
    let encoded: EncodedResult<'_> = serde_json::from_str(&text)
        .map_err(|_| diagnostic("internal", "encoded render result has no PNG image"))?;
    if encoded.image.mime_type != "image/png" || encoded.image.encoding != "base64" {
        return Err(diagnostic("internal", "encoded render image must be base64 PNG"));
    }
    Ok(json!({"content": [
        {"type": "text", "text": text},
        {"type": "image", "mimeType": encoded.image.mime_type, "data": encoded.image.data}
    ], "isError": false}))
}

fn tool_error(error: Diagnostic) -> Value {
    json!({"content": [{"type": "text", "text":
        json!({"diagnostic": {"code": error.code.as_str(), "message": error.message}}).to_string()
    }], "isError": true})
}

#[cfg(test)]
mod tests;
