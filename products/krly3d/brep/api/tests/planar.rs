#![allow(clippy::unwrap_used)]

use krly3d_brep::planar::{decode_call, descriptor, encode_result, metadata, PlanarQuality, PlanarResult};
use krly3d_brep::planar::{MAX_CALL_BYTES, MAX_REQUEST_ID_BYTES, MAX_SOURCE_BYTES, PROTOCOL};
use krly3d_brep::{Id, SnapshotIdentity};
use krlyxd::{Coordinates, Exactness, Length, LengthUnit};
use serde_json::{json, Value};

const CALL: &str = r#"{"protocol":"krly3d.brep.planar/1","request_id":"request-1","input":{"polygons":[[[10,-4],[16,-4],[16,0],[10,0]]],"height":2,"length_unit":"model","source":"caller:image-claim"}}"#;

fn fixture() -> Value {
    serde_json::from_str(CALL).unwrap()
}

#[test]
fn decodes_the_complete_envelope_without_rebasing_coordinates_or_source() {
    let call = decode_call(CALL).unwrap();
    assert_eq!(call.protocol, PROTOCOL);
    assert_eq!(call.request_id, "request-1");
    assert_eq!(call.input.polygons[0][0], [10.0, -4.0]);
    assert_eq!(call.input.height, 2.0);
    assert_eq!(call.input.length_unit, "model");
    assert_eq!(call.input.source, "caller:image-claim");
    call.validate().unwrap();
}

#[test]
fn rejects_unknown_duplicate_and_escaped_duplicate_keys_at_every_object_level() {
    for text in [
        CALL.replacen("\"protocol\":", "\"unexpected\":0,\"protocol\":", 1),
        CALL.replacen("\"height\":2", "\"unexpected\":0,\"height\":2", 1),
        CALL.replacen("\"request_id\":", "\"request_id\":\"first\",\"request_id\":", 1),
        CALL.replacen("\"height\":2", "\"height\":1,\"height\":2", 1),
        CALL.replacen("\"height\":2", "\"heig\\u0068t\":1,\"height\":2", 1),
        CALL.replacen("\"input\":", "\"input\":{},\"input\":", 1),
        CALL.replacen("\"polygons\":", "\"polygons\":[],\"polygons\":", 1),
        CALL.replacen("\"length_unit\":", "\"length_unit\":\"model\",\"length_unit\":", 1),
        CALL.replacen("\"source\":", "\"source\":\"first\",\"source\":", 1),
        CALL.replacen("\"protocol\":", "\"timeout_ms\":null,\"protocol\":", 1),
    ] {
        assert_eq!(decode_call(&text).unwrap_err().code.as_str(), "input", "{text}");
    }
}

#[test]
fn rejects_missing_fields_wrong_types_wrong_protocol_and_trailing_values() {
    for key in ["protocol", "request_id", "input"] {
        let mut value = fixture();
        value.as_object_mut().unwrap().remove(key);
        assert!(decode_call(&value.to_string()).is_err());
    }
    for key in ["polygons", "height", "length_unit", "source"] {
        let mut value = fixture();
        value["input"].as_object_mut().unwrap().remove(key);
        assert!(decode_call(&value.to_string()).is_err());
    }
    for text in ["null", "[]", "{}", "", "[\"krly3d.brep.planar/1\",\"id\",{}]"] {
        assert!(decode_call(text).is_err());
    }
    assert!(decode_call(&format!("{CALL} {{}}")).is_err());
    let mut value = fixture();
    value["protocol"] = json!("krly3d.brep.planar/2");
    assert_eq!(decode_call(&value.to_string()).unwrap_err().code.as_str(), "unsupported_version");
    value = fixture();
    value["input"] = json!([[[[0, 0], [1, 0], [0, 1]]], 2, "model", "source"]);
    assert!(decode_call(&value.to_string()).is_err());
}

#[test]
fn rejects_invalid_shapes_nonfinite_values_and_provider_overflow_inputs() {
    for polygons in [
        json!([]),
        json!([[]]),
        json!([[[0, 0], [1, 0]]]),
        json!([[[0, 0, 1], [1, 0], [0, 1]]]),
        json!([[[0], [1, 0], [0, 1]]]),
        json!([[["0", 0], [1, 0], [0, 1]]]),
        json!([[[true, 0], [1, 0], [0, 1]]]),
        json!([[[null, 0], [1, 0], [0, 1]]]),
        json!([[[{"x":0,"x":1}, 0], [1, 0], [0, 1]]]),
        json!([[[1e200, 0], [1, 0], [0, 1]]]),
    ] {
        let mut value = fixture();
        value["input"]["polygons"] = polygons;
        assert!(decode_call(&value.to_string()).is_err(), "{value}");
    }
    for height in [json!(0), json!(-1), json!(1e200), json!("2"), json!(null), json!(true)] {
        let mut value = fixture();
        value["input"]["height"] = height;
        assert!(decode_call(&value.to_string()).is_err(), "{value}");
    }
    for token in ["1e309", "NaN", "Infinity", "-Infinity"] {
        assert!(decode_call(&CALL.replace("\"height\":2", &format!("\"height\":{token}"))).is_err());
    }
    let mut value = fixture();
    value["input"]["length_unit"] = json!("metre");
    assert!(decode_call(&value.to_string()).is_err());
}

