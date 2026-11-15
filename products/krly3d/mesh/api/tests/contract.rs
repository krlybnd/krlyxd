#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_mesh::codec::{decode_call, encode_call, encode_result};
use krly3d_mesh::{descriptor, ApiVersion, ArtifactId, Exactness, Id, Length, LengthUnit, MeshArtifact};
use krly3d_mesh::{MeshCall, MeshData, MeshInput, MeshResult, Provenance, Quality, RequestId};
use krly3d_mesh::{API_VERSION, ARTIFACT_FORMAT, DEFAULT_TOLERANCE, MAX_SAFE_INTEGER, PROTOCOL_VERSION};
use serde_json::{json, Value};

fn minimal() -> Value {
    json!({"request_id": "r1", "protocol": {"major": 1, "minor": 0},
        "api": {"major": 1, "minor": 0}, "input": {"node": "part"}})
}

fn result() -> MeshResult {
    MeshResult {
        points: 0,
        triangles: 0,
        volume: 0.0,
        area: 0.0,
        open: 0,
        quality: Quality {
            exactness: Exactness::Sampled { samples: 0 },
            positional_uncertainty: None,
            confidence: None,
        },
        provenance: Provenance {
            package_version: "0.1.0".into(),
            engine_revision: Id::new("fixture"),
            source: Id::new("fixture"),
            input: MeshInput::new("part@1"),
        },
        mesh: None,
    }
}

#[test]
fn defaults_round_trip_as_explicit_model_units() {
    let call = decode_call(&minimal().to_string()).unwrap();
    assert_eq!(call.protocol, PROTOCOL_VERSION);
    assert_eq!(call.api, API_VERSION);
    assert_eq!(call.input, MeshInput::new("part"));
    assert_eq!(call.input.tolerance.value, DEFAULT_TOLERANCE);
    assert_eq!(call.timeout_ms, None);
    assert_eq!(decode_call(&encode_call(&call).unwrap()).unwrap(), call);
    let mut call = call;
    call.input.triangles = true;
    call.input.tolerance.value = 0.01;
    assert_eq!(decode_call(&encode_call(&call).unwrap()).unwrap(), call);
}

#[test]
fn only_exact_supported_major_and_minor_versions_are_accepted() {
    for field in ["protocol", "api"] {
        for version in [json!({"major": 2, "minor": 0}), json!({"major": 1, "minor": 1})] {
            let mut wire = minimal();
            wire[field] = version;
            assert_eq!(decode_call(&wire.to_string()).unwrap_err().code.as_str(), "unsupported_version");
        }
        for bad in [
            json!({"major": 65536, "minor": 0}),
            json!({"major": 1.0, "minor": 0}),
            json!({"major": -1, "minor": 0}),
            json!({"major": 1}),
            json!([1, 0]),
        ] {
            let mut wire = minimal();
            wire[field] = bad;
            assert!(decode_call(&wire.to_string()).is_err());
        }
    }
    let mut call = MeshCall::new(RequestId::new("r1"), MeshInput::new("part"));
    call.api = ApiVersion { major: 1, minor: 1 };
    assert!(encode_call(&call).is_err());
}

#[test]
fn unknown_fields_at_every_level_and_unknown_units_are_rejected() {
    for pointer in ["", "/input", "/api", "/protocol"] {
        let mut wire = minimal();
        wire.pointer_mut(pointer).unwrap()["extra"] = json!(true);
        assert!(decode_call(&wire.to_string()).unwrap_err().message.contains("unknown field"));
    }
    for unit in ["metre", "mm", "Model", ""] {
        let mut wire = minimal();
        wire["input"]["tolerance"] = json!({"value": 0.05, "unit": unit});
        assert!(decode_call(&wire.to_string()).is_err(), "{unit}");
    }
    let mut wire = minimal();
    wire["input"]["tolerance"] = json!({"value": 0.05, "unit": "model", "extra": true});
    assert!(decode_call(&wire.to_string()).unwrap_err().message.contains("unknown field"));
}

