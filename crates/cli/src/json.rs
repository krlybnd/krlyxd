//! Values of the kernel as JSON and back, and a schema as JSON Schema.
//!
//! Both directions are total: every input gives either the converted value or a
//! [`Misfit`] that says what cannot cross and where in the nesting it sits. JSON has one
//! thing a [`Value`] lacks (`null`) and a [`Value`] has one thing JSON lacks (numbers that
//! are not finite); both are refused rather than replaced by something else.

use krly3d_kernel::schema::{Kind, Params, Schema, Value};
use krly3d_kernel::Reply;
use serde_json::{json, Map, Number, Value as Json};
use std::fmt;

/// The largest whole number below which every whole number is an `f64` (2^53). Whole
/// numbers up to it are written without a fraction, so a count reads `6`, not `6.0`.
const LARGEST_WHOLE: f64 = 9_007_199_254_740_992.0;

/// Why a value has no counterpart on the other side, and where it sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Misfit {
    /// The way down to the value, innermost step first.
    steps: Vec<String>,
    reason: &'static str,
}

impl Misfit {
    fn new(reason: &'static str) -> Self {
        Self { steps: Vec::new(), reason }
    }

    fn under(mut self, step: String) -> Self {
        self.steps.push(step);
        self
    }
}

impl fmt::Display for Misfit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.steps.is_empty() {
            return f.write_str(self.reason);
        }
        let mut path = String::new();
        for step in self.steps.iter().rev() {
            if !path.is_empty() && !step.starts_with('[') {
                path.push('.');
            }
            path.push_str(step);
        }
        write!(f, "`{path}`: {}", self.reason)
    }
}

const NULL: &str = "null has no counterpart here; leave the entry out instead";
const NOT_FINITE: &str = "a number that is not finite cannot be written as JSON";
const NOT_A_MAP: &str = "arguments are given by name, as a JSON object";

pub fn to_json(value: &Value) -> Result<Json, Misfit> {
    match value {
        Value::Bool(flag) => Ok(Json::Bool(*flag)),
        Value::Number(number) => number_to_json(*number).ok_or_else(|| Misfit::new(NOT_FINITE)),
        Value::Text(text) => Ok(Json::String(text.clone())),
        Value::List(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| to_json(item).map_err(|misfit| misfit.under(format!("[{index}]"))))
            .collect::<Result<Vec<Json>, Misfit>>()
            .map(Json::Array),
        Value::Map(entries) => entries
            .iter()
            .map(|(name, entry)| match to_json(entry) {
                Ok(entry) => Ok((name.clone(), entry)),
                Err(misfit) => Err(misfit.under(name.clone())),
            })
            .collect::<Result<Map<String, Json>, Misfit>>()
            .map(Json::Object),
    }
}

fn number_to_json(number: f64) -> Option<Json> {
    if number.fract() == 0.0 && number.abs() <= LARGEST_WHOLE {
        // Within this range the conversion is exact, and it turns -0 into 0.
        Some(Json::from(number as i64))
    } else {
        Number::from_f64(number).map(Json::Number)
    }
}

pub fn from_json(json: &Json) -> Result<Value, Misfit> {
    match json {
        Json::Null => Err(Misfit::new(NULL)),
        Json::Bool(flag) => Ok(Value::Bool(*flag)),
        Json::Number(number) => number
            .as_f64()
            .filter(|number| number.is_finite())
            .map(Value::Number)
            .ok_or_else(|| Misfit::new("the number is too large to be kept")),
        Json::String(text) => Ok(Value::Text(text.clone())),
        Json::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| from_json(item).map_err(|misfit| misfit.under(format!("[{index}]"))))
            .collect::<Result<Vec<Value>, Misfit>>()
            .map(Value::List),
        Json::Object(entries) => entries
            .iter()
            .map(|(name, entry)| match from_json(entry) {
                Ok(entry) => Ok((name.clone(), entry)),
                Err(misfit) => Err(misfit.under(name.clone())),
            })
            .collect::<Result<_, Misfit>>()
            .map(Value::Map),
    }
}

/// The arguments of a call from a JSON object.
pub fn params_from_json(json: &Json) -> Result<Params, Misfit> {
    match from_json(json)? {
        Value::Map(entries) => Ok(Params::from_map(entries)),
        _ => Err(Misfit::new(NOT_A_MAP)),
    }
}

/// A reply as the one object `{ok, text, data, exact}`.
pub fn reply_to_json(reply: &Reply) -> Result<Json, Misfit> {
    to_json(&reply.to_value())
}

/// A schema as a JSON Schema of the object that holds the arguments. A parameter without
/// `required` must be given; nothing the schema does not name is accepted.
pub fn schema_to_json(schema: &Schema) -> Result<Json, Misfit> {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for param in &schema.params {
        let mut property = kind_to_json(param.kind);
        property.insert("description".to_string(), Json::from(param.about));
        if let Some(default) = &param.default {
            let default = to_json(default).map_err(|misfit| misfit.under(param.name.to_string()))?;
            property.insert("default".to_string(), default);
        }
        if param.required {
            required.push(param.name);
        }
        properties.insert(param.name.to_string(), Json::Object(property));
    }
    Ok(json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    }))
}

