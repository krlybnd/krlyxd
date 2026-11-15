#![allow(clippy::unwrap_used)]
use krly3d_view_api::{operation::*, RenderInput, RequestId};
#[test]
fn strict_versioned_codec_preserves_numbers_and_rejects_ambiguous_tokens() {
    let mut call = OperationCall::new(RequestId::new("op"), RenderInput::new("part"));
    call.timeout_ms = Some(0);
    call.input.tolerance.value = 0.12345678901234566;
    let text = encode_operation_call(&call).unwrap();
    assert_eq!(decode_operation_call(&text).unwrap(), call);
    assert!(krly3d_view_api::wire::decode_call(&text).is_err());
    for bad in [
        text.replace("\"timeout_ms\":0", "\"timeout_ms\":0.0"),
        text.replace("\"timeout_ms\":0", "\"timeout_ms\":-1"),
        text.replace("\"timeout_ms\":0", "\"timeout_ms\":9007199254740992"),
        text.replace("\"timeout_ms\":0", "\"timeout_ms\":0,\"timeout_ms\":1"),
        text.replace("\"major\":2", "\"major\":1"),
        text.replace("\"node\":\"part\"", "\"node\":\"part\",\"node\":\"other\""),
        text.replace("\"width\":480", "\"width\":9007199254740993"),
    ] {
        assert_ne!(bad, text);
        assert!(decode_operation_call(&bad).is_err(), "{bad}");
    }
    assert_eq!(
        descriptor()["supported_events"],
        serde_json::json!(["started", "completed", "failed", "cancelled"])
    );
    assert_eq!(descriptor()["call_schema"]["properties"]["protocol"]["properties"]["major"]["const"], 2);
}
