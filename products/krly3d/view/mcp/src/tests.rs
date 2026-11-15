#![allow(clippy::unwrap_used, clippy::panic)]

use super::{render_result, serve, Server, MAX_MESSAGE_BYTES};
use krly3d_view_api::wire::{decode_call, descriptor, encode_result};
use krly3d_view_api::{Backend, Camera, NamedView, RenderInput};
use krly3d_view_renderer::ViewEngine;
use serde_json::{json, Value};
use std::io::{self, BufReader, Cursor, Write};

const MODEL: &str = "k3d 1\nlayer geo\npart = box w=6 d=4 h=2\n";
const HELLO: &str = r#"{"jsonrpc":"2.0","id":"hello","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;
const READY: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

fn server() -> Server {
    Server::new(ViewEngine::from_k3d(MODEL).unwrap())
}

fn ready(server: &mut Server) {
    assert!(server.handle(HELLO).unwrap().get("result").is_some());
    assert!(server.handle(READY).is_none());
}

fn rpc(server: &mut Server, method: &str, params: Value) -> Value {
    server.handle(&json!({"jsonrpc":"2.0","id":17,"method":method,"params":params}).to_string()).unwrap()
}

fn arguments() -> Value {
    json!({"protocol":{"major":1,"minor":0},"api":{"major":1,"minor":0},
        "request_id":"domain-id","input":{"node":"part","width":48,"height":36}})
}

fn call(server: &mut Server, arguments: Value) -> Value {
    let response = rpc(server, "tools/call", json!({"name":"view_render","arguments":arguments}));
    assert!(response.get("error").is_none(), "{response}");
    response["result"].clone()
}

