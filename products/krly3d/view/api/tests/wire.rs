#![allow(clippy::unwrap_used)]
use krly3d_view_api::{wire, Backend, Camera, NamedView, RenderCall, RenderInput, RequestId};
use serde_json::{json, Value};

fn call(input: &str) -> String {
    format!(
        r#"{{"protocol":{{"major":1,"minor":0}},"api":{{"major":1,"minor":0}},"request_id":"test","input":{input}}}"#
    )
}

#[test]
fn minimal_defaults_and_all_camera_forms_round_trip() {
    let decoded = wire::decode_call(&call(r#"{"node":"part"}"#)).unwrap();
    assert_eq!(decoded, RenderCall::new(RequestId::new("test"), RenderInput::new("part")));
    let cameras = [
        Camera::Named(NamedView::Back),
        Camera::Orthographic { toward: [1., 2., 3.], up: [0., 0., 1.] },
        Camera::Pinhole {
            eye: [0., -10., 5.],
            toward: [0., 1., 0.],
            up: [0., 0., 1.],
            focal: 100.,
            centre: Some([20., 30.]),
        },
    ];
    for camera in cameras {
        let mut call = decoded.clone();
        call.input.camera = camera;
        call.input.backend = Backend::Mesh;
        assert_eq!(wire::decode_call(&wire::encode_call(&call).unwrap()).unwrap(), call);
    }
}

#[test]
fn rejects_unknown_duplicate_null_and_wrong_shape_at_every_boundary() {
    for input in [
        r#"{"node":"part","node":"other"}"#,
        r#"{"node":"part","width":3,"width":4}"#,
        r#"{"node":"part","extra":1}"#,
        r#"{"node":"part","camera":null}"#,
        r#"["part"]"#,
        r#"{"node":"part","camera":{"kind":"named","kind":"named","name":"iso"}}"#,
        r#"{"node":"part","camera":{"kind":"named","name":"iso","focal":10}}"#,
        r#"{"node":"part","camera":{"kind":"pinhole","eye":[0,0,0],"toward":[1,0,0],"up":[0,0,1],"focal":10,"centre":null}}"#,
        r#"{"node":"part","tolerance":{"value":0.1,"value":0.2,"unit":"model"}}"#,
        r#"{"node":"part","tolerance":[0.1,"model"]}"#,
    ] {
        assert!(wire::decode_call(&call(input)).is_err(), "{input}");
    }
    let valid = call(r#"{"node":"part"}"#);
    for text in [
        valid.replace("\"major\":1", "\"major\":1,\"major\":1"),
        valid.replace("\"minor\":0", "\"minor\":0,\"future\":0"),
        valid.replace("\"input\":", "\"timeout_ms\":10,\"input\":"),
        format!("{valid} null"),
    ] {
        assert!(wire::decode_call(&text).is_err(), "{text}");
    }
}

#[test]
fn rejects_lossy_integer_tokens_and_invalid_dimensions_units_vectors() {
    for input in [
        r#"{"node":"p","width":1.0}"#,
        r#"{"node":"p","height":2049}"#,
        r#"{"node":"p","height":0}"#,
        r#"{"node":"p","tolerance":{"value":9007199254740993,"unit":"model"}}"#,
        r#"{"node":"p","tolerance":{"value":184467440737095516160,"unit":"model"}}"#,
        r#"{"node":"p","tolerance":{"value":1e999,"unit":"model"}}"#,
        r#"{"node":"p","tolerance":{"value":0.1,"unit":"metre"}}"#,
        r#"{"node":"p","camera":{"kind":"orthographic","toward":[9007199254740993,1,1],"up":[0,0,1]}}"#,
        r#"{"node":"p","camera":{"kind":"orthographic","toward":[0,0,0],"up":[0,0,1]}}"#,
    ] {
        assert!(wire::decode_call(&call(input)).is_err(), "{input}");
    }
    let mut value: Value = serde_json::from_str(&call(r#"{"node":"p"}"#)).unwrap();
    value["api"]["minor"] = json!(1);
    assert_eq!(wire::decode_call(&value.to_string()).unwrap_err().code.as_str(), "unsupported_version");
}

#[test]
fn descriptor_advertises_only_synchronous_implemented_features() {
    let descriptor = wire::descriptor();
    assert_eq!(descriptor["synchronous"], true);
    assert_eq!(descriptor["cancellation"], false);
    assert_eq!(descriptor["timeouts"], false);
    assert_eq!(descriptor["supported_events"], json!([]));
    assert_eq!(descriptor["call_schema"]["additionalProperties"], false);
}

#[test]
fn output_preserves_unknown_quality_and_rejects_nonfinite_or_inconsistent_metadata() {
    use krly3d_view_api::{
        Exactness, FaceCoverage, Id, Provenance, Quality, RenderResult, SnapshotIdentity, Timing,
    };
    let input = RenderInput::new("part@1");
    let valid = RenderResult {
        request_id: RequestId::new("result"),
        width: input.width,
        height: input.height,
        png: b"\x89PNG\r\n\x1a\n".to_vec(),
        faces: vec![FaceCoverage { name: "top".into(), pixels: 1 }],
        quality: Quality {
            exactness: Exactness::Sampled { samples: input.width * input.height },
            requested_tolerance: None,
            positional_uncertainty: None,
            confidence: None,
        },
        provenance: Provenance {
            source: SnapshotIdentity {
                source: Id::new("opaque"),
                session: Id::new("session"),
                node: "part@1".into(),
                build_key: "0".repeat(32),
            },
            input,
            engine_revision: Id::new("test"),
        },
        timing: Timing { render_ms: 1., encoding_ms: 0.5 },
    };
    let value: Value = serde_json::from_str(&wire::encode_result(&valid).unwrap()).unwrap();
    assert!(value["quality"]["confidence"].is_null());
    assert!(value["quality"]["positional_uncertainty"].is_null());
    assert_eq!(value["image"]["data"], "iVBORw0KGgo=");
    let mut changed = valid.clone();
    changed.timing.render_ms = f64::NAN;
    assert!(wire::encode_result(&changed).is_err());
    changed = valid.clone();
    changed.width += 1;
    assert!(wire::encode_result(&changed).is_err());
    changed = valid.clone();
    changed.faces[0].pixels = u64::MAX;
    assert!(wire::encode_result(&changed).is_err());
    changed = valid;
    changed.quality.confidence = Some(f64::INFINITY);
    assert!(wire::encode_result(&changed).is_err());
}