#[test]
fn bounds_utf8_strings_wire_bytes_and_aggregate_vertices() {
    for (key, limit) in [("request_id", MAX_REQUEST_ID_BYTES), ("source", MAX_SOURCE_BYTES)] {
        for text in [String::new(), " \n\t".into(), "a".repeat(limit + 1), "é".repeat(limit / 2 + 1)] {
            let mut value = fixture();
            if key == "source" {
                value["input"][key] = json!(text);
            } else {
                value[key] = json!(text);
            }
            assert!(decode_call(&value.to_string()).is_err());
        }
    }
    let padded = format!("{CALL}{}", " ".repeat(MAX_CALL_BYTES - CALL.len()));
    assert!(decode_call(&padded).is_ok());
    assert_eq!(decode_call(&(padded + " ")).unwrap_err().code.as_str(), "resource_limit");
    let mut value = fixture();
    value["input"]["polygons"] = json!([vec![[0, 0]; 253], [[0, 0], [1, 0], [0, 1]]]);
    assert!(decode_call(&value.to_string()).is_ok());
    value["input"]["polygons"][1].as_array_mut().unwrap().push(json!([0, 0]));
    assert_eq!(decode_call(&value.to_string()).unwrap_err().code.as_str(), "resource_limit");
}

#[test]
fn typed_calls_cannot_bypass_boundary_checks() {
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e200] {
        let mut call = decode_call(CALL).unwrap();
        call.input.height = number;
        assert!(call.validate().is_err());
        call = decode_call(CALL).unwrap();
        call.input.polygons[0][0][0] = number;
        assert!(call.validate().is_err());
    }
}

fn result() -> PlanarResult {
    PlanarResult {
        protocol: PROTOCOL.into(),
        request_id: "request-1".into(),
        model: "k3d 1\nlayer geo\n".into(),
        node: "part@1".into(),
        source: "caller:image-claim".into(),
        provenance: SnapshotIdentity {
            source: Id::new("session:part@1:key"),
            session: Id::new("session"),
            node: "part@1".into(),
            build_key: "0".repeat(32),
        },
        quality: PlanarQuality::default(),
    }
}

#[test]
fn result_encoding_preserves_provenance_and_honest_unknown_quality() {
    let result = result();
    let value: Value = serde_json::from_str(&encode_result(&result).unwrap()).unwrap();
    assert_eq!(value["protocol"], PROTOCOL);
    assert_eq!(value["model"], result.model);
    assert_eq!(value["source"], result.source);
    assert_eq!(value["provenance"]["source"], result.provenance.source.as_str());
    assert_eq!(value["provenance"]["session"], result.provenance.session.as_str());
    assert_eq!(value["provenance"]["build_key"], result.provenance.build_key);
    assert_eq!(value["quality"]["exactness"]["kind"], "exact");
    assert!(value["quality"]["confidence"].is_null());
    assert!(value["quality"]["positional_uncertainty"].is_null());
    assert!(value["quality"]["source_claim"].as_str().unwrap().contains("not a verified image hash"));
    assert!(value["quality"]["accuracy_scope"].as_str().unwrap().contains("not physical accuracy"));
}

#[test]
fn encoding_rejects_nonfinite_quality_and_inconsistent_identity() {
    let mut result = result();
    for value in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        result.quality.confidence = Some(value);
        assert!(encode_result(&result).is_err());
    }
    result.quality.confidence = None;
    result.quality.positional_uncertainty = Some(Length { value: f64::NAN, unit: LengthUnit::Model });
    assert!(encode_result(&result).is_err());
    result.quality.positional_uncertainty = None;
    result.quality.exactness = Exactness::Numeric { tolerance: f64::INFINITY };
    assert!(encode_result(&result).is_err());
    result.quality.exactness = Exactness::Sampled { samples: 0 };
    assert!(encode_result(&result).is_err());
    result.quality.exactness = Exactness::Exact;
    result.provenance.node = "other@1".into();
    assert!(encode_result(&result).is_err());
}

#[test]
fn encoding_rejects_empty_provenance_and_invalid_native_build_keys() {
    for source in ["", " "] {
        let mut value = result();
        value.provenance.source = Id::new(source);
        assert!(encode_result(&value).is_err());
        value = result();
        value.provenance.session = Id::new(source);
        assert!(encode_result(&value).is_err());
    }
    for key in [String::new(), "a".repeat(31), "a".repeat(33), "A".repeat(32), "g".repeat(32), "é".repeat(16)]
    {
        let mut value = result();
        value.provenance.build_key = key;
        assert!(encode_result(&value).is_err());
    }
}

#[test]
fn descriptor_mirrors_validated_common_metadata_and_schemas() {
    let metadata = metadata();
    metadata.validate().unwrap();
    assert_eq!(metadata.coordinates, Coordinates::RIGHT_HANDED_Z_UP);
    assert_eq!(metadata.length_unit, LengthUnit::Model);
    let value = descriptor();
    assert_eq!(value["package"], metadata.package.name);
    assert_eq!(value["package_version"], metadata.package.version);
    assert_eq!(value["engine_revision"], metadata.engine_revision);
    assert_eq!(value["protocol"], PROTOCOL);
    assert_eq!(value["api"], json!({"major": metadata.api.major, "minor": metadata.api.minor}));
    assert_eq!(value["length_unit"], "model");
    assert_eq!(value["max_call_bytes"], MAX_CALL_BYTES);
    assert_eq!(value["call_schema"]["additionalProperties"], false);
    assert_eq!(value["input_schema"]["additionalProperties"], false);
    assert_eq!(value["output_schema"]["properties"]["node"]["const"], "part@1");
    assert_eq!(value["synchronous"], true);
    assert!(value.get("lifecycle").is_none());
}
