use crate::diagnostic;
use krly3d_core::schema::Value;
use krlyvis_api::Diagnostic;
use serde_json::{Map, Number, Value as Json};

/// A checked core-value bridge, with the same integral number normalization as
/// the legacy transport. This is serialization only; datum geometry is untouched.
pub(super) fn to_json(value: Value) -> Result<Json, Diagnostic> {
    match value {
        Value::Bool(flag) => Ok(Json::Bool(flag)),
        Value::Number(number) => {
            if !number.is_finite() {
                return Err(diagnostic("output", "nonfinite provider evidence cannot be JSON"));
            }
            if number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_992.0 {
                Ok(Json::from(number as i64))
            } else {
                Number::from_f64(number)
                    .map(Json::Number)
                    .ok_or_else(|| diagnostic("output", "invalid evidence number"))
            }
        }
        Value::Text(text) => Ok(Json::String(text)),
        Value::List(values) => {
            values.into_iter().map(to_json).collect::<Result<Vec<_>, _>>().map(Json::Array)
        }
        Value::Map(values) => values
            .into_iter()
            .map(|(key, value)| Ok((key, to_json(value)?)))
            .collect::<Result<Map<_, _>, Diagnostic>>()
            .map(Json::Object),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::to_json;
    use krly3d_core::schema::Value;

    #[test]
    fn refuses_nonfinite_core_values_and_normalizes_integral_numbers() {
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(to_json(Value::map([("nested", Value::List(vec![number.into()]))])).is_err());
        }
        assert_eq!(
            to_json(Value::List(vec![6.0.into(), (-0.0).into(), 0.5.into()])).unwrap().to_string(),
            "[6,0,0.5]"
        );
    }
}