fn kind_to_json(kind: Kind) -> Map<String, Json> {
    let list_of = |item: &str| json!({ "type": "array", "items": { "type": item } });
    let constraint = match kind {
        Kind::Number => json!({ "type": "number" }),
        Kind::Count => json!({ "type": "integer", "minimum": 1 }),
        Kind::Text => json!({ "type": "string" }),
        Kind::Triple => {
            json!({ "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 })
        }
        Kind::Numbers => list_of("number"),
        Kind::Texts => list_of("string"),
        Kind::Flag => json!({ "type": "boolean" }),
        Kind::Any => json!({}),
    };
    match constraint {
        Json::Object(constraint) => constraint,
        _ => Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{from_json, params_from_json, schema_to_json, to_json};
    use krly3d_kernel::schema::{Kind, Param, Schema, Value};
    use serde_json::{json, Value as Json};

    fn nested() -> Value {
        Value::map([
            ("flag", Value::from(true)),
            ("size", Value::from(2.5)),
            ("name", Value::from("plate")),
            ("at", Value::from([1.0, -2.0, 0.0])),
            ("inner", Value::map([("list", Value::List(vec![Value::from("a"), Value::from(false)]))])),
        ])
    }

    #[test]
    fn a_nested_value_reads_back_from_json_as_it_was() {
        let value = nested();

        let back = from_json(&to_json(&value).unwrap()).unwrap();

        assert_eq!(back, value);
    }

    #[test]
    fn a_whole_number_is_written_without_a_fraction() {
        let written = to_json(&Value::List(vec![Value::from(6.0), Value::from(-0.0), Value::from(0.5)]));

        assert_eq!(written.unwrap().to_string(), "[6,0,0.5]");
    }

    #[test]
    fn a_json_integer_becomes_a_number() {
        let read = from_json(&json!({ "n": 3, "big": 18446744073709551615u64, "low": -7 })).unwrap();

        assert_eq!(
            read,
            Value::map([
                ("n", Value::from(3.0)),
                ("big", Value::from(18_446_744_073_709_551_615.0)),
                ("low", Value::from(-7.0)),
            ])
        );
    }

    #[test]
    fn null_is_refused_with_the_place_it_was_found() {
        let misfit = from_json(&json!({ "calls": [{ "args": { "w": null } }] })).unwrap_err();

        assert_eq!(
            misfit.to_string(),
            "`calls[0].args.w`: null has no counterpart here; leave the entry out instead"
        );
    }

    #[test]
    fn a_number_that_is_not_finite_is_refused_with_the_place_it_was_found() {
        let value = Value::map([("volume", Value::List(vec![Value::from(1.0), Value::from(f64::NAN)]))]);

        let misfit = to_json(&value).unwrap_err();

        assert_eq!(misfit.to_string(), "`volume[1]`: a number that is not finite cannot be written as JSON");
    }

    #[test]
    fn arguments_that_are_not_an_object_are_refused() {
        let misfit = params_from_json(&json!([1, 2, 3])).unwrap_err();

        assert_eq!(misfit.to_string(), "arguments are given by name, as a JSON object");
    }

    #[test]
    fn arguments_keep_their_names_and_kinds() {
        let params = params_from_json(&json!({ "id": "plate", "w": 60, "dry": true })).unwrap();

        assert_eq!(params.text("id").unwrap(), "plate");
        assert_eq!(params.number("w").unwrap(), 60.0);
        assert!(params.flag("dry").unwrap());
    }

    #[test]
    fn every_kind_of_parameter_has_its_json_schema() {
        let kinds = [
            ("number", Kind::Number),
            ("count", Kind::Count),
            ("text", Kind::Text),
            ("triple", Kind::Triple),
            ("numbers", Kind::Numbers),
            ("texts", Kind::Texts),
            ("flag", Kind::Flag),
            ("any", Kind::Any),
        ];
        let schema = Schema {
            about: "all kinds",
            inputs: (0, 0),
            params: kinds.iter().map(|(name, kind)| Param::required(name, *kind, "what it is")).collect(),
        };

        let written = schema_to_json(&schema).unwrap();

        let property = |name: &str| written["properties"][name].clone();
        assert_eq!(property("number"), json!({ "type": "number", "description": "what it is" }));
        assert_eq!(
            property("count"),
            json!({ "type": "integer", "minimum": 1, "description": "what it is" })
        );
        assert_eq!(property("text"), json!({ "type": "string", "description": "what it is" }));
        assert_eq!(
            property("triple"),
            json!({ "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3,
                    "description": "what it is" })
        );
        assert_eq!(
            property("numbers"),
            json!({ "type": "array", "items": { "type": "number" }, "description": "what it is" })
        );
        assert_eq!(
            property("texts"),
            json!({ "type": "array", "items": { "type": "string" }, "description": "what it is" })
        );
        assert_eq!(property("flag"), json!({ "type": "boolean", "description": "what it is" }));
        assert_eq!(property("any"), json!({ "description": "what it is" }));
    }

    #[test]
    fn the_parameters_without_a_default_are_the_required_ones() {
        let schema = Schema {
            about: "a block",
            inputs: (0, 0),
            params: vec![
                Param::required("w", Kind::Number, "width along x"),
                Param::optional("at", Kind::Triple, [0.0, 0.0, 0.0], "where the base centre goes"),
                Param::required("h", Kind::Number, "height along z"),
            ],
        };

        let written = schema_to_json(&schema).unwrap();

        assert_eq!(written["type"], json!("object"));
        assert_eq!(written["required"], json!(["w", "h"]));
        assert_eq!(written["additionalProperties"], Json::Bool(false));
        assert_eq!(written["properties"]["at"]["default"], json!([0, 0, 0]));
        assert_eq!(written["properties"]["w"].get("default"), None);
    }
}
