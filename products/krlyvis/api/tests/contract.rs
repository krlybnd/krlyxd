#![allow(clippy::unwrap_used)]

use base64::{engine::general_purpose::STANDARD, Engine};
use krlyvis_api::{
    decode_call, descriptor, metadata, MAX_CALL_BYTES, MAX_IMAGE_BYTES, MAX_SIMPLIFY_PIXELS, PROTOCOL,
};
use serde_json::{json, Value};

const CALL: &str = r#"{"protocol":"krlyvis.drawing/1","request_id":"request-1","input":{"image":{"format":"pnm","data":"UDUgMSAxIDI1NQoA"},"pixel_unit":2.5,"seed":null,"source":"caller:unverified"}}"#;

fn fixture() -> Value {
    serde_json::from_str(CALL).unwrap()
}

#[test]
fn decodes_actual_image_bytes_and_preserves_correlation_scale_seed_and_claim() {
    let call = decode_call(CALL).unwrap();
    assert_eq!(call.protocol, PROTOCOL);
    assert_eq!(call.request_id, "request-1");
    assert_eq!(call.input.image.format, "pnm");
    assert_eq!(call.input.image.data, b"P5 1 1 255\n\0");
    assert_eq!(call.input.pixel_unit, 2.5);
    assert_eq!(call.input.simplify, 0.0);
    assert_eq!(call.input.seed, None);
    assert_eq!(call.input.source, "caller:unverified");
    call.validate().unwrap();
    let mut value = fixture();
    value["request_id"] = json!(""); // The contract requires a string, not a nonempty ID.
    value["input"]["seed"] = json!([3.25, 4.5]);
    assert_eq!(decode_call(&value.to_string()).unwrap().input.seed, Some([3.25, 4.5]));
}

#[test]
fn simplification_is_optional_explicit_bounded_and_rejects_duplicate_fields() {
    for tolerance in [0.0, 1.0, MAX_SIMPLIFY_PIXELS] {
        let mut value = fixture();
        value["input"]["simplify"] = json!(tolerance);
        assert_eq!(decode_call(&value.to_string()).unwrap().input.simplify, tolerance);
    }
    for value in [json!(-1), json!(MAX_SIMPLIFY_PIXELS + 1.0), json!("1"), json!(true), Value::Null] {
        let mut input = fixture();
        input["input"]["simplify"] = value;
        assert!(decode_call(&input.to_string()).is_err());
    }
    for tolerance in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, MAX_SIMPLIFY_PIXELS + 1.0] {
        let mut call = decode_call(CALL).unwrap();
        call.input.simplify = tolerance;
        assert!(call.validate().is_err());
    }
    for fields in ["\"simplify\":0,\"simplify\":1,", "\"simplify\":0,\"simpl\\u0069fy\":1,"] {
        let text = CALL.replacen("\"pixel_unit\":", &format!("{fields}\"pixel_unit\":"), 1);
        assert!(decode_call(&text).is_err());
    }
}

#[test]
fn rejects_unknown_duplicate_and_escaped_duplicate_keys_at_every_object_level() {
    for (needle, replacement) in [
        ("\"protocol\":", "\"extra\":0,\"protocol\":"),
        ("\"image\":", "\"extra\":0,\"image\":"),
        ("\"format\":", "\"extra\":0,\"format\":"),
        ("\"protocol\":", "\"protocol\":\"first\",\"protocol\":"),
        ("\"request_id\":", "\"request_id\":\"first\",\"request_id\":"),
        ("\"input\":", "\"input\":{},\"input\":"),
        ("\"image\":", "\"image\":{},\"image\":"),
        ("\"seed\":", "\"seed\":null,\"seed\":"),
        ("\"source\":", "\"source\":\"first\",\"source\":"),
        ("\"pixel_unit\":", "\"pixel_unit\":1,\"pixel_unit\":"),
        ("\"format\":", "\"format\":\"png\",\"format\":"),
        ("\"data\":", "\"data\":\"\",\"data\":"),
        ("\"data\":", "\"da\\u0074a\":\"\",\"data\":"),
        ("\"seed\":", "\"se\\u0065d\":null,\"seed\":"),
    ] {
        assert!(decode_call(&CALL.replacen(needle, replacement, 1)).is_err(), "{replacement}");
    }
}

