//! Named parameters and what a plugin says about them.
//!
//! One schema serves three purposes: checking the parameters of a call, reading and
//! writing them as text, and describing the plugin to a client.

use crate::math::{Point3, Vec3};
use crate::{Error, Id};
use std::collections::BTreeMap;

/// A nested value: what goes into a call and what comes out of one. It maps one to one
/// onto JSON without `null`, so any transport can carry it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

impl Value {
    /// A map from name and value pairs.
    pub fn map<const N: usize>(entries: [(&str, Value); N]) -> Self {
        Self::Map(entries.into_iter().map(|(name, value)| (name.to_string(), value)).collect())
    }

    /// A list of texts.
    pub fn texts<'a>(items: impl IntoIterator<Item = &'a str>) -> Self {
        Self::List(items.into_iter().map(Self::from).collect())
    }
}

impl From<bool> for Value {
    fn from(flag: bool) -> Self {
        Self::Bool(flag)
    }
}

impl From<String> for Value {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<usize> for Value {
    fn from(count: usize) -> Self {
        Self::Number(count as f64)
    }
}

impl From<f64> for Value {
    fn from(number: f64) -> Self {
        Self::Number(number)
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<[f64; 3]> for Value {
    fn from(triple: [f64; 3]) -> Self {
        Self::List(triple.into_iter().map(Self::Number).collect())
    }
}

impl From<Vec<Value>> for Value {
    fn from(list: Vec<Value>) -> Self {
        Self::List(list)
    }
}

/// The parameters of one call, by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params {
    values: BTreeMap<String, Value>,
}

impl Params {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.set(name, value);
        self
    }

    pub fn set(&mut self, name: &str, value: impl Into<Value>) {
        self.values.insert(name.to_string(), value.into());
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.values.iter().map(|(name, value)| (name.as_str(), value))
    }

    pub fn from_map(values: BTreeMap<String, Value>) -> Self {
        Self { values }
    }

    pub fn into_value(self) -> Value {
        Value::Map(self.values)
    }

    pub fn remove(&mut self, name: &str) -> Option<Value> {
        self.values.remove(name)
    }

    pub fn flag(&self, name: &str) -> Result<bool, Error> {
        match self.get(name) {
            Some(Value::Bool(flag)) => Ok(*flag),
            other => Err(mismatch(name, "true or false", other)),
        }
    }

    pub fn list(&self, name: &str) -> Result<&[Value], Error> {
        match self.get(name) {
            Some(Value::List(items)) => Ok(items),
            other => Err(mismatch(name, "a list", other)),
        }
    }

    pub fn number(&self, name: &str) -> Result<f64, Error> {
        match self.get(name) {
            Some(Value::Number(number)) => Ok(*number),
            other => Err(mismatch(name, "a number", other)),
        }
    }

    pub fn text(&self, name: &str) -> Result<&str, Error> {
        match self.get(name) {
            Some(Value::Text(text)) => Ok(text),
            other => Err(mismatch(name, "a text", other)),
        }
    }

    pub fn numbers(&self, name: &str) -> Result<Vec<f64>, Error> {
        match self.get(name) {
            Some(Value::List(items)) => items
                .iter()
                .map(|item| match item {
                    Value::Number(number) => Ok(*number),
                    other => Err(mismatch(name, "a list of numbers", Some(other))),
                })
                .collect(),
            other => Err(mismatch(name, "a list of numbers", other)),
        }
    }

    pub fn texts(&self, name: &str) -> Result<Vec<String>, Error> {
        match self.get(name) {
            Some(Value::List(items)) => items
                .iter()
                .map(|item| match item {
                    Value::Text(text) => Ok(text.clone()),
                    other => Err(mismatch(name, "a list of texts", Some(other))),
                })
                .collect(),
            other => Err(mismatch(name, "a list of texts", other)),
        }
    }

