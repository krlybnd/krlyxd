//! Transport parsing retains argument number tokens until the domain codec sees them.

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub struct Failure {
    code: i32,
    message: String,
}

impl Failure {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

    pub fn params(message: impl Into<String>) -> Self {
        Self::new(-32602, message)
    }

    pub fn response(self, id: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id,
            "error": {"code": self.code, "message": self.message}})
    }
}

pub struct Fields<'a>(pub BTreeMap<String, &'a RawValue>);

impl<'a> Fields<'a> {
    pub fn read(text: &'a str, allowed: &[&str]) -> Result<Self, Failure> {
        let fields: BTreeMap<String, &RawValue> =
            serde_json::from_str(text).map_err(|_| Failure::params("expected an object"))?;
        if fields.keys().any(|name| !allowed.contains(&name.as_str())) {
            return Err(Failure::params("unknown field"));
        }
        if let Some(meta) = fields.get("_meta") {
            if !meta.get().starts_with('{') {
                return Err(Failure::params("_meta must be an object"));
            }
        }
        Ok(Self(fields))
    }

    pub fn required(&self, name: &str) -> Result<&'a RawValue, Failure> {
        self.0.get(name).copied().ok_or_else(|| Failure::params(format!("missing {name}")))
    }

    pub fn string(&self, name: &str) -> Result<String, Failure> {
        serde_json::from_str(self.required(name)?.get())
            .map_err(|_| Failure::params(format!("{name} must be a string")))
    }
}

/// Validate syntax and reject duplicate keys recursively, including escaped aliases.
/// Values are discarded; this pass never rewrites a numeric token.
pub fn validate(text: &str) -> Result<(), Failure> {
    // Distinguish malformed JSON from valid JSON with ambiguous duplicate keys.
    serde_json::from_str::<&RawValue>(text).map_err(|_| Failure::new(-32700, "Parse error"))?;
    serde_json::from_str::<Unique>(text)
        .map_err(|_| Failure::new(-32600, "Invalid request: duplicate keys or invalid JSON value"))?;
    Ok(())
}

struct Unique;

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = Unique;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON with unique object keys")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Unique, M::Error> {
        let mut names = BTreeSet::new();
        while let Some(name) = map.next_key::<String>()? {
            if !names.insert(name) {
                return Err(serde::de::Error::custom("duplicate key"));
            }
            map.next_value::<Unique>()?;
        }
        Ok(Unique)
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Unique, S::Error> {
        while seq.next_element::<Unique>()?.is_some() {}
        Ok(Unique)
    }

    fn visit_bool<E>(self, _: bool) -> Result<Unique, E> {
        Ok(Unique)
    }
    fn visit_i64<E>(self, _: i64) -> Result<Unique, E> {
        Ok(Unique)
    }
    fn visit_u64<E>(self, _: u64) -> Result<Unique, E> {
        Ok(Unique)
    }
    fn visit_f64<E>(self, _: f64) -> Result<Unique, E> {
        Ok(Unique)
    }
    fn visit_str<E>(self, _: &str) -> Result<Unique, E> {
        Ok(Unique)
    }
    fn visit_unit<E>(self) -> Result<Unique, E> {
        Ok(Unique)
    }
}