#[test]
fn every_field_is_required_even_nullable_seed_and_positional_objects_are_refused() {
    for path in [vec![], vec!["input"], vec!["input", "image"]] {
        let mut value = fixture();
        let mut object = &mut value;
        for part in &path {
            object = &mut object[*part];
        }
        let keys: Vec<_> = object.as_object().unwrap().keys().cloned().collect();
        for key in keys {
            let mut value = fixture();
            let mut object = &mut value;
            for part in &path {
                object = &mut object[*part];
            }
            object.as_object_mut().unwrap().remove(&key);
            assert!(decode_call(&value.to_string()).is_err(), "missing {path:?}.{key}");
        }
    }
    for text in ["null", "[]", "{}", "", "[\"krlyvis.drawing/1\",\"id\",{}]"] {
        assert!(decode_call(text).is_err());
    }
    for (path, value) in [
        ("/input", json!([{"format": "pnm", "data": ""}, 1, null, "source"])),
        ("/input/image", json!(["pnm", ""])),
        ("/input/image", Value::Null),
        ("/request_id", json!(42)),
        ("/input/source", Value::Null),
    ] {
        let mut input = fixture();
        *input.pointer_mut(path).unwrap() = value;
        assert!(decode_call(&input.to_string()).is_err());
    }
    assert!(decode_call(&format!("{CALL} {{}}")).is_err());
    let mut value = fixture();
    value["protocol"] = json!("krlyvis.drawing/2");
    assert_eq!(decode_call(&value.to_string()).unwrap_err().code.as_str(), "unsupported_version");
}

#[test]
fn strict_numeric_format_and_source_validation() {
    for value in [json!(0), json!(-1), json!(1e9 + 1.0), json!("1"), json!(true), Value::Null] {
        let mut input = fixture();
        input["input"]["pixel_unit"] = value;
        assert!(decode_call(&input.to_string()).is_err());
    }
    for value in [
        json!([]),
        json!([1]),
        json!([1, 2, 3]),
        json!([-1, 0]),
        json!(["1", 2]),
        json!([null, 2]),
        json!({"x": 1, "y": 2}),
    ] {
        let mut input = fixture();
        input["input"]["seed"] = value;
        assert!(decode_call(&input.to_string()).is_err());
    }
    for token in ["1e309", "NaN", "Infinity", "-Infinity"] {
        assert!(decode_call(&CALL.replace("\"pixel_unit\":2.5", &format!("\"pixel_unit\":{token}"))).is_err());
        assert!(decode_call(&CALL.replace("\"seed\":null", &format!("\"seed\":[{token},0]"))).is_err());
    }
    for format in ["jpg", "PNG", "pnm ", "gif", ""] {
        let mut input = fixture();
        input["input"]["image"]["format"] = json!(format);
        assert!(decode_call(&input.to_string()).is_err());
    }
    for source in ["", " \t\n"] {
        let mut input = fixture();
        input["input"]["source"] = json!(source);
        assert!(decode_call(&input.to_string()).is_err());
    }
    for pixel_unit in [f64::MIN_POSITIVE, f64::from_bits(1), 1.0e9] {
        let mut input = fixture();
        input["input"]["pixel_unit"] = json!(pixel_unit);
        decode_call(&input.to_string()).unwrap();
    }
}

#[test]
fn standard_base64_is_strict_about_padding_alphabet_trailing_bits_and_whitespace() {
    for data in [
        "!",
        "AA",
        "A===",
        "AA=",
        "AA===",
        "AB==",
        "AAB=",
        "_w==",
        "-w==",
        "AA==\n",
        " AA==",
        "data:image/png;base64,AA==",
    ] {
        let mut value = fixture();
        value["input"]["image"]["data"] = json!(data);
        assert!(decode_call(&value.to_string()).is_err(), "{data:?}");
    }
    for data in ["", "AA==", "AAA=", "AAAA", "////"] {
        let mut value = fixture();
        value["input"]["image"]["data"] = json!(data);
        assert!(decode_call(&value.to_string()).is_ok(), "{data:?}");
    }
}

