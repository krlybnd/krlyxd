//! Strict JSON at the domain boundary. Raw primitives deliberately have no serde dependency.

use crate::{diagnostic, ApiVersion, Diagnostic, Exactness, Length, LengthUnit, MeshCall, MeshInput};
use crate::{MeshResult, RequestId, MAX_SAFE_INTEGER};
use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt;

// Preserve number tokens until their expected type is known. Parsing through Value first
// would lose duplicate keys and could round overflowing integer tokens into floats.
struct Object(BTreeMap<String, Box<RawValue>>);

impl<'de> Deserialize<'de> for Object {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Object;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object with unique field names")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Object, M::Error> {
                let mut fields = BTreeMap::new();
                while let Some((name, value)) = map.next_entry::<String, Box<RawValue>>()? {
                    if fields.insert(name.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!("duplicate field `{name}`")));
                    }
                }
                Ok(Object(fields))
            }
        }
        deserializer.deserialize_map(Fields)
    }
}

impl Object {
    fn read(text: &str, allowed: &[&str]) -> Result<Self, Diagnostic> {
        let object: Self = parse(text)?;
        if let Some(name) = object.0.keys().find(|name| !allowed.contains(&name.as_str())) {
            return Err(diagnostic("input", format!("unknown field `{name}`")));
        }
        Ok(object)
    }

    fn required(&mut self, name: &str) -> Result<Box<RawValue>, Diagnostic> {
        self.0.remove(name).ok_or_else(|| diagnostic("input", format!("missing field `{name}`")))
    }
}

fn parse<T: DeserializeOwned>(text: &str) -> Result<T, Diagnostic> {
    serde_json::from_str(text).map_err(|error| diagnostic("input", error.to_string()))
}

fn version(text: &str) -> Result<ApiVersion, Diagnostic> {
    let mut fields = Object::read(text, &["major", "minor"])?;
    Ok(ApiVersion {
        major: parse(fields.required("major")?.get())?,
        minor: parse(fields.required("minor")?.get())?,
    })
}

fn length(text: &str) -> Result<Length, Diagnostic> {
    let mut fields = Object::read(text, &["value", "unit"])?;
    let value = fields.required("value")?;
    // Even a quantity written as an integer must not silently lose integer precision.
    if !value.get().contains(['.', 'e', 'E']) {
        let integer: i64 = parse(value.get())?;
        if integer.unsigned_abs() > MAX_SAFE_INTEGER {
            return Err(diagnostic("input", "length exceeds the JSON-safe integer range"));
        }
    }
    let unit: String = parse(fields.required("unit")?.get())?;
    let unit = match unit.as_str() {
        "model" => LengthUnit::Model,
        "metre" => LengthUnit::Metre,
        _ => return Err(diagnostic("input", format!("unknown length unit `{unit}`"))),
    };
    Ok(Length { value: parse(value.get())?, unit })
}

fn input(text: &str) -> Result<MeshInput, Diagnostic> {
    let mut fields = Object::read(text, &["node", "tolerance", "triangles"])?;
    let node: String = parse(fields.required("node")?.get())?;
    let mut input = MeshInput::new(node);
    if let Some(tolerance) = fields.0.remove("tolerance") {
        input.tolerance = length(tolerance.get())?;
    }
    if let Some(triangles) = fields.0.remove("triangles") {
        input.triangles = parse(triangles.get())?;
    }
    Ok(input)
}

pub fn decode_call(text: &str) -> Result<MeshCall, Diagnostic> {
    let call = decode_call_fields(text)?;
    call.validate()?;
    Ok(call)
}

/// Decode the shared strict envelope before applying a version's call policy.
pub(crate) fn decode_call_fields(text: &str) -> Result<MeshCall, Diagnostic> {
    let mut fields = Object::read(text, &["request_id", "protocol", "api", "timeout_ms", "input"])?;
    let request_id: String = parse(fields.required("request_id")?.get())?;
    let protocol = version(fields.required("protocol")?.get())?;
    let api = version(fields.required("api")?.get())?;
    let timeout_ms = fields.0.remove("timeout_ms").map(|value| parse(value.get())).transpose()?.flatten();
    Ok(MeshCall {
        request_id: RequestId::new(&request_id),
        protocol,
        api,
        timeout_ms,
        input: input(fields.required("input")?.get())?,
    })
}