#[test]
fn duplicate_keys_are_rejected_before_they_can_be_overwritten() {
    let envelope = r#""request_id":"r1","protocol":{"major":1,"minor":0},"api":{"major":1,"minor":0}"#;
    for input in [
        r#"{"node":"part","node":"other"}"#,
        r#"{"node":"part","triangles":true,"triangles":false}"#,
        r#"{"node":"part","tolerance":{"value":0.05,"value":0.1,"unit":"model"}}"#,
    ] {
        let text = format!("{{{envelope},\"input\":{input}}}");
        assert!(decode_call(&text).unwrap_err().message.contains("duplicate field"));
    }
    let text = format!("{{{envelope},\"request_id\":\"r2\",\"input\":{{\"node\":\"part\"}}}}");
    assert!(decode_call(&text).unwrap_err().message.contains("duplicate field"));
    let text = format!("{{{envelope},\"input\":{{\"node\":\"part\"}}}}")
        .replace("\"major\":1,\"minor\":0", "\"major\":1,\"minor\":0,\"major\":1");
    assert!(decode_call(&text).unwrap_err().message.contains("duplicate field"));
}

#[test]
fn bad_types_nulls_nonfinite_and_oversized_integers_fail_closed() {
    let base = minimal().to_string();
    for token in [
        "NaN",
        "Infinity",
        "1e999",
        "0",
        "-0.1",
        "null",
        "\"0.05\"",
        "9007199254740993",
        "18446744073709551616",
    ] {
        let mut wire = minimal();
        wire["input"]["tolerance"] = json!({"value": "TOKEN", "unit": "model"});
        let text = wire.to_string().replace("\"TOKEN\"", token);
        assert!(decode_call(&text).is_err(), "accepted {token}");
    }
    for token in ["-1", "1.0", "9007199254740992", "18446744073709551616"] {
        let text = base.replacen('{', &format!("{{\"timeout_ms\":{token},"), 1);
        assert!(decode_call(&text).is_err(), "accepted timeout {token}");
    }
    for pointer in ["/request_id", "/input/node", "/input/triangles", "/input/tolerance"] {
        let mut wire = minimal();
        if pointer == "/input/triangles" || pointer == "/input/tolerance" {
            wire["input"][pointer.rsplit('/').next().unwrap()] = Value::Null;
        } else {
            *wire.pointer_mut(pointer).unwrap() = Value::Null;
        }
        assert!(decode_call(&wire.to_string()).is_err());
    }
    assert!(decode_call(&(base + " false")).is_err());
}

#[test]
fn timeout_requests_are_explicitly_unsupported_including_zero() {
    for timeout in [0, 1, MAX_SAFE_INTEGER] {
        let mut wire = minimal();
        wire["timeout_ms"] = json!(timeout);
        let error = decode_call(&wire.to_string()).unwrap_err();
        assert_eq!(error.code.as_str(), "unsupported");
        assert!(error.message.contains("controlled execution"));
    }
}

#[test]
fn direct_calls_cannot_bypass_numeric_or_unit_validation() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0] {
        let mut call = MeshCall::new(RequestId::new("r1"), MeshInput::new("part"));
        call.input.tolerance.value = value;
        assert!(call.validate().is_err());
        assert!(encode_call(&call).is_err());
    }
    let mut call = MeshCall::new(RequestId::new("r1"), MeshInput::new("part"));
    call.input.tolerance.unit = LengthUnit::Metre;
    assert!(encode_call(&call).is_err());
    call.input = MeshInput::new("part");
    call.timeout_ms = Some(u64::MAX);
    assert_eq!(call.validate().unwrap_err().code.as_str(), "input");
}

