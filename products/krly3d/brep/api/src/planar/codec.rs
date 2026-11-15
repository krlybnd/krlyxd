use super::{bounded_text, diagnostic, PlanarCall, PlanarInput, PlanarResult};
use super::{MAX_CALL_BYTES, MAX_REQUEST_ID_BYTES, MAX_SOURCE_BYTES, PROTOCOL};
use crate::Diagnostic;
use krlyxd::{Exactness, LengthUnit};
use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{json, value::RawValue};
use std::{collections::BTreeMap, fmt};

// Raw values retain duplicate keys until the corresponding typed object is read.
// deserialize_map also excludes serde's positional struct/array representation.
struct Object(BTreeMap<String, Box<RawValue>>);

impl<'de> Deserialize<'de> for Object {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Object;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object with unique fields")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Object, M::Error> {
                let mut fields = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                    if fields.insert(key.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!("duplicate field `{key}`")));
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
        let fields: Self = parse(text)?;
        if let Some(key) = fields.0.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(diagnostic("input", format!("unknown field `{key}`")));
        }
        Ok(fields)
    }

    fn take<T: DeserializeOwned>(&mut self, name: &str) -> Result<T, Diagnostic> {
        let raw =
            self.0.remove(name).ok_or_else(|| diagnostic("input", format!("missing field `{name}`")))?;
        parse(raw.get())
    }
}

fn parse<T: DeserializeOwned>(text: &str) -> Result<T, Diagnostic> {
    serde_json::from_str(text).map_err(|error| diagnostic("input", error.to_string()))
}

pub fn decode_call(text: &str) -> Result<PlanarCall, Diagnostic> {
    if text.len() > MAX_CALL_BYTES {
        return Err(diagnostic("resource_limit", "planar JSON exceeds 1048576 UTF-8 bytes"));
    }
    let mut fields = Object::read(text, &["protocol", "request_id", "input"])?;
    let raw_input: Box<RawValue> = fields.take("input")?;
    let mut input = Object::read(raw_input.get(), &["polygons", "height", "length_unit", "source"])?;
    let call = PlanarCall {
        protocol: fields.take("protocol")?,
        request_id: fields.take("request_id")?,
        input: PlanarInput {
            polygons: input.take("polygons")?,
            height: input.take("height")?,
            length_unit: input.take("length_unit")?,
            source: input.take("source")?,
        },
    };
    call.validate()?;
    Ok(call)
}

/// Encode the native document and explicit quality/provenance without inventing
/// measurement uncertainty, physical calibration, confidence, or source verification.
pub fn encode_result(result: &PlanarResult) -> Result<String, Diagnostic> {
    if result.protocol != PROTOCOL || result.node != "part@1" || result.provenance.node != result.node {
        return Err(diagnostic("input", "invalid planar result protocol or node identity"));
    }
    bounded_text("request_id", &result.request_id, MAX_REQUEST_ID_BYTES)?;
    bounded_text("source", &result.source, MAX_SOURCE_BYTES)?;
    if result.model.is_empty() {
        return Err(diagnostic("input", "planar result must contain a native document"));
    }
    let quality = &result.quality;
    let exactness = match quality.exactness {
        Exactness::Exact => json!({"kind": "exact"}),
        Exactness::Numeric { tolerance } if tolerance.is_finite() && tolerance > 0.0 => {
            json!({"kind": "numeric", "tolerance": tolerance})
        }
        Exactness::Sampled { samples } if samples > 0 => json!({"kind": "sampled", "samples": samples}),
        _ => return Err(diagnostic("input", "invalid exactness tolerance")),
    };
    if quality.confidence.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
        return Err(diagnostic("input", "confidence must be finite and between zero and one"));
    }
    let uncertainty = quality
        .positional_uncertainty
        .map(|length| {
            if !length.value.is_finite() || length.value < 0.0 || length.unit != LengthUnit::Model {
                return Err(diagnostic(
                    "input",
                    "positional uncertainty must be finite and nonnegative in model units",
                ));
            }
            Ok(json!({"value": length.value, "unit": "model"}))
        })
        .transpose()?;
    let provenance = &result.provenance;
    if provenance.source.as_str().trim().is_empty()
        || provenance.session.as_str().trim().is_empty()
        || provenance.build_key.len() != 32
        || !provenance.build_key.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(diagnostic("input", "invalid planar snapshot provenance"));
    }
    if quality.accuracy_scope.trim().is_empty()
        || quality.source_claim.trim().is_empty()
        || quality.assumptions.iter().any(|text| text.trim().is_empty())
    {
        return Err(diagnostic("input", "invalid planar quality metadata"));
    }
    Ok(json!({
        "protocol": result.protocol, "request_id": result.request_id,
        "model": result.model, "node": result.node, "source": result.source,
        "provenance": {
            "source": provenance.source.as_str(), "session": provenance.session.as_str(),
            "node": provenance.node, "build_key": provenance.build_key,
        },
        "quality": {
            "exactness": exactness, "positional_uncertainty": uncertainty,
            "confidence": quality.confidence, "assumptions": quality.assumptions,
            "accuracy_scope": quality.accuracy_scope, "source_claim": quality.source_claim,
        },
    })
    .to_string())
}
