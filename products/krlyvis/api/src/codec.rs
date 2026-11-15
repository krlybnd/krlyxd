use crate::{diagnostic, Diagnostic, DrawingCall, DrawingImage, DrawingInput, DrawingResult};
use crate::{MAX_BASE64_BYTES, MAX_CALL_BYTES, MAX_IMAGE_BYTES};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use std::{collections::BTreeMap, fmt};

// Keep raw nested objects until their own strict parser sees duplicate fields.
// deserialize_map deliberately refuses serde's positional struct representation.
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

    fn optional<T: DeserializeOwned>(&mut self, name: &str) -> Result<Option<T>, Diagnostic> {
        self.0.remove(name).map(|raw| parse(raw.get())).transpose()
    }
}

fn parse<T: DeserializeOwned>(text: &str) -> Result<T, Diagnostic> {
    serde_json::from_str(text).map_err(|error| diagnostic("input", error.to_string()))
}

/// Bound the actual JSON request bytes, then decode and validate typed semantics.
pub fn decode_call(text: &str) -> Result<DrawingCall, Diagnostic> {
    if text.len() > MAX_CALL_BYTES {
        return Err(diagnostic("resource_limit", "drawing JSON exceeds 24 MiB"));
    }
    let mut fields = Object::read(text, &["protocol", "request_id", "input"])?;
    let raw: Box<RawValue> = fields.take("input")?;
    let mut input = Object::read(raw.get(), &["image", "pixel_unit", "simplify", "seed", "source"])?;
    let raw: Box<RawValue> = input.take("image")?;
    let mut image = Object::read(raw.get(), &["format", "data"])?;
    let data: String = image.take("data")?;
    if data.len() > MAX_BASE64_BYTES {
        return Err(diagnostic("resource_limit", "decoded image file exceeds 16 MiB"));
    }
    let data = STANDARD
        .decode(&data)
        .map_err(|_| diagnostic("input", "image.data must be canonical standard base64 with padding"))?;
    if data.len() > MAX_IMAGE_BYTES {
        return Err(diagnostic("resource_limit", "decoded image file exceeds 16 MiB"));
    }
    let call = DrawingCall {
        protocol: fields.take("protocol")?,
        request_id: fields.take("request_id")?,
        input: DrawingInput {
            image: DrawingImage { format: image.take("format")?, data },
            pixel_unit: input.take("pixel_unit")?,
            simplify: input.optional("simplify")?.unwrap_or(0.0),
            seed: input.take("seed")?,
            source: input.take("source")?,
        },
    };
    call.validate()?;
    Ok(call)
}

/// Validate the typed result before serde can replace a nonfinite float with null.
pub fn encode_result(result: &DrawingResult) -> Result<String, Diagnostic> {
    result.validate()?;
    serde_json::to_string(result).map_err(|error| diagnostic("output", error.to_string()))
}
