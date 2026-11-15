use super::{
    metadata, MAX_ABS_COORDINATE, MAX_CALL_BYTES, MAX_HEIGHT, MAX_REQUEST_ID_BYTES, MAX_SOURCE_BYTES,
    MAX_VERTICES, PROTOCOL,
};
use serde_json::{json, Value};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "additionalProperties": false, "properties": properties, "required": required})
}

fn text(maximum: usize) -> Value {
    json!({"type": "string", "minLength": 1, "maxLength": maximum,
        "description": format!("Nonblank; at most {maximum} UTF-8 bytes"), "x-max-utf8-bytes": maximum})
}

fn input() -> Value {
    object(
        json!({
            "polygons": {"type": "array", "minItems": 1, "maxItems": MAX_VERTICES / 3,
                "description": "First loop outer; remaining loops disjoint interior holes; winding independent; at most 256 total supplied vertices, including any repeated closing vertices",
                "x-max-total-vertices": MAX_VERTICES,
                "items": {"type": "array", "minItems": 3, "maxItems": MAX_VERTICES,
                    "items": {"type": "array", "minItems": 2, "maxItems": 2,
                        "items": {"type": "number", "minimum": -MAX_ABS_COORDINATE,
                            "maximum": MAX_ABS_COORDINATE, "description": "finite model coordinate"}}}},
            "height": {"type": "number", "exclusiveMinimum": 0, "maximum": MAX_HEIGHT, "description": "finite extrusion along +Z"},
            "length_unit": {"const": "model"},
            "source": text(MAX_SOURCE_BYTES),
        }),
        &["polygons", "height", "length_unit", "source"],
    )
}

fn output() -> Value {
    let string = json!({"type": "string", "minLength": 1});
    let exactness = json!({"oneOf": [
        object(json!({"kind": {"const": "exact"}}), &["kind"]),
        object(json!({"kind": {"const": "numeric"}, "tolerance": {"type": "number", "exclusiveMinimum": 0}}), &["kind", "tolerance"]),
        object(json!({"kind": {"const": "sampled"}, "samples": {"type": "integer", "minimum": 1, "maximum": u32::MAX}}), &["kind", "samples"]),
    ]});
    object(
        json!({
            "protocol": {"const": PROTOCOL}, "request_id": text(MAX_REQUEST_ID_BYTES),
            "model": {"type": "string", "minLength": 1, "description": "Native k3d document"},
            "node": {"const": "part@1"}, "source": text(MAX_SOURCE_BYTES),
            "provenance": object(json!({"source": string, "session": string,
                "node": {"const": "part@1"}, "build_key": {"type": "string", "pattern": "^[0-9a-f]{32}$"}}),
                &["source", "session", "node", "build_key"]),
            "quality": object(json!({
                "exactness": exactness,
                "positional_uncertainty": {"anyOf": [{"type": "null"}, object(json!({
                    "value": {"type": "number", "minimum": 0}, "unit": {"const": "model"}}), &["value", "unit"])]},
                "confidence": {"type": ["null", "number"], "minimum": 0, "maximum": 1},
                "assumptions": {"type": "array", "items": string},
                "accuracy_scope": string, "source_claim": string,
            }), &["exactness", "positional_uncertainty", "confidence", "assumptions", "accuracy_scope", "source_claim"]),
        }),
        &["protocol", "request_id", "model", "node", "source", "provenance", "quality"],
    )
}

/// Metadata for exactly one bounded synchronous operation, with no retained jobs.
/// Semantic validation supplements JSON Schema for finite values and aggregate limits.
pub fn descriptor() -> Value {
    let metadata = metadata();
    json!({
        "package": metadata.package.name, "package_version": metadata.package.version,
        "protocol": PROTOCOL,
        "api": {"major": metadata.api.major, "minor": metadata.api.minor},
        "engine_revision": metadata.engine_revision,
        "operation": "build_planar", "synchronous": true, "stateless": true,
        "coordinates": "right-handed Z-up; XY contour at z=0; extrusion along +Z",
        "length_unit": "model", "artifact_format": "k3d 1",
        "max_vertices": MAX_VERTICES, "max_call_bytes": MAX_CALL_BYTES,
        "source_semantics": "Caller-supplied provenance; not a verified image hash",
        "call_schema": object(json!({"protocol": {"const": PROTOCOL},
            "request_id": text(MAX_REQUEST_ID_BYTES), "input": input()}), &["protocol", "request_id", "input"]),
        "input_schema": input(), "output_schema": output(),
    })
}
