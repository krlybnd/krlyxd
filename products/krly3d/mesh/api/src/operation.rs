//! Explicit version 2.0 contract for cancellable mesh operations with deadlines.
//!
//! This module describes calls and events; execution belongs to an engine. A zero
//! timeout requests an immediate deadline. Version 1.0 remains synchronous.

use crate::codec::{decode_call_fields, encode_result, input_value, provenance_value, version_value};
use crate::{diagnostic, ApiVersion, Descriptor, Diagnostic, MeshInput, MeshResult, Provenance};
use crate::{RequestId, MAX_SAFE_INTEGER};
pub use krlyxd::OperationId;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::value::RawValue;
use serde_json::{json, Value};
use std::sync::OnceLock;

pub const PROTOCOL_VERSION: ApiVersion = ApiVersion { major: 2, minor: 0 };
pub const API_VERSION: ApiVersion = ApiVersion { major: 2, minor: 0 };

#[derive(Clone, Debug, PartialEq)]
pub struct OperationCall {
    pub request_id: RequestId,
    pub protocol: ApiVersion,
    pub api: ApiVersion,
    /// Milliseconds from submission; zero requests an immediate deadline.
    pub timeout_ms: Option<u64>,
    pub input: MeshInput,
}

impl OperationCall {
    pub fn new(request_id: RequestId, input: MeshInput) -> Self {
        Self { request_id, protocol: PROTOCOL_VERSION, api: API_VERSION, timeout_ms: None, input }
    }

    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.protocol != PROTOCOL_VERSION || self.api != API_VERSION {
            return Err(diagnostic(
                "unsupported_version",
                "mesh operations require protocol 2.0 and mesh API 2.0",
            ));
        }
        if self.timeout_ms.is_some_and(|timeout| timeout > MAX_SAFE_INTEGER) {
            return Err(diagnostic("input", "timeout_ms exceeds the JSON-safe integer range"));
        }
        nonempty(self.request_id.as_str(), "request_id")?;
        self.input.validate()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshEvent {
    pub request_id: RequestId,
    pub operation_id: OperationId,
    pub seq: u64,
    pub body: MeshEventBody,
}

/// The complete event vocabulary. Only `Started` is nonterminal.
#[derive(Clone, Debug, PartialEq)]
pub enum MeshEventBody {
    Started(Provenance),
    Completed(MeshResult),
    Failed(Diagnostic),
    Cancelled,
}

/// Reuse the v1 raw-field parser, then apply the explicit v2 policy.
pub fn decode_operation_call(text: &str) -> Result<OperationCall, Diagnostic> {
    let fields = decode_call_fields(text)?;
    let call = OperationCall {
        request_id: fields.request_id,
        protocol: fields.protocol,
        api: fields.api,
        timeout_ms: fields.timeout_ms,
        input: fields.input,
    };
    call.validate()?;
    Ok(call)
}

pub fn encode_operation_call(call: &OperationCall) -> Result<String, Diagnostic> {
    call.validate()?;
    Ok(json!({
        "request_id": call.request_id.as_str(), "protocol": version_value(call.protocol),
        "api": version_value(call.api), "timeout_ms": call.timeout_ms, "input": input_value(&call.input)?,
    })
    .to_string())
}

fn nonempty(value: &str, name: &str) -> Result<(), Diagnostic> {
    if value.is_empty() {
        return Err(diagnostic("input", format!("mesh operations need a nonempty {name}")));
    }
    Ok(())
}

fn validate_provenance(provenance: &Provenance) -> Result<(), Diagnostic> {
    nonempty(&provenance.package_version, "package_version")?;
    nonempty(provenance.engine_revision.as_str(), "engine_revision")?;
    nonempty(provenance.source.as_str(), "source")?;
    provenance.input.validate()
}

/// Canonical event JSON, suitable for enforcing an encoded event-size limit.
/// Invalid results fail through the same finite-number/count checks as v1.
pub fn encode_event(event: &MeshEvent) -> Result<String, Diagnostic> {
    nonempty(event.request_id.as_str(), "request_id")?;
    nonempty(event.operation_id.as_str(), "operation_id")?;
    if event.seq > MAX_SAFE_INTEGER {
        return Err(diagnostic("resource_limit", "mesh event seq exceeds the JSON-safe integer range"));
    }
    let body = match &event.body {
        MeshEventBody::Started(provenance) => {
            validate_provenance(provenance)?;
            json!({"kind": "started", "provenance": provenance_value(provenance)?}).to_string()
        }
        MeshEventBody::Completed(result) => {
            validate_provenance(&result.provenance)?;
            if let Some(mesh) = &result.mesh {
                nonempty(mesh.id.as_str(), "artifact id")?;
            }
            // Preserve the codec's numeric tokens; a Value reparse can round f64s.
            format!(r#"{{"kind":"completed","result":{}}}"#, encode_result(result)?)
        }
        MeshEventBody::Failed(error) => {
            nonempty(error.code.as_str(), "diagnostic code")?;
            json!({"kind": "failed", "diagnostic": {"code": error.code.as_str(), "message": error.message}})
                .to_string()
        }
        MeshEventBody::Cancelled => json!({"kind": "cancelled"}).to_string(),
    };
    let body = RawValue::from_string(body).map_err(|error| diagnostic("input", error.to_string()))?;
    serde_json::to_string(&EventEnvelope { event, body: &body })
        .map_err(|error| diagnostic("input", error.to_string()))
}

struct EventEnvelope<'a> {
    event: &'a MeshEvent,
    body: &'a RawValue,
}

impl Serialize for EventEnvelope<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Match the canonical lexical key order used by the shared Value encoders.
        let mut map = serializer.serialize_map(Some(6))?;
        map.serialize_entry("api", &version_value(API_VERSION))?;
        map.serialize_entry("body", self.body)?;
        map.serialize_entry("operation_id", self.event.operation_id.as_str())?;
        map.serialize_entry("protocol", &version_value(PROTOCOL_VERSION))?;
        map.serialize_entry("request_id", self.event.request_id.as_str())?;
        map.serialize_entry("seq", &self.event.seq)?;
        map.end()
    }
}