    pub fn vector(&self, name: &str) -> Result<Vec3, Error> {
        match self.numbers(name)?.as_slice() {
            [x, y, z] => Ok(Vec3::new(*x, *y, *z)),
            _ => Err(mismatch(name, "three numbers", self.get(name))),
        }
    }

    pub fn point(&self, name: &str) -> Result<Point3, Error> {
        self.vector(name).map(Point3::from)
    }

    /// A whole number of at least one.
    pub fn count(&self, name: &str) -> Result<usize, Error> {
        let number = self.number(name)?;
        if number >= 1.0 && number.fract() == 0.0 && number <= f64::from(u32::MAX) {
            Ok(number as usize)
        } else {
            Err(mismatch(name, "a whole number of at least 1", self.get(name)))
        }
    }
}

fn mismatch(name: &str, wanted: &str, found: Option<&Value>) -> Error {
    match found {
        Some(value) => Error::input(format!("parameter `{name}` must be {wanted}, got {value:?}")),
        None => Error::input(format!("parameter `{name}` is missing")),
    }
}

/// What kind of value a parameter takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Number,
    /// A whole number of at least one.
    Count,
    Text,
    /// Three numbers: a point or a vector.
    Triple,
    Numbers,
    Texts,
    /// `true` or `false`.
    Flag,
    /// Any value, nested as deep as the plugin likes; the plugin checks it itself.
    Any,
}

impl Kind {
    /// The kind as a word, for descriptions of a method.
    pub fn name(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Count => "count",
            Self::Text => "text",
            Self::Triple => "triple",
            Self::Numbers => "numbers",
            Self::Texts => "texts",
            Self::Flag => "flag",
            Self::Any => "any",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    /// Whether the caller must give it.
    pub required: bool,
    /// The value used when the caller gives none.
    pub default: Option<Value>,
    pub about: &'static str,
}

impl Param {
    pub fn required(name: &'static str, kind: Kind, about: &'static str) -> Self {
        Self { name, kind, required: true, default: None, about }
    }

    pub fn optional(name: &'static str, kind: Kind, default: impl Into<Value>, about: &'static str) -> Self {
        Self { name, kind, required: false, default: Some(default.into()), about }
    }

    /// One of several ways of saying the same thing, such as a height or an end point:
    /// absent unless given, so that the plugin can tell which way was chosen and refuse
    /// two at once.
    pub fn alternative(name: &'static str, kind: Kind, about: &'static str) -> Self {
        Self { name, kind, required: false, default: None, about }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    pub about: &'static str,
    /// Fewest and most input bodies.
    pub inputs: (usize, usize),
    pub params: Vec<Param>,
}

impl Schema {
    /// The parameters as the plugin will see them: defaults filled in, kinds verified,
    /// and nothing the plugin does not know.
    pub fn admit(&self, plugin: &Id, given: &Params, inputs: usize) -> Result<Params, Error> {
        if inputs < self.inputs.0 || inputs > self.inputs.1 {
            let (least, most) = self.inputs;
            let wanted = if least == most { format!("{least}") } else { format!("{least} to {most}") };
            return Err(Error::input(format!("`{plugin}` takes {wanted} input(s), got {inputs}")));
        }
        if let Some((unknown, _)) =
            given.iter().find(|(name, _)| !self.params.iter().any(|param| param.name == *name))
        {
            let known: Vec<&str> = self.params.iter().map(|param| param.name).collect();
            return Err(Error::input(format!(
                "`{plugin}` has no parameter `{unknown}`; it takes {}",
                known.join(", ")
            )));
        }
        let mut admitted = Params::new();
        for param in &self.params {
            let Some(value) = given.get(param.name).or(param.default.as_ref()) else {
                if param.required {
                    return Err(Error::input(format!("`{plugin}` needs `{}`: {}", param.name, param.about)));
                }
                continue;
            };
            admitted.set(param.name, shaped(plugin, param, value)?);
        }
        Ok(admitted)
    }
}

/// The value in the shape its kind promises, so that a plugin reads it without guessing:
/// a single item given for a list becomes a list of one.
fn shaped(plugin: &Id, param: &Param, value: &Value) -> Result<Value, Error> {
    let all = |items: &[Value], wanted: fn(&Value) -> bool| items.iter().all(wanted);
    let number = |value: &Value| matches!(value, Value::Number(_));
    let text = |value: &Value| matches!(value, Value::Text(_));
    let fits = match (param.kind, value) {
        (Kind::Number, Value::Number(_)) | (Kind::Text, Value::Text(_)) => true,
        (Kind::Flag, Value::Bool(_)) | (Kind::Any, _) => true,
        (Kind::Count, Value::Number(count)) => *count >= 1.0 && count.fract() == 0.0,
        (Kind::Triple, Value::List(items)) => items.len() == 3 && all(items, number),
        (Kind::Numbers, Value::List(items)) => all(items, number),
        (Kind::Texts, Value::List(items)) => all(items, text),
        (Kind::Numbers, Value::Number(_)) | (Kind::Texts, Value::Text(_)) => {
            return Ok(Value::List(vec![value.clone()]));
        }
        _ => false,
    };
    if fits {
        Ok(value.clone())
    } else {
        Err(Error::input(format!(
            "`{plugin}`: `{}` must be {}, got {value:?}",
            param.name,
            wanted(param.kind)
        )))
    }
}

fn wanted(kind: Kind) -> &'static str {
    match kind {
        Kind::Number => "a number",
        Kind::Flag => "true or false",
        Kind::Any => "a value",
        Kind::Count => "a whole number of at least 1",
        Kind::Text => "a text",
        Kind::Triple => "three numbers",
        Kind::Numbers => "a list of numbers",
        Kind::Texts => "a list of texts",
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, Param, Params, Schema, Value};
    use crate::{ErrorKind, Id};