fn payload(result: &Value) -> Value {
    assert_eq!(result["content"][0]["type"], "text");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn lifecycle_negotiates_and_advertises_only_one_read_only_tool_with_full_call_schema() {
    let mut server = server();
    assert!(server.handle(READY).is_none());
    assert_eq!(rpc(&mut server, "ping", json!({}))["result"], json!({}));
    for method in ["tools/list", "tools/call"] {
        assert_eq!(rpc(&mut server, method, json!({}))["error"]["code"], -32000);
    }
    let initialized = server.handle(&HELLO.replace("2025-11-25", "2099-01-01")).unwrap();
    assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(initialized["result"]["capabilities"], json!({"tools":{"listChanged":false}}));
    assert_eq!(initialized["result"]["serverInfo"]["name"], "krly3d-view-mcp");
    assert_eq!(rpc(&mut server, "tools/list", json!({}))["error"]["code"], -32000);
    assert!(server.handle(READY).is_none());
    let listed = rpc(&mut server, "tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);
    assert_eq!(tools[0]["name"], "view_render");
    assert_eq!(tools[0]["inputSchema"], descriptor()["call_schema"]);
    assert!(tools[0]["inputSchema"].is_object());
    assert_eq!(tools[0]["annotations"], json!({"readOnlyHint":true,"openWorldHint":false}));
    assert!(tools[0].get("outputSchema").is_none());
    assert!(tools[0].get("execution").is_none());
    assert_eq!(rpc(&mut server, "tasks/list", json!({}))["error"]["code"], -32601);
    assert_eq!(rpc(&mut server, "tools/list", json!({"cursor":"bad"}))["error"]["code"], -32602);
    assert_eq!(server.handle(HELLO).unwrap()["error"]["code"], -32600);
}

#[test]
fn invalid_initialization_and_notifications_do_not_advance_the_session() {
    let mut server = server();
    for params in [
        json!({}),
        json!({"protocolVersion":"","capabilities":{},"clientInfo":{"name":"x","version":"0"}}),
        json!({"protocolVersion":5,"capabilities":{},"clientInfo":{"name":"x","version":"0"}}),
        json!({"protocolVersion":"2025-11-25","capabilities":[],"clientInfo":{"name":"x","version":"0"}}),
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":1,"version":"0"}}),
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":null}),
    ] {
        assert_eq!(rpc(&mut server, "initialize", params)["error"]["code"], -32602);
    }
    assert!(server.handle(HELLO).unwrap().get("result").is_some());
    assert!(server
        .handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{"unexpected":true}}"#)
        .is_none());
    assert_eq!(rpc(&mut server, "tools/list", json!({}))["error"]["code"], -32000);
    assert!(server.handle(READY).is_none());
    assert!(rpc(&mut server, "tools/list", json!({})).get("result").is_some());
}

#[test]
fn malformed_requests_ids_batches_and_duplicate_keys_are_rejected() {
    let mut server = server();
    ready(&mut server);
    for message in [
        "null",
        "[]",
        "[{}]",
        r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":1.5,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":true,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":{},"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":3}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"ping","extra":true}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"ping","result":{}}"#,
        r#"{"jsonrpc":"2.0","id":1,"id":2,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"ping","m\u0065thod":"tools/call"}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"_meta":{"a":1,"a":2}}}"#,
    ] {
        assert_eq!(server.handle(message).unwrap()["error"]["code"], -32600, "{message}");
    }
    for message in ["{", "{} {}", r#"{"jsonrpc":"2.0","method":"ping",}"#] {
        assert_eq!(server.handle(message).unwrap()["error"]["code"], -32700);
    }
    for id in [json!(u64::MAX), json!(i64::MIN), json!("string-id")] {
        let request = json!({"jsonrpc":"2.0","id":id,"method":"ping"});
        assert_eq!(server.handle(&request.to_string()).unwrap()["id"], id);
    }
}

#[test]
fn nested_duplicates_in_the_original_argument_text_never_reach_rendering() {
    let mut server = server();
    ready(&mut server);
    for input in [
        r#"{"node":"missing","node":"part"}"#,
        r#"{"node":"missing","n\u006fde":"part"}"#,
        r#"{"node":"part","camera":{"kind":"named","name":"front","name":"iso"}}"#,
        r#"{"node":"part","tolerance":{"value":0.01,"v\u0061lue":0.05,"unit":"model"}}"#,
        r#"{"node":"part","unexpected":[{"duplicate":1,"duplicate":2}]}"#,
    ] {
        let message = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"view_render","arguments":{{"protocol":{{"major":1,"minor":0}},"api":{{"major":1,"minor":0}},"request_id":"dup","input":{input}}}}}}}"#
        );
        assert_eq!(server.handle(&message).unwrap()["error"]["code"], -32600, "{message}");
    }
    assert_eq!(call(&mut server, arguments())["isError"], false);
}

#[test]
fn domain_decoder_receives_original_numeric_tokens() {
    // A Value roundtrip converts this out-of-range integer to floating-point
    // notation, hiding the lexical integer safety check in the domain codec.
    let args = r#"{"protocol":{"major":1,"minor":0},"api":{"major":1,"minor":0},"request_id":"raw","input":{"node":"part","width":48,"height":36,"tolerance":{"value":18446744073709551616,"unit":"model"}}}"#;
    let expected = decode_call(args).unwrap_err();
    let rewritten: Value = serde_json::from_str(args).unwrap();
    assert!(decode_call(&rewritten.to_string()).is_ok());
    let mut server = server();
    ready(&mut server);
    let request = format!(
        r#"{{"jsonrpc":"2.0","id":17,"method":"tools/call","params":{{"name":"view_render","arguments":{args}}}}}"#
    );
    let response = server.handle(&request).unwrap();
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        payload(&response["result"]),
        json!({"diagnostic":{"code":expected.code.as_str(),"message":expected.message}})
    );
}