#[test]
fn checks_encoded_and_decoded_limits_including_one_byte_over_with_same_base64_length() {
    assert_eq!(decode_call(&" ".repeat(MAX_CALL_BYTES + 1)).unwrap_err().code.as_str(), "resource_limit");
    let mut call = decode_call(CALL).unwrap();
    call.input.image.data = vec![0; MAX_IMAGE_BYTES];
    call.validate().unwrap();
    for count in [MAX_IMAGE_BYTES, MAX_IMAGE_BYTES + 1] {
        let mut value = fixture();
        value["input"]["image"]["data"] = json!(STANDARD.encode(vec![0; count]));
        let text = value.to_string();
        assert!(text.len() < MAX_CALL_BYTES);
        assert_eq!(decode_call(&text).is_ok(), count == MAX_IMAGE_BYTES);
    }
    call.input.image.data.push(0);
    assert!(call.validate().is_err());
    call.input.image.data.clear();
    call.input.source = "\"".repeat(MAX_CALL_BYTES / 2);
    // Native validation does not build a JSON envelope or bound escaped text.
    call.validate().unwrap();
    let mut value = fixture();
    value["input"]["source"] = json!(call.input.source);
    let text = value.to_string();
    assert!(text.len() > MAX_CALL_BYTES);
    assert_eq!(decode_call(&text).unwrap_err().code.as_str(), "resource_limit");
}

#[test]
fn directly_constructed_calls_revalidate_nonfinite_scalars_and_seed() {
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 1.0e10] {
        let mut call = decode_call(CALL).unwrap();
        call.input.pixel_unit = number;
        assert!(call.validate().is_err());
    }
    for point in [[f64::NAN, 1.0], [1.0, f64::INFINITY], [-1.0, 1.0]] {
        let mut call = decode_call(CALL).unwrap();
        call.input.seed = Some(point);
        assert!(call.validate().is_err());
    }
}

#[test]
fn descriptor_is_strict_complete_and_explicit_about_local_labels_and_unknown_quality() {
    let descriptor = descriptor();
    assert_eq!(descriptor["protocol"], PROTOCOL);
    assert_eq!(descriptor["operation"], "read_drawing");
    assert_eq!(descriptor["synchronous"], true);
    assert_eq!(descriptor["stateless"], true);
    assert_eq!(descriptor["max_call_bytes"], MAX_CALL_BYTES);
    assert_eq!(descriptor["max_image_bytes"], MAX_IMAGE_BYTES);
    for path in [
        "/call_schema",
        "/call_schema/properties/input",
        "/call_schema/properties/input/properties/image",
        "/output_schema",
        "/output_schema/properties/evidence",
        "/output_schema/properties/evidence/properties/evidence",
        "/output_schema/properties/provenance",
        "/output_schema/properties/quality",
        "/output_schema/properties/timing",
    ] {
        let object = descriptor.pointer(path).unwrap();
        assert_eq!(object["type"], "object");
        assert_eq!(object["additionalProperties"], false);
        let properties = object["properties"].as_object().unwrap();
        let required = object["required"].as_array().unwrap();
        let optional = usize::from(path == "/call_schema/properties/input");
        assert_eq!(properties.len(), required.len() + optional, "{path}");
        assert!(
            properties
                .keys()
                .all(|key| required.contains(&json!(key)) || (optional == 1 && key == "simplify")),
            "{path}"
        );
    }
    assert_eq!(
        descriptor["output_schema"]["properties"]["quality"]["properties"]["confidence"]["type"],
        "null"
    );
    assert!(descriptor["node_semantics"].as_str().unwrap().contains("local result label"));
    assert_eq!(descriptor["input_schema"]["properties"]["simplify"]["default"], 0);
    assert_eq!(
        descriptor["output_schema"]["properties"]["provenance"]["properties"]["simplify"]["maximum"],
        MAX_SIMPLIFY_PIXELS
    );
    metadata().validate().unwrap();
    assert_eq!(metadata().package.name, "krlyvis-api");
    assert_eq!(metadata().length_unit, krlyxd::LengthUnit::Model);
}
