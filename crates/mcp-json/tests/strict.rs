#![allow(clippy::unwrap_used, clippy::panic)]

use krly_mcp_json::{validate, Fields};
use serde_json::{json, Value};

#[test]
fn syntax_and_ambiguous_values_keep_distinct_rpc_errors() {
    for (text, code, message) in [
        ("{", -32700, "Parse error"),
        ("{} {}", -32700, "Parse error"),
        ("NaN", -32700, "Parse error"),
        ("Infinity", -32700, "Parse error"),
        ("1e400", -32600, "Invalid request: duplicate keys or invalid JSON value"),
        (
            r#"{"nested":[{"a":1,"\u0061":2}]}"#,
            -32600,
            "Invalid request: duplicate keys or invalid JSON value",
        ),
    ] {
        let response = validate(text).err().unwrap().response(json!(17));
        assert_eq!(
            response,
            json!({"jsonrpc":"2.0", "id":17,
            "error":{"code":code,"message":message}})
        );
    }
}

#[test]
fn raw_fields_keep_large_integer_and_decimal_spellings() {
    let text = r#"{"arguments":{"large":18446744073709551616,"decimal":1.2300e+02},"name":"tool"}"#;
    assert!(validate(text).is_ok());
    let fields =
        Fields::read(text, &["arguments", "name"]).unwrap_or_else(|_| panic!("valid raw fields rejected"));
    assert_eq!(
        fields.required("arguments").unwrap_or_else(|_| panic!("missing arguments")).get(),
        r#"{"large":18446744073709551616,"decimal":1.2300e+02}"#
    );
    assert_eq!(fields.string("name").ok().as_deref(), Some("tool"));
}

#[test]
fn field_validation_preserves_parameter_errors() {
    for (text, message) in [
        ("[]", "expected an object"),
        (r#"{"extra":1}"#, "unknown field"),
        (r#"{"_meta":[]}"#, "_meta must be an object"),
    ] {
        let response = Fields::read(text, &["_meta"]).err().unwrap().response(Value::Null);
        assert_eq!(response["error"], json!({"code":-32602,"message":message}));
    }
}