#[test]
fn notifications_and_unsolicited_responses_are_silent_and_never_render() {
    let mut server = server();
    ready(&mut server);
    for message in [
        r#"{"jsonrpc":"2.0","method":"ping"}"#,
        r#"{"jsonrpc":"2.0","method":"anything"}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":17}}"#,
        r#"{"jsonrpc":"2.0","id":3,"result":{}}"#,
        r#"{"jsonrpc":"2.0","id":3,"error":{"code":-1,"message":"no"}}"#,
    ] {
        assert!(server.handle(message).is_none());
    }
    let silent = json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"view_render","arguments":arguments()}});
    assert!(server.handle(&silent.to_string()).is_none());
    assert_eq!(call(&mut server, arguments())["isError"], false);
}

#[test]
fn protocol_failures_and_domain_errors_have_distinct_encodings_and_allow_retry() {
    let mut server = server();
    ready(&mut server);
    for params in [
        json!({}),
        json!({"name":3}),
        json!({"name":"mesh"}),
        json!({"name":"operation_events"}),
        json!({"name":"view_render","arguments":[]}),
        json!({"name":"view_render","arguments":null}),
        json!({"name":"view_render","task":{},"arguments":arguments()}),
        json!({"name":"view_render","_meta":null}),
    ] {
        assert_eq!(rpc(&mut server, "tools/call", params)["error"]["code"], -32602);
    }
    for bad in [json!({}), json!({"input":{"node":"part"}})] {
        assert_eq!(call(&mut server, bad)["isError"], true);
    }
    for change in [
        json!({"protocol":{"major":2,"minor":0}}),
        json!({"api":{"major":2,"minor":0}}),
        json!({"timeout_ms":1}),
        json!({"request_id":""}),
        json!({"input":{"node":"missing"}}),
        json!({"input":{"node":"part","width":0}}),
        json!({"input":{"node":"part","width":2049}}),
        json!({"input":{"node":"part","backend":"other"}}),
        json!({"input":{"node":"part","camera":{"kind":"named","name":"unknown"}}}),
        json!({"input":{"node":"part","tolerance":{"value":0,"unit":"model"}}}),
        json!({"input":{"node":"part","camera":{"kind":"orthographic","toward":[0,1,0],"up":[0,2,0]}}}),
    ] {
        let mut bad = arguments();
        for (key, value) in change.as_object().unwrap() {
            bad[key] = value.clone();
        }
        let expected = decode_call(&bad.to_string()).and_then(|call| server.engine.call(call)).unwrap_err();
        let result = call(&mut server, bad);
        assert_eq!(result["isError"], true);
        assert_eq!(result["content"].as_array().unwrap().len(), 1);
        assert_eq!(
            payload(&result),
            json!({"diagnostic":{"code":expected.code.as_str(),"message":expected.message}})
        );
    }
    assert_eq!(call(&mut server, arguments())["isError"], false);
}

#[test]
fn defaults_are_domain_defaults_and_render_both_backends_as_text_and_png() {
    let minimal = json!({"protocol":{"major":1,"minor":0},"api":{"major":1,"minor":0},
        "request_id":"defaults","input":{"node":"part"}});
    let decoded = decode_call(&minimal.to_string()).unwrap();
    assert_eq!(decoded.input, RenderInput::new("part"));
    assert_eq!(decoded.input.backend, Backend::Raycast);
    assert_eq!(decoded.input.camera, Camera::Named(NamedView::Iso));
    let mut server = server();
    ready(&mut server);
    for backend in ["raycast", "mesh"] {
        let mut args = arguments();
        args["input"]["backend"] = json!(backend);
        let result = call(&mut server, args);
        assert_eq!(result["isError"], false, "{result}");
        assert!(result.get("structuredContent").is_none());
        assert_eq!(result["content"].as_array().unwrap().len(), 2);
        let data = payload(&result);
        assert_eq!(data["protocol"], json!({"major":1,"minor":0}));
        assert_eq!(data["api"], json!({"major":1,"minor":0}));
        assert_eq!(data["request_id"], "domain-id");
        assert_eq!((data["width"].as_u64(), data["height"].as_u64()), (Some(48), Some(36)));
        assert_eq!(data["provenance"]["input"]["node"], "part@1");
        assert_eq!(data["provenance"]["input"]["backend"], backend);
        assert!(!data["faces"].as_array().unwrap().is_empty());
        assert!(data["quality"].is_object());
        assert!(data["timing"].is_object());
        assert_eq!(data["image"]["mime_type"], "image/png");
        assert_eq!(data["image"]["encoding"], "base64");
        assert_eq!(
            result["content"][1],
            json!({"type":"image","mimeType":"image/png","data":data["image"]["data"]})
        );
        assert!(data["image"]["data"].as_str().unwrap().starts_with("iVBORw0KGgo"));
    }
}

