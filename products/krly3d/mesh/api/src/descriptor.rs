use crate::{ApiVersion, Id, API_VERSION, ARTIFACT_FORMAT, ENGINE_REVISION, MAX_SAFE_INTEGER};
use crate::{DEFAULT_TOLERANCE, DEFAULT_TRIANGLES, PROTOCOL_VERSION};
use serde_json::{json, Value};
use std::sync::OnceLock;

/// The sole metadata/schema source for this mesh contract. Future adapters consume it.
#[derive(Clone, Debug, PartialEq)]
pub struct Descriptor {
    pub package: Id,
    pub package_version: &'static str,
    pub protocol: ApiVersion,
    pub api: ApiVersion,
    pub engine_revision: Id,
    pub operation: &'static str,
    pub synchronous: bool,
    pub read_only: bool,
    pub cancellation: bool,
    pub timeouts: bool,
    pub supported_events: &'static [&'static str],
    pub coordinates: &'static str,
    pub length_unit: &'static str,
    pub area_unit: &'static str,
    pub volume_unit: &'static str,
    pub artifact_format: &'static str,
    pub call_schema: Value,
    pub input_schema: Value,
    pub output_schema: Value,
}

impl Descriptor {
    /// Common metadata, validated by the shared raw contract without changing wire v1/v2.
    pub fn metadata(&self) -> krlyxd::ContractMetadata<'_> {
        krlyxd::ContractMetadata {
            package: krlyxd::PackageMetadata { name: self.package.as_str(), version: self.package_version },
            protocol: self.protocol,
            api: self.api,
            engine_revision: self.engine_revision.as_str(),
            coordinates: krlyxd::Coordinates::RIGHT_HANDED_Z_UP,
            length_unit: krlyxd::LengthUnit::Model,
        }
    }
}

pub fn descriptor() -> &'static Descriptor {
    static DESCRIPTOR: OnceLock<Descriptor> = OnceLock::new();
    DESCRIPTOR.get_or_init(build_descriptor)
}

fn build_descriptor() -> Descriptor {
    let length = json!({
        "type": "object", "additionalProperties": false, "required": ["value", "unit"],
        "properties": {"value": {"type": "number", "exclusiveMinimum": 0},
            "unit": {"const": "model"}},
    });
    let input = json!({
        "type": "object", "additionalProperties": false, "required": ["node"],
        "properties": {
            "node": {"type": "string", "minLength": 1, "description": "node or node@version"},
            "tolerance": {"allOf": [length.clone()],
                "default": {"value": DEFAULT_TOLERANCE, "unit": "model"},
                "description": "must exceed the engine's model length accuracy"},
            "triangles": {"type": "boolean", "default": DEFAULT_TRIANGLES},
        },
    });
    let version = json!({"type": "object", "additionalProperties": false,
        "required": ["major", "minor"],
        "properties": {"major": {"type": "integer", "const": 1},
            "minor": {"type": "integer", "const": 0}}});
    let call = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object", "additionalProperties": false,
        "required": ["request_id", "protocol", "api", "input"],
        "properties": {
            "request_id": {"type": "string", "minLength": 1}, "protocol": version, "api": version,
            "timeout_ms": {"type": "null", "default": null,
                "description": "timeouts are rejected until controlled execution exists"},
            "input": input.clone(),
        },
    });
    Descriptor {
        package: Id::new("krly3d-mesh"),
        package_version: env!("CARGO_PKG_VERSION"),
        protocol: PROTOCOL_VERSION,
        api: API_VERSION,
        engine_revision: Id::new(ENGINE_REVISION),
        operation: "mesh",
        synchronous: true,
        read_only: true,
        cancellation: false,
        timeouts: false,
        supported_events: &[],
        coordinates: "right-handed, Z-up world space",
        length_unit: "model",
        area_unit: "model^2",
        volume_unit: "model^3",
        artifact_format: ARTIFACT_FORMAT,
        call_schema: call,
        input_schema: input.clone(),
        output_schema: output_schema(input, length),
    }
}

fn output_schema(input: Value, length: Value) -> Value {
    let mut uncertainty = length;
    uncertainty["properties"]["value"] = json!({"type": "number", "minimum": 0});
    let count = json!({"type": "integer", "minimum": 0, "maximum": MAX_SAFE_INTEGER});
    let index = json!({"type": "integer", "minimum": 0, "maximum": u32::MAX});
    let triple = |items: Value| json!({"type": "array", "minItems": 3, "maxItems": 3, "items": items});
    let exactness = json!({"oneOf": [
        {"type": "object", "additionalProperties": false, "required": ["kind"],
            "properties": {"kind": {"const": "exact"}}},
        {"type": "object", "additionalProperties": false, "required": ["kind", "tolerance"],
            "properties": {"kind": {"const": "numeric"}, "tolerance": {"type": "number"}}},
        {"type": "object", "additionalProperties": false, "required": ["kind", "samples"],
            "properties": {"kind": {"const": "sampled"}, "samples": index}},
    ]});
    let artifact = json!({"type": "object", "additionalProperties": false,
    "required": ["id", "revision", "data"], "properties": {
        "id": {"type": "string"}, "revision": index,
        "data": {"type": "object", "additionalProperties": false,
            "required": ["points", "triangles"], "properties": {
                "points": {"type": "array", "items": triple(json!({"type": "number"}))},
                "triangles": {"type": "array", "items": triple(index.clone())},
            }},
    }});
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object", "additionalProperties": false,
        "required": ["points", "triangles", "volume", "area", "open", "quality", "provenance", "mesh"],
        "properties": {
            "points": count, "triangles": count, "open": count,
            "volume": {"type": "number", "description": "model units cubed"},
            "area": {"type": "number", "description": "model units squared"},
            "quality": {"type": "object", "additionalProperties": false,
                "required": ["exactness", "positional_uncertainty", "confidence"], "properties": {
                    "exactness": exactness,
                    "positional_uncertainty": {"anyOf": [{"type": "null"}, uncertainty]},
                    "confidence": {"type": ["null", "number"], "minimum": 0, "maximum": 1},
                }},
            "provenance": {"type": "object", "additionalProperties": false,
                "required": ["package_version", "engine_revision", "source", "input"], "properties": {
                    "package_version": {"type": "string"}, "engine_revision": {"type": "string"},
                    "source": {"type": "string"}, "input": input,
                }},
            "mesh": {"anyOf": [{"type": "null"}, artifact]},
        },
    })
}
