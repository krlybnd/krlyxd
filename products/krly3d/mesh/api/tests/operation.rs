#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_mesh::codec::{decode_call, encode_result};
use krly3d_mesh::operation::{
    decode_operation_call, descriptor, encode_event, encode_operation_call, MeshEvent, MeshEventBody,
    OperationCall, OperationId, API_VERSION, PROTOCOL_VERSION,
};
use krly3d_mesh::{
    diagnostic, ApiVersion, ArtifactId, Exactness, Id, Length, LengthUnit, MeshArtifact, MeshData, MeshInput,
    MeshResult, Provenance, Quality, RequestId, MAX_SAFE_INTEGER,
};
use serde_json::{json, Value};

fn minimal() -> Value {
    json!({"request_id": "r1", "protocol": {"major": 2, "minor": 0},
        "api": {"major": 2, "minor": 0}, "input": {"node": "part"}})
}

fn provenance() -> Provenance {
    Provenance {
        package_version: env!("CARGO_PKG_VERSION").into(),
        engine_revision: Id::new("fixture-engine/1"),
        source: Id::new("session/part@1/build"),
        input: MeshInput::new("part@1"),
    }
}

fn result() -> MeshResult {
    MeshResult {
        points: 3,
        triangles: 1,
        volume: 0.0,
        area: 0.5,
        open: 3,
        quality: Quality {
            exactness: Exactness::Sampled { samples: 0 },
            positional_uncertainty: None,
            confidence: None,
        },
        provenance: provenance(),
        mesh: Some(MeshArtifact {
            id: ArtifactId::new("artifact/mesh"),
            revision: 1,
            data: MeshData {
                points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                triangles: vec![[0, 1, 2]],
            },
        }),
    }
}

fn event(body: MeshEventBody) -> MeshEvent {
    MeshEvent {
        request_id: RequestId::new("r1"),
        operation_id: OperationId::new("session/op1"),
        seq: 0,
        body,
    }
}

fn encoded_body(body: MeshEventBody) -> Value {
    serde_json::from_str::<Value>(&encode_event(&event(body)).unwrap()).unwrap()["body"].clone()
}

#[test]
fn new_and_decoded_calls_default_to_v2_and_reuse_mesh_input_defaults() {
    let call = OperationCall::new(RequestId::new("r1"), MeshInput::new("part"));
    assert_eq!(call.protocol, ApiVersion { major: 2, minor: 0 });
    assert_eq!(call.api, ApiVersion { major: 2, minor: 0 });
    assert_eq!(call.protocol, PROTOCOL_VERSION);
    assert_eq!(call.api, API_VERSION);
    assert_eq!(call.timeout_ms, None);
    assert_eq!(decode_operation_call(&minimal().to_string()).unwrap(), call);
    let encoded = encode_operation_call(&call).unwrap();
    assert_eq!(decode_operation_call(&encoded).unwrap(), call);
    let wire: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(wire["input"]["tolerance"], json!({"value": 0.05, "unit": "model"}));
    assert_eq!(wire["input"]["triangles"], false);
    assert!(wire["timeout_ms"].is_null());
}

#[test]
fn absent_null_zero_and_json_safe_timeouts_round_trip() {
    for timeout in [None, Some(0), Some(1), Some(MAX_SAFE_INTEGER)] {
        let mut wire = minimal();
        wire["timeout_ms"] = json!(timeout);
        let call = decode_operation_call(&wire.to_string()).unwrap();
        assert_eq!(call.timeout_ms, timeout);
        assert_eq!(decode_operation_call(&encode_operation_call(&call).unwrap()).unwrap(), call);
    }
    assert_eq!(decode_operation_call(&minimal().to_string()).unwrap().timeout_ms, None);
}