#[test]
fn canonical_result_text_survives_the_outer_json_roundtrip_byte_for_byte() {
    let engine = ViewEngine::from_k3d(MODEL).unwrap();
    let call = decode_call(&arguments().to_string()).unwrap();
    let mut result = engine.call(call).unwrap();
    result.timing.render_ms = f64::from_bits(0x3fd5_5555_5555_5555);
    result.timing.encoding_ms = -0.0;
    let expected = encode_result(&result).unwrap();
    let content = render_result(&result).unwrap();
    let roundtrip: Value = serde_json::from_str(&content.to_string()).unwrap();
    assert_eq!(roundtrip["content"][0]["text"], expected);
    let mut reader = png::Decoder::new(Cursor::new(&result.png)).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((frame.width, frame.height), (48, 36));
    assert!(pixels.iter().any(|byte| *byte != 255));
}

#[test]
fn transport_recovers_from_parse_errors_and_preserves_request_order_through_render() {
    let tool = json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"view_render","arguments":arguments()}});
    let mut input = format!("{HELLO}\n{READY}\n{{\n").into_bytes();
    input.extend_from_slice(b"\xff\n");
    input.extend_from_slice(
        format!("{tool}\n{{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"ping\"}}\r\n").as_bytes(),
    );
    let mut output = Vec::new();
    serve(server(), BufReader::with_capacity(7, Cursor::new(input)), &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    let responses: Vec<Value> = output.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(responses.len(), 5);
    assert_eq!(responses[0]["id"], "hello");
    for response in &responses[1..3] {
        assert_eq!(response["error"]["code"], -32700);
    }
    assert_eq!(responses[3]["id"], 7);
    assert_eq!(responses[3]["result"]["isError"], false);
    assert_eq!(responses[4], json!({"jsonrpc":"2.0","id":8,"result":{}}));
}

#[test]
fn transport_accepts_exact_limit_and_rejects_oversize_or_unterminated_frames() {
    let ping = br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    let mut exact = ping.to_vec();
    exact.resize(MAX_MESSAGE_BYTES, b' ');
    exact.push(b'\n');
    let mut output = Vec::new();
    serve(server(), BufReader::with_capacity(31, Cursor::new(exact)), &mut output).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&output).unwrap()["result"], json!({}));
    for frame in [vec![b' '; MAX_MESSAGE_BYTES + 1], [ping.as_slice(), b"\n", ping].concat()] {
        let error =
            serve(server(), BufReader::with_capacity(31, Cursor::new(frame)), Vec::new()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
    let oversize = [vec![b' '; MAX_MESSAGE_BYTES + 1], b"\n".to_vec()].concat();
    assert_eq!(
        serve(server(), Cursor::new(oversize), Vec::new()).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    serve(server(), Cursor::new(Vec::<u8>::new()), Vec::new()).unwrap();
}

struct BrokenOutput;

impl Write for BrokenOutput {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed output"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed output"))
    }
}

#[test]
fn transport_propagates_output_failure() {
    let input = Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n");
    assert_eq!(serve(server(), input, BrokenOutput).unwrap_err().kind(), io::ErrorKind::BrokenPipe);
}