    fn schema() -> Schema {
        Schema {
            about: "a block",
            inputs: (0, 0),
            params: vec![
                Param::required("w", Kind::Number, "width along x"),
                Param::optional("at", Kind::Triple, [0.0, 0.0, 0.0], "where the base centre goes"),
                Param::optional("names", Kind::Texts, Vec::new(), "edge names"),
            ],
        }
    }

    #[test]
    fn defaults_fill_in_what_the_caller_leaves_out() {
        let admitted = schema().admit(&Id::new("box"), &Params::new().with("w", 4.0), 0).unwrap();

        assert_eq!(admitted.number("w").unwrap(), 4.0);
        assert_eq!(admitted.numbers("at").unwrap(), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_missing_required_parameter_is_named_with_its_meaning() {
        let error = schema().admit(&Id::new("box"), &Params::new(), 0).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Input);
        assert_eq!(error.message, "`box` needs `w`: width along x");
    }

    #[test]
    fn an_unknown_parameter_is_rejected_with_the_known_ones() {
        let given = Params::new().with("w", 4.0).with("height", 2.0);

        let error = schema().admit(&Id::new("box"), &given, 0).unwrap_err();

        assert_eq!(error.message, "`box` has no parameter `height`; it takes w, at, names");
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_rejected() {
        let error = schema().admit(&Id::new("box"), &Params::new().with("w", "wide"), 0).unwrap_err();

        assert_eq!(error.message, "`box`: `w` must be a number, got Text(\"wide\")");
    }

    #[test]
    fn a_single_item_given_for_a_list_becomes_a_list_of_one() {
        let given = Params::new().with("w", 4.0).with("names", "front");

        let admitted = schema().admit(&Id::new("box"), &given, 0).unwrap();

        assert_eq!(admitted.get("names"), Some(&Value::List(vec![Value::Text("front".into())])));
    }

    #[test]
    fn the_wrong_number_of_inputs_is_rejected() {
        let error = schema().admit(&Id::new("box"), &Params::new().with("w", 4.0), 2).unwrap_err();

        assert_eq!(error.message, "`box` takes 0 input(s), got 2");
    }
}