#[test]
fn timeout_decoder_rejects_noninteger_unsafe_and_malformed_tokens() {
    let base = minimal().to_string();
    for token in [
        "-1",
        "1.0",
        "0.0",
        "1e0",
        "true",
        "\"1\"",
        "{}",
        "[]",
        "NaN",
        "Infinity",
        "1e999",
        "9007199254740992",
        "9007199254740993",
        "18446744073709551616",
    ] {
        let wire = base.replacen('{', &format!("{{\"timeout_ms\":{token},"), 1);
        assert!(decode_operation_call(&wire).is_err(), "accepted timeout {token}");
    }
    for timeout in [MAX_SAFE_INTEGER + 1, u64::MAX] {
        let mut call = OperationCall::new(RequestId::new("r1"), MeshInput::new("part"));
        call.timeout_ms = Some(timeout);
        assert_eq!(call.validate().unwrap_err().code.as_str(), "input");
        assert!(encode_operation_call(&call).is_err());
    }
}

#[test]
fn only_exact_v2_versions_are_accepted_and_v1_stays_synchronous() {
    for field in ["api", "protocol"] {
        for version in [
            json!({"major": 1, "minor": 0}),
            json!({"major": 2, "minor": 1}),
            json!({"major": 3, "minor": 0}),
        ] {
            let mut wire = minimal();
            wire[field] = version;
            assert_eq!(
                decode_operation_call(&wire.to_string()).unwrap_err().code.as_str(),
                "unsupported_version"
            );
        }
        for version in [
            json!({"major": 2.0, "minor": 0}),
            json!({"major": -2, "minor": 0}),
            json!({"major": 65536, "minor": 0}),
            json!({"major": 2, "minor": null}),
            json!({"major": 2}),
            json!([2, 0]),
            Value::Null,
        ] {
            let mut wire = minimal();
            wire[field] = version;
            assert!(decode_operation_call(&wire.to_string()).is_err());
        }
    }
    let mut call = OperationCall::new(RequestId::new("r1"), MeshInput::new("part"));
    call.protocol = krly3d_mesh::PROTOCOL_VERSION;
    assert!(encode_operation_call(&call).is_err());
    call.protocol = PROTOCOL_VERSION;
    call.api = krly3d_mesh::API_VERSION;
    assert!(encode_operation_call(&call).is_err());
    assert_eq!(decode_call(&minimal().to_string()).unwrap_err().code.as_str(), "unsupported_version");
    let mut v1 = minimal();
    for field in ["protocol", "api"] {
        v1[field]["major"] = json!(1);
    }
    assert!(decode_call(&v1.to_string()).is_ok());
    assert!(decode_operation_call(&v1.to_string()).is_err());
    for timeout in [0, 1, MAX_SAFE_INTEGER] {
        v1["timeout_ms"] = json!(timeout);
        assert_eq!(decode_call(&v1.to_string()).unwrap_err().code.as_str(), "unsupported");
    }
}

#[test]
fn unknown_fields_are_rejected_at_every_level() {
    for pointer in ["", "/input", "/protocol", "/api", "/input/tolerance"] {
        let mut wire = minimal();
        wire["input"]["tolerance"] = json!({"value": 0.05, "unit": "model"});
        wire.pointer_mut(pointer).unwrap()["extra"] = json!(false);
        assert!(decode_operation_call(&wire.to_string()).unwrap_err().message.contains("unknown field"));
    }
}