#[test]
fn result_codec_preserves_integer_counters_and_unknown_quality() {
    let mut result = result();
    result.points = MAX_SAFE_INTEGER;
    let wire: Value = serde_json::from_str(&encode_result(&result).unwrap()).unwrap();
    assert_eq!(wire["points"].as_u64(), Some(MAX_SAFE_INTEGER));
    assert_eq!(wire["quality"]["exactness"], json!({"kind": "sampled", "samples": 0}));
    assert!(wire["quality"]["positional_uncertainty"].is_null());
    assert!(wire["quality"]["confidence"].is_null());
    result.points += 1;
    assert_eq!(encode_result(&result).unwrap_err().code.as_str(), "resource_limit");
}

#[test]
fn result_codec_never_silently_turns_nonfinite_values_into_null() {
    let mut result = result();
    result.volume = f64::INFINITY;
    assert!(encode_result(&result).is_err());
    result.volume = 0.0;
    result.quality.positional_uncertainty = Some(Length { value: f64::NAN, unit: LengthUnit::Model });
    assert!(encode_result(&result).is_err());
    result.quality.positional_uncertainty = None;
    result.quality.confidence = Some(f64::NAN);
    assert!(encode_result(&result).is_err());
    result.quality.confidence = None;
    result.quality.exactness = Exactness::Numeric { tolerance: f64::NAN };
    assert!(encode_result(&result).is_err());
    result.quality.exactness = Exactness::Exact;
    result.mesh = Some(MeshArtifact {
        id: ArtifactId::new("fixture"),
        revision: 1,
        data: MeshData { points: vec![[0.0, f64::NAN, 0.0]], triangles: vec![] },
    });
    assert!(encode_result(&result).is_err());
}

#[test]
fn quality_encoder_and_descriptor_agree_on_uncertainty_and_confidence_ranges() {
    let mut result = result();
    for value in [0.0, 0.1] {
        result.quality.positional_uncertainty = Some(Length { value, unit: LengthUnit::Model });
        assert!(encode_result(&result).is_ok());
    }
    for length in
        [Length { value: -0.1, unit: LengthUnit::Model }, Length { value: 0.0, unit: LengthUnit::Metre }]
    {
        result.quality.positional_uncertainty = Some(length);
        assert!(encode_result(&result).is_err());
    }
    result.quality.positional_uncertainty = None;
    for value in [0.0, 0.5, 1.0] {
        result.quality.confidence = Some(value);
        assert!(encode_result(&result).is_ok());
    }
    for value in [-0.01, 1.01] {
        result.quality.confidence = Some(value);
        assert!(encode_result(&result).is_err());
    }
    let schema = &descriptor().output_schema;
    let uncertainty = &schema["properties"]["quality"]["properties"]["positional_uncertainty"]["anyOf"][1];
    assert_eq!(uncertainty["properties"]["value"]["minimum"], 0);
    assert!(uncertainty["properties"]["value"].get("exclusiveMinimum").is_none());
    let confidence = &schema["properties"]["quality"]["properties"]["confidence"];
    assert_eq!(confidence["minimum"], 0);
    assert_eq!(confidence["maximum"], 1);
}

#[test]
fn descriptor_truthfully_advertises_only_the_synchronous_contract() {
    let metadata = descriptor();
    assert_eq!(metadata.package.as_str(), "krly3d-mesh");
    assert_eq!(metadata.package_version, env!("CARGO_PKG_VERSION"));
    assert!(metadata.synchronous && metadata.read_only);
    assert!(!metadata.cancellation && !metadata.timeouts);
    assert!(metadata.supported_events.is_empty());
    assert_eq!(metadata.coordinates, "right-handed, Z-up world space");
    assert_eq!(metadata.artifact_format, ARTIFACT_FORMAT);
    assert_eq!(
        metadata.input_schema["properties"]["tolerance"]["default"],
        json!({"value": 0.05, "unit": "model"})
    );
    assert_eq!(metadata.input_schema["properties"]["triangles"]["default"], false);
    assert_eq!(metadata.call_schema["properties"]["timeout_ms"]["type"], "null");
    assert_eq!(metadata.output_schema["properties"]["points"]["maximum"], MAX_SAFE_INTEGER);
}