/// Metadata/schema for v2 operations, independent of the synchronous descriptor.
pub fn descriptor() -> &'static Descriptor {
    static DESCRIPTOR: OnceLock<Descriptor> = OnceLock::new();
    DESCRIPTOR.get_or_init(build_descriptor)
}

fn build_descriptor() -> Descriptor {
    let mut descriptor = crate::descriptor().clone();
    descriptor.protocol = PROTOCOL_VERSION;
    descriptor.api = API_VERSION;
    descriptor.synchronous = false;
    descriptor.cancellation = true;
    descriptor.timeouts = true;
    descriptor.supported_events = &["started", "completed", "failed", "cancelled"];
    for field in ["protocol", "api"] {
        descriptor.call_schema["properties"][field]["properties"]["major"]["const"] = json!(2);
    }
    descriptor.call_schema["properties"]["timeout_ms"] = json!({
        "type": ["null", "integer"], "minimum": 0, "maximum": MAX_SAFE_INTEGER, "default": null,
        "description": "milliseconds from submission; zero requests an immediate deadline",
    });
    let mut result_schema = descriptor.output_schema.clone();
    for field in ["package_version", "engine_revision", "source"] {
        result_schema["properties"]["provenance"]["properties"][field]["minLength"] = json!(1);
    }
    result_schema["properties"]["mesh"]["anyOf"][1]["properties"]["id"]["minLength"] = json!(1);
    descriptor.output_schema = event_schema(result_schema);
    descriptor
}

fn event_schema(result_schema: Value) -> Value {
    let id = json!({"type": "string", "minLength": 1});
    let version = json!({"type": "object", "additionalProperties": false,
    "required": ["major", "minor"], "properties": {
        "major": {"type": "integer", "const": 2}, "minor": {"type": "integer", "const": 0},
    }});
    let diagnostic = json!({"type": "object", "additionalProperties": false,
    "required": ["code", "message"], "properties": {
        "code": id, "message": {"type": "string"},
    }});
    let provenance = result_schema["properties"]["provenance"].clone();
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object", "additionalProperties": false,
        "required": ["protocol", "api", "request_id", "operation_id", "seq", "body"],
        "properties": {
            "protocol": version, "api": version, "request_id": id, "operation_id": id,
            "seq": {"type": "integer", "minimum": 0, "maximum": MAX_SAFE_INTEGER},
            "body": {"oneOf": [
                {"type": "object", "additionalProperties": false,
                    "required": ["kind", "provenance"], "properties": {
                        "kind": {"const": "started"}, "provenance": provenance,
                    }},
                {"type": "object", "additionalProperties": false,
                    "required": ["kind", "result"], "properties": {
                        "kind": {"const": "completed"}, "result": result_schema,
                    }},
                {"type": "object", "additionalProperties": false,
                    "required": ["kind", "diagnostic"], "properties": {
                        "kind": {"const": "failed"}, "diagnostic": diagnostic,
                    }},
                {"type": "object", "additionalProperties": false,
                    "required": ["kind"], "properties": {"kind": {"const": "cancelled"}}},
            ]},
        },
    })
}