#[test]
fn duplicate_fields_are_rejected_before_values_can_be_overwritten() {
    let base = minimal().to_string();
    for field in ["request_id", "protocol", "api", "input", "timeout_ms"] {
        let wire = base.replacen('{', &format!("{{\"{field}\":null,\"{field}\":null,"), 1);
        assert!(decode_operation_call(&wire).unwrap_err().message.contains("duplicate field"), "{field}");
    }
    for input in [
        r#"{"node":"part","node":"other"}"#,
        r#"{"node":"part","triangles":true,"triangles":false}"#,
        r#"{"node":"part","tolerance":{"value":0.1,"unit":"model"},"tolerance":null}"#,
        r#"{"node":"part","tolerance":{"value":0.1,"value":0.2,"unit":"model"}}"#,
        r#"{"node":"part","tolerance":{"value":0.1,"unit":"metre","unit":"model"}}"#,
    ] {
        let wire = base.replace(r#"{"node":"part"}"#, input);
        assert!(decode_operation_call(&wire).unwrap_err().message.contains("duplicate field"), "{input}");
    }
    for field in ["major", "minor"] {
        let wire =
            base.replace(r#"{"major":2,"minor":0}"#, &format!("{{\"major\":2,\"minor\":0,\"{field}\":0}}"));
        assert!(decode_operation_call(&wire).unwrap_err().message.contains("duplicate field"));
    }
}

#[test]
fn missing_required_fields_bad_types_and_trailing_data_fail_closed() {
    for field in ["request_id", "protocol", "api", "input"] {
        let mut wire = minimal();
        wire.as_object_mut().unwrap().remove(field);
        assert!(decode_operation_call(&wire.to_string()).is_err(), "missing {field}");
    }
    for pointer in ["/request_id", "/protocol", "/api", "/input", "/input/node"] {
        for invalid in [Value::Null, json!(true), json!(12), json!([])] {
            let mut wire = minimal();
            *wire.pointer_mut(pointer).unwrap() = invalid;
            assert!(decode_operation_call(&wire.to_string()).is_err(), "{pointer}");
        }
    }
    let mut wire = minimal();
    wire["request_id"] = json!("");
    assert!(decode_operation_call(&wire.to_string()).is_err());
    wire = minimal();
    wire["input"]["triangles"] = Value::Null;
    assert!(decode_operation_call(&wire.to_string()).is_err());
    for wire in ["null".into(), "[]".into(), minimal().to_string() + " false"] {
        assert!(decode_operation_call(&wire).is_err());
    }
}

#[test]
fn mesh_input_validation_is_shared_for_direct_and_decoded_calls() {
    let mut call = OperationCall::new(RequestId::new("r1"), MeshInput::new("part"));
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        call.input.tolerance.value = value;
        assert!(call.validate().is_err());
        assert!(encode_operation_call(&call).is_err());
    }
    call.input = MeshInput::new("part");
    call.input.tolerance.unit = LengthUnit::Metre;
    assert!(call.validate().is_err());
    call.input = MeshInput::new("");
    assert!(call.validate().is_err());
    call.input = MeshInput::new("part");
    call.request_id = RequestId::new("");
    assert!(encode_operation_call(&call).is_err());
    for token in ["0", "-0.1", "null", "\"0.05\"", "1e999", "9007199254740993", "NaN"] {
        let mut wire = minimal();
        wire["input"]["tolerance"] = json!({"value": "TOKEN", "unit": "model"});
        assert!(decode_operation_call(&wire.to_string().replace("\"TOKEN\"", token)).is_err());
    }
    for unit in ["metre", "mm", "Model", ""] {
        let mut wire = minimal();
        wire["input"]["tolerance"] = json!({"value": 0.05, "unit": unit});
        assert!(decode_operation_call(&wire.to_string()).is_err());
    }
}

#[test]
fn all_four_events_encode_exact_envelopes_and_payloads() {
    let provenance = provenance();
    let result = result();
    let started = json!({"kind": "started", "provenance": {
        "package_version": provenance.package_version, "engine_revision": provenance.engine_revision.as_str(),
        "source": provenance.source.as_str(), "input": {
            "node": "part@1", "tolerance": {"value": 0.05, "unit": "model"}, "triangles": false,
        },
    }});
    let completed = json!({"kind": "completed", "result": serde_json::from_str::<Value>(&encode_result(&result).unwrap()).unwrap()});
    let failed = json!({"kind": "failed", "diagnostic": {"code": "deadline", "message": "expired\n\"now\""}});
    for (index, (body, expected)) in [
        (MeshEventBody::Started(provenance), started),
        (MeshEventBody::Completed(result), completed),
        (MeshEventBody::Failed(diagnostic("deadline", "expired\n\"now\"")), failed),
        (MeshEventBody::Cancelled, json!({"kind": "cancelled"})),
    ]
    .into_iter()
    .enumerate()
    {
        let mut event = event(body);
        event.seq = index as u64;
        let encoded = encode_event(&event).unwrap();
        assert_eq!(encode_event(&event).unwrap(), encoded);
        assert_eq!(
            serde_json::from_str::<Value>(&encoded).unwrap(),
            json!({
                "api": {"major": 2, "minor": 0}, "protocol": {"major": 2, "minor": 0},
                "request_id": "r1", "operation_id": "session/op1", "seq": index, "body": expected,
            })
        );
    }
}

#[test]
fn events_require_json_safe_sequence_numbers_and_nonempty_ids() {
    let mut event = event(MeshEventBody::Cancelled);
    event.seq = MAX_SAFE_INTEGER;
    let wire: Value = serde_json::from_str(&encode_event(&event).unwrap()).unwrap();
    assert_eq!(wire["seq"].as_u64(), Some(MAX_SAFE_INTEGER));
    for seq in [MAX_SAFE_INTEGER + 1, u64::MAX] {
        event.seq = seq;
        assert_eq!(encode_event(&event).unwrap_err().code.as_str(), "resource_limit");
    }
    event.seq = 0;
    event.request_id = RequestId::new("");
    assert!(encode_event(&event).is_err());
    event.request_id = RequestId::new("r1");
    event.operation_id = OperationId::new("");
    assert!(encode_event(&event).is_err());
    assert!(encode_event(&self::event(MeshEventBody::Failed(diagnostic("", "bad")))).is_err());
}

#[test]
fn started_and_completed_require_valid_complete_provenance() {
    let mut bad = Vec::new();
    let mut value = provenance();
    value.package_version.clear();
    bad.push(value);
    let mut value = provenance();
    value.engine_revision = Id::new("");
    bad.push(value);
    let mut value = provenance();
    value.source = Id::new("");
    bad.push(value);
    let mut value = provenance();
    value.input.node.clear();
    bad.push(value);
    let mut value = provenance();
    value.input.tolerance.value = f64::NAN;
    bad.push(value);
    let mut value = provenance();
    value.input.tolerance.unit = LengthUnit::Metre;
    bad.push(value);
    for provenance in bad {
        assert!(encode_event(&event(MeshEventBody::Started(provenance.clone()))).is_err());
        let mut result = result();
        result.provenance = provenance;
        assert!(encode_event(&event(MeshEventBody::Completed(result))).is_err());
    }
}

#[test]
fn completion_preserves_exactness_unknown_quality_and_integer_counts() {
    for exactness in
        [Exactness::Exact, Exactness::Numeric { tolerance: 0.002 }, Exactness::Sampled { samples: u32::MAX }]
    {
        let mut result = result();
        result.points = MAX_SAFE_INTEGER;
        result.quality.exactness = exactness;
        let expected: Value = serde_json::from_str(&encode_result(&result).unwrap()).unwrap();
        let wire = encoded_body(MeshEventBody::Completed(result));
        assert_eq!(wire["result"], expected);
        assert_eq!(wire["result"]["points"].as_u64(), Some(MAX_SAFE_INTEGER));
        assert!(wire["result"]["quality"]["confidence"].is_null());
        assert!(wire["result"]["quality"]["positional_uncertainty"].is_null());
    }
}

#[test]
fn completion_embeds_the_result_codec_output_without_reparsing_numbers() {
    use serde_json::value::RawValue;
    use std::collections::BTreeMap;

    // Exercise deterministic bit patterns across the full finite exponent range.
    let mut bits = 0x6a09_e667_f3bc_c909_u64;
    for _ in 0..512 {
        bits = bits.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let volume = f64::from_bits(bits);
        if !volume.is_finite() {
            continue;
        }
        let mut result = result();
        result.volume = volume;
        let expected = encode_result(&result).unwrap();
        let encoded = encode_event(&event(MeshEventBody::Completed(result))).unwrap();
        let envelope: BTreeMap<String, Box<RawValue>> = serde_json::from_str(&encoded).unwrap();
        let body: BTreeMap<String, Box<RawValue>> = serde_json::from_str(envelope["body"].get()).unwrap();
        assert_eq!(body["result"].get(), expected, "changed result for volume {volume}");
    }
}

#[test]
fn completion_rejects_all_nonfinite_numeric_paths_and_unsafe_counts() {
    let mut bad = Vec::new();
    for count in [MAX_SAFE_INTEGER + 1, u64::MAX] {
        for field in ["points", "triangles", "open"] {
            let mut value = result();
            match field {
                "points" => value.points = count,
                "triangles" => value.triangles = count,
                _ => value.open = count,
            }
            bad.push(value);
        }
    }
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut value = result();
        value.volume = number;
        bad.push(value);
        let mut value = result();
        value.area = number;
        bad.push(value);
        let mut value = result();
        value.quality.exactness = Exactness::Numeric { tolerance: number };
        bad.push(value);
        let mut value = result();
        value.quality.confidence = Some(number);
        bad.push(value);
        let mut value = result();
        value.quality.positional_uncertainty = Some(Length { value: number, unit: LengthUnit::Model });
        bad.push(value);
        let mut value = result();
        value.mesh.as_mut().unwrap().data.points[0][0] = number;
        bad.push(value);
    }
    for value in bad {
        assert!(encode_event(&event(MeshEventBody::Completed(value))).is_err());
    }
}

