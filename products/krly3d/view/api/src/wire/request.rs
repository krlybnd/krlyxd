use crate::{
    diagnostic, ApiVersion, Backend, Camera, Diagnostic, Length, LengthUnit, NamedView, RenderCall,
    RenderInput, RequestId,
};
use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{json, value::RawValue};
use std::{collections::BTreeMap, fmt};

pub(crate) struct Object(pub(crate) BTreeMap<String, Box<RawValue>>);
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
                        return Err(serde::de::Error::custom(format!("duplicate field {key}")));
                    }
                }
                Ok(Object(fields))
            }
        }
        deserializer.deserialize_map(Fields)
    }
}
impl Object {
    pub(crate) fn read(text: &str, allowed: &[&str]) -> Result<Self, Diagnostic> {
        let fields: Self = parse(text)?;
        if let Some(key) = fields.0.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(diagnostic("input", format!("unknown field {key}")));
        }
        Ok(fields)
    }
    pub(crate) fn take(&mut self, name: &str) -> Result<Box<RawValue>, Diagnostic> {
        self.0.remove(name).ok_or_else(|| diagnostic("input", format!("missing field {name}")))
    }
}
pub(crate) fn parse<T: DeserializeOwned>(text: &str) -> Result<T, Diagnostic> {
    serde_json::from_str(text).map_err(|e| diagnostic("input", e.to_string()))
}
pub(crate) fn version(text: &str) -> Result<ApiVersion, Diagnostic> {
    let mut fields = Object::read(text, &["major", "minor"])?;
    Ok(ApiVersion { major: parse(fields.take("major")?.get())?, minor: parse(fields.take("minor")?.get())? })
}
fn number(text: &str) -> Result<f64, Diagnostic> {
    if !text.contains(['.', 'e', 'E']) {
        let value: i64 = parse(text)?;
        if value.unsigned_abs() > (1_u64 << 53) - 1 {
            return Err(diagnostic("input", "number exceeds JSON-safe integer range"));
        }
    }
    let value: f64 = parse(text)?;
    if !value.is_finite() {
        return Err(diagnostic("input", "non-finite number"));
    }
    Ok(value)
}
fn vector<const N: usize>(text: &str) -> Result<[f64; N], Diagnostic> {
    let items: Vec<Box<RawValue>> = parse(text)?;
    if items.len() != N {
        return Err(diagnostic("input", format!("expected {N} vector components")));
    }
    let mut values = [0.0; N];
    for (out, item) in values.iter_mut().zip(items) {
        *out = number(item.get())?;
    }
    Ok(values)
}
fn quantity(text: &str) -> Result<Length, Diagnostic> {
    let mut fields = Object::read(text, &["value", "unit"])?;
    let unit: String = parse(fields.take("unit")?.get())?;
    if unit != "model" {
        return Err(diagnostic("input", "only model units are supported"));
    }
    Ok(Length { value: number(fields.take("value")?.get())?, unit: LengthUnit::Model })
}
fn named(text: &str) -> Result<NamedView, Diagnostic> {
    match parse::<String>(text)?.as_str() {
        "iso" => Ok(NamedView::Iso),
        "front" => Ok(NamedView::Front),
        "back" => Ok(NamedView::Back),
        "left" => Ok(NamedView::Left),
        "right" => Ok(NamedView::Right),
        "top" => Ok(NamedView::Top),
        "bottom" => Ok(NamedView::Bottom),
        _ => Err(diagnostic("input", "unknown named camera")),
    }
}
fn camera(text: &str) -> Result<Camera, Diagnostic> {
    let mut fields = Object::read(text, &["kind", "name", "toward", "up", "eye", "focal", "centre"])?;
    let kind: String = parse(fields.take("kind")?.get())?;
    let camera = match kind.as_str() {
        "named" => Camera::Named(named(fields.take("name")?.get())?),
        "orthographic" => Camera::Orthographic {
            toward: vector(fields.take("toward")?.get())?,
            up: vector(fields.take("up")?.get())?,
        },
        "pinhole" => Camera::Pinhole {
            eye: vector(fields.take("eye")?.get())?,
            toward: vector(fields.take("toward")?.get())?,
            up: vector(fields.take("up")?.get())?,
            focal: number(fields.take("focal")?.get())?,
            centre: fields.0.remove("centre").map(|raw| vector(raw.get())).transpose()?,
        },
        _ => return Err(diagnostic("input", "unknown camera kind")),
    };
    if !fields.0.is_empty() {
        return Err(diagnostic("input", "unexpected fields for camera kind"));
    }
    Ok(camera)
}
pub(crate) fn input(text: &str) -> Result<RenderInput, Diagnostic> {
    let mut fields = Object::read(text, &["node", "width", "height", "backend", "camera", "tolerance"])?;
    let mut input = RenderInput::new(parse::<String>(fields.take("node")?.get())?);
    if let Some(value) = fields.0.remove("width") {
        input.width = parse(value.get())?;
    }
    if let Some(value) = fields.0.remove("height") {
        input.height = parse(value.get())?;
    }
    if let Some(value) = fields.0.remove("camera") {
        input.camera = camera(value.get())?;
    }
    if let Some(value) = fields.0.remove("tolerance") {
        input.tolerance = quantity(value.get())?;
    }
    if let Some(value) = fields.0.remove("backend") {
        input.backend = match parse::<String>(value.get())?.as_str() {
            "raycast" => Backend::Raycast,
            "mesh" => Backend::Mesh,
            _ => return Err(diagnostic("input", "unknown backend")),
        };
    }
    Ok(input)
}

pub fn decode_call(text: &str) -> Result<RenderCall, Diagnostic> {
    let mut fields = Object::read(text, &["protocol", "api", "request_id", "input"])?;
    if version(fields.take("protocol")?.get())? != crate::API_VERSION {
        return Err(diagnostic("unsupported_version", "render requires wire protocol 1.0"));
    }
    let call = RenderCall {
        request_id: RequestId::new(&parse::<String>(fields.take("request_id")?.get())?),
        api: version(fields.take("api")?.get())?,
        input: input(fields.take("input")?.get())?,
    };
    call.validate()?;
    Ok(call)
}
pub fn encode_call(call: &RenderCall) -> Result<String, Diagnostic> {
    call.validate()?;
    Ok(json!({"protocol":super::version(),"api":super::version(),"request_id":call.request_id.as_str(),"input":super::input(&call.input)}).to_string())
}
