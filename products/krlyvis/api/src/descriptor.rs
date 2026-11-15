use crate::{metadata, ENGINE_REVISION, LOCAL_NODE, MAX_BASE64_BYTES, MAX_CALL_BYTES, MAX_IMAGE_BYTES};
use crate::{MAX_IMAGE_PIXELS, MAX_JPEG_SIDE, MAX_PIXEL_UNIT, MAX_SIMPLIFY_PIXELS, MAX_VERTICES, PROTOCOL};
use serde_json::{json, Value};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "additionalProperties": false, "properties": properties, "required": required})
}

fn scale() -> Value {
    json!({"type": "number", "exclusiveMinimum": 0, "maximum": MAX_PIXEL_UNIT, "description": "Finite model units per pixel; no physical calibration"})
}

fn simplify() -> Value {
    json!({"type": "number", "minimum": 0, "maximum": MAX_SIMPLIFY_PIXELS, "default": 0,
        "description": "Requested maximum contour distance in source pixels, before pixel_unit scaling. Zero preserves the exact trace; unsafe simplification may retain exact loops. Positional and physical accuracy remain unknown."})
}

fn seed() -> Value {
    json!({"anyOf": [{"type": "null"}, {"type": "array", "minItems": 2, "maxItems": 2,
        "items": {"type": "number", "minimum": 0},
        "description": "Finite pixel coordinates, x right and y down; fractions floored; must lie on the subject"}]})
}

fn source() -> Value {
    json!({"type": "string", "minLength": 1, "pattern": "\\S", "description": "Nonblank caller provenance claim; unverified"})
}

fn image() -> Value {
    object(
        json!({
            "format": {"enum": ["png", "jpeg", "pnm"]},
            "data": {"type": "string", "contentEncoding": "base64", "maxLength": MAX_BASE64_BYTES,
                "pattern": "^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/][AQgw]==|[A-Za-z0-9+/]{2}[AEIMQUYcgkosw048]=)?$",
                "x-max-decoded-bytes": MAX_IMAGE_BYTES,
                "description": "Canonical padded standard base64; zero unused trailing bits; decoded image file at most 16 MiB"},
        }),
        &["format", "data"],
    )
}

fn input() -> Value {
    object(
        json!({"image": image(), "pixel_unit": scale(), "simplify": simplify(), "seed": seed(), "source": source()}),
        &["image", "pixel_unit", "seed", "source"],
    )
}

fn polygons() -> Value {
    json!({"type": "array", "minItems": 1, "maxItems": MAX_VERTICES / 3,
        "x-max-total-vertices": MAX_VERTICES,
        "description": "Outer loop first, then holes; closed implicitly; at most 256 vertices across all loops",
        "items": {"type": "array", "minItems": 3, "maxItems": MAX_VERTICES,
            "items": {"type": "array", "minItems": 2, "maxItems": 2, "items": {"type": "number"}}}})
}

fn evidence() -> Value {
    let raster = object(
        json!({
            "kind": {"const": "raster"}, "source": {"const": "krlyvis.placed-pixel-outline/1"},
            "uncertainty": {"const": "unknown"}, "pixel_unit": scale(),
            "threshold": {"type": "number", "minimum": 0, "maximum": 255.5},
            "coordinates": {"const": "pixel-derived polyline; simplification and image measurement error unknown"},
        }),
        &["kind", "source", "uncertainty", "pixel_unit", "threshold", "coordinates"],
    );
    object(
        json!({
            "kind": {"const": "placed"}, "view": {"const": "top"},
            "looking": {"const": [0, 0, -1]}, "axes": {"const": ["x", "y"]}, "unit": scale(),
            "material_area": {"type": "number", "minimum": 0}, "evidence": raster,
            "polygons": polygons(), "pixels": polygons(),
            "node": {"const": LOCAL_NODE, "description": "Local result label only; not a global live handle; no document is retained"},
        }),
        &[
            "kind",
            "view",
            "looking",
            "axes",
            "unit",
            "material_area",
            "evidence",
            "polygons",
            "pixels",
            "node",
        ],
    )
}

fn output() -> Value {
    let text = json!({"type": "string", "minLength": 1, "pattern": "\\S"});
    let provenance = object(
        json!({
            "image_sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$",
                "description": "Actual decoded base64 image-file bytes, not re-encoded or decompressed pixels"},
            "source": source(), "image_format": {"enum": ["png", "jpeg", "pnm"]},
            "image_size": {"type": "array", "minItems": 2, "maxItems": 2,
                "items": {"type": "integer", "minimum": 1, "maximum": MAX_IMAGE_PIXELS},
                "x-max-pixels": MAX_IMAGE_PIXELS},
            "pixel_unit": scale(), "seed": seed(), "method": {"const": "trace-holes"},
            "simplify": simplify(), "engine_revision": {"const": ENGINE_REVISION},
        }),
        &[
            "image_sha256",
            "source",
            "image_format",
            "image_size",
            "pixel_unit",
            "seed",
            "method",
            "simplify",
            "engine_revision",
        ],
    );
    let quality = object(
        json!({
            "confidence": {"type": "null"}, "positional_uncertainty": {"type": "null"},
            "assumptions": {"type": "array", "items": text}, "accuracy_scope": text, "source_claim": text,
        }),
        &["confidence", "positional_uncertainty", "assumptions", "accuracy_scope", "source_claim"],
    );
    let milliseconds = json!({"type": "number", "minimum": 0});
    let timing = object(
        json!({"read_ms": milliseconds, "outline_ms": milliseconds, "total_ms": milliseconds}),
        &["read_ms", "outline_ms", "total_ms"],
    );
    object(
        json!({
            "protocol": {"const": PROTOCOL}, "request_id": {"type": "string"}, "evidence": evidence(),
            "provenance": provenance, "quality": quality, "timing": timing,
        }),
        &["protocol", "request_id", "evidence", "provenance", "quality", "timing"],
    )
}

/// JSON Schema supplemented by finite-number, aggregate-size and provider checks.
pub fn descriptor() -> Value {
    let metadata = metadata();
    json!({
        "package": metadata.package.name, "package_version": metadata.package.version,
        "protocol": PROTOCOL, "api": {"major": metadata.api.major, "minor": metadata.api.minor},
        "engine_revision": metadata.engine_revision, "operation": "read_drawing",
        "synchronous": true, "stateless": true, "length_unit": "model",
        "coordinates": "Top view along -Z; pixel (0,0) maps to model (0,0); x and y retain image signs and scale uniformly by pixel_unit",
        "node_semantics": "top@1 is a local result label, not a global live handle; no native document is retained",
        "source_semantics": "source is an unverified caller claim; image_sha256 hashes actual supplied image bytes",
        "max_call_bytes": MAX_CALL_BYTES, "max_image_bytes": MAX_IMAGE_BYTES,
        "max_image_pixels": MAX_IMAGE_PIXELS, "max_jpeg_side": MAX_JPEG_SIDE, "max_vertices": MAX_VERTICES,
        "validation": "All existing keys are required; input.simplify is optional and defaults to zero; duplicate and unknown keys rejected at all object levels; finite numbers only; image provider resource limits apply before sample allocation",
        "call_schema": object(json!({"protocol": {"const": PROTOCOL}, "request_id": {"type": "string"}, "input": input()}),
            &["protocol", "request_id", "input"]),
        "input_schema": input(), "output_schema": output(),
    })
}