pub(crate) fn version_value(version: ApiVersion) -> Value {
    json!({"major": version.major, "minor": version.minor})
}

fn length_value(length: Length) -> Result<Value, Diagnostic> {
    finite(length.value)?;
    let unit = match length.unit {
        LengthUnit::Model => "model",
        LengthUnit::Metre => "metre",
    };
    Ok(json!({"value": length.value, "unit": unit}))
}

pub(crate) fn input_value(input: &MeshInput) -> Result<Value, Diagnostic> {
    input.validate()?;
    Ok(json!({
        "node": input.node, "tolerance": length_value(input.tolerance)?, "triangles": input.triangles,
    }))
}

pub fn encode_call(call: &MeshCall) -> Result<String, Diagnostic> {
    call.validate()?;
    Ok(json!({
        "request_id": call.request_id.as_str(), "protocol": version_value(call.protocol),
        "api": version_value(call.api), "timeout_ms": call.timeout_ms, "input": input_value(&call.input)?,
    })
    .to_string())
}

fn finite(value: f64) -> Result<(), Diagnostic> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(diagnostic("input", "mesh JSON requires finite numbers"))
    }
}

fn counter(value: u64) -> Result<(), Diagnostic> {
    if value <= MAX_SAFE_INTEGER {
        Ok(())
    } else {
        Err(diagnostic("resource_limit", "mesh counter exceeds the JSON-safe integer range"))
    }
}

fn exactness_value(exactness: Exactness) -> Result<Value, Diagnostic> {
    Ok(match exactness {
        Exactness::Exact => json!({"kind": "exact"}),
        Exactness::Numeric { tolerance } => {
            finite(tolerance)?;
            json!({"kind": "numeric", "tolerance": tolerance})
        }
        Exactness::Sampled { samples } => json!({"kind": "sampled", "samples": samples}),
    })
}

pub(crate) fn provenance_value(provenance: &crate::Provenance) -> Result<Value, Diagnostic> {
    Ok(json!({
        "package_version": provenance.package_version,
        "engine_revision": provenance.engine_revision.as_str(),
        "source": provenance.source.as_str(), "input": input_value(&provenance.input)?,
    }))
}

/// Encode integers without the legacy all-f64 Value and reject nonfinite payloads.
pub fn encode_result(result: &MeshResult) -> Result<String, Diagnostic> {
    for count in [result.points, result.triangles, result.open] {
        counter(count)?;
    }
    finite(result.volume)?;
    finite(result.area)?;
    if let Some(confidence) = result.quality.confidence {
        finite(confidence)?;
        if krlyxd::Confidence::new(confidence).is_err() {
            return Err(diagnostic("input", "mesh confidence must be between zero and one"));
        }
    }
    if let Some(uncertainty) = result.quality.positional_uncertainty {
        finite(uncertainty.value)?;
        if uncertainty.unit != LengthUnit::Model
            || krlyxd::PositionalUncertainty::measured(uncertainty).is_err()
        {
            return Err(diagnostic("input", "mesh uncertainty must be nonnegative in model units"));
        }
    }
    let uncertainty = result.quality.positional_uncertainty.map(length_value).transpose()?;
    let mesh = result
        .mesh
        .as_ref()
        .map(|artifact| {
            for value in artifact.data.points.iter().flatten() {
                finite(*value)?;
            }
            Ok::<_, Diagnostic>(json!({
                "id": artifact.id.as_str(), "revision": artifact.revision,
                "data": {"points": artifact.data.points, "triangles": artifact.data.triangles},
            }))
        })
        .transpose()?;
    Ok(json!({
        "points": result.points, "triangles": result.triangles, "volume": result.volume,
        "area": result.area, "open": result.open,
        "quality": {"exactness": exactness_value(result.quality.exactness)?,
            "positional_uncertainty": uncertainty, "confidence": result.quality.confidence},
        "provenance": provenance_value(&result.provenance)?,
        "mesh": mesh,
    })
    .to_string())
}