#[test]
fn completion_rejects_invalid_quality_ranges_and_empty_artifact_ids() {
    for confidence in [-0.1, 1.1] {
        let mut result = result();
        result.quality.confidence = Some(confidence);
        assert!(encode_event(&event(MeshEventBody::Completed(result))).is_err());
    }
    for uncertainty in
        [Length { value: -0.1, unit: LengthUnit::Model }, Length { value: 0.0, unit: LengthUnit::Metre }]
    {
        let mut result = result();
        result.quality.positional_uncertainty = Some(uncertainty);
        assert!(encode_event(&event(MeshEventBody::Completed(result))).is_err());
    }
    let mut result = result();
    result.mesh.as_mut().unwrap().id = ArtifactId::new("");
    assert!(encode_event(&event(MeshEventBody::Completed(result))).is_err());
}

#[test]
fn descriptor_advertises_only_v2_operation_capabilities_and_shared_units() {
    let v1_before = krly3d_mesh::descriptor().clone();
    let v2 = descriptor();
    assert!(std::ptr::eq(descriptor(), v2));
    assert_eq!(v2.protocol, PROTOCOL_VERSION);
    assert_eq!(v2.api, API_VERSION);
    assert!(!v2.synchronous);
    assert!(v2.read_only && v2.cancellation && v2.timeouts);
    assert_eq!(v2.supported_events, ["started", "completed", "failed", "cancelled"]);
    assert_eq!(v2.artifact_format, v1_before.artifact_format);
    assert_eq!(v2.package, v1_before.package);
    assert_eq!(v2.package_version, v1_before.package_version);
    assert_eq!(v2.engine_revision, v1_before.engine_revision);
    assert_eq!(v2.operation, v1_before.operation);
    assert_eq!(v2.coordinates, v1_before.coordinates);
    assert_eq!(v2.length_unit, "model");
    assert_eq!(v2.area_unit, "model^2");
    assert_eq!(v2.volume_unit, "model^3");
    assert_eq!(v2.input_schema, v1_before.input_schema);
    assert_eq!(krly3d_mesh::descriptor(), &v1_before);
    assert!(v1_before.synchronous && v1_before.read_only);
    assert!(!v1_before.cancellation && !v1_before.timeouts);
    assert!(v1_before.supported_events.is_empty());
    assert_eq!(v1_before.call_schema["properties"]["timeout_ms"]["type"], "null");
}

#[test]
fn operation_schemas_match_the_version_timeout_and_event_encoding() {
    let v2 = descriptor();
    for field in ["protocol", "api"] {
        assert_eq!(v2.call_schema["properties"][field]["properties"]["major"]["const"], 2);
        assert_eq!(v2.call_schema["properties"][field]["properties"]["minor"]["const"], 0);
    }
    let timeout = &v2.call_schema["properties"]["timeout_ms"];
    assert_eq!(timeout["type"], json!(["null", "integer"]));
    assert_eq!(timeout["minimum"], 0);
    assert_eq!(timeout["maximum"], MAX_SAFE_INTEGER);
    let events = v2.output_schema["properties"]["body"]["oneOf"].as_array().unwrap();
    assert_eq!(events.len(), 4);
    for (event, kind) in events.iter().zip(v2.supported_events) {
        assert_eq!(event["properties"]["kind"]["const"], *kind);
        assert_eq!(event["additionalProperties"], false);
    }
    let provenance = &events[0]["properties"]["provenance"];
    assert_eq!(*provenance, events[1]["properties"]["result"]["properties"]["provenance"]);
    for field in ["package_version", "engine_revision", "source"] {
        assert_eq!(provenance["properties"][field]["minLength"], 1);
    }
}
