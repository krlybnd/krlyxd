#![allow(clippy::unwrap_used, clippy::panic)]

use super::{serve, text_result, Server, MAX_MESSAGE_BYTES};
use krly3d_brep::planar::{decode_call, descriptor, encode_result};
use krly3d_brep_engine::build_planar;
use serde_json::{json, Value};
use std::io::{self, BufReader, Cursor, Write};

const HELLO: &str = r#"{"jsonrpc":"2.0","id":"hello","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;
const READY: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

fn ready(server: &mut Server) {
    assert!(server.handle(HELLO).unwrap().get("result").is_some());
    assert!(server.handle(READY).is_none());
}

fn rpc(server: &mut Server, method: &str, params: Value) -> Value {
    server.handle(&json!({"jsonrpc":"2.0","id":17,"method":method,"params":params}).to_string()).unwrap()
}

fn arguments() -> Value {
    json!({"protocol":"krly3d.brep.planar/1","request_id":"domain-id",
        "input":{"polygons":[[[0,0],[6,0],[6,4],[0,4]]],"height":2,
            "length_unit":"model","source":"mcp-test"}})
}

fn call(server: &mut Server, arguments: Value) -> Value {
    let response = rpc(server, "tools/call", json!({"name":"brep_planar","arguments":arguments}));
    assert!(response.get("error").is_none(), "{response}");
    response["result"].clone()
}

fn payload(result: &Value) -> Value {
    assert_eq!(result["content"][0]["type"], "text");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn lifecycle_requires_both_initialization_steps_and_uses_the_api_call_schema() {
    let mut server = Server::new();
    assert!(server.handle(READY).is_none());
    assert_eq!(rpc(&mut server, "ping", json!({}))["result"], json!({}));
    for method in ["tools/list", "tools/call"] {
        assert_eq!(rpc(&mut server, method, json!({}))["error"]["code"], -32000);
    }
    let initialized = server.handle(&HELLO.replace("2025-11-25", "2099-01-01")).unwrap();
    let result = &initialized["result"];
    assert_eq!(result["protocolVersion"], "2025-11-25");
    assert_eq!(result["capabilities"], json!({"tools":{"listChanged":false}}));
    assert_eq!(result["serverInfo"]["name"], "krly3d-brep-mcp");
    let instructions = result["instructions"].as_str().unwrap();
    for fragment in ["synchronous", "No in-flight request cancellation", "new document", "No files"] {
        assert!(instructions.contains(fragment));
    }
    assert_eq!(rpc(&mut server, "tools/list", json!({}))["error"]["code"], -32000);
    assert!(server.handle(READY).is_none());
    let listed = rpc(&mut server, "tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "brep_planar");
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
fn invalid_initialization_and_initialized_notifications_do_not_advance_state() {
    let mut server = Server::new();
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
fn malformed_requests_ids_batches_and_recursive_duplicate_keys_are_rejected() {
    let mut server = Server::new();
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
fn nested_argument_duplicates_and_escaped_aliases_never_reach_the_domain() {
    let mut server = Server::new();
    ready(&mut server);
    for input in [
        r#"{"height":0,"height":2}"#,
        r#"{"height":0,"h\u0065ight":2}"#,
        r#"{"unknown":[{"duplicate":1,"duplicate":2}]}"#,
        r#"{"polygons":[[[0,0],[1,0],[0,1]]],"source":{"a":1,"\u0061":2}}"#,
    ] {
        let message = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"brep_planar","arguments":{{"protocol":"krly3d.brep.planar/1","request_id":"dup","input":{input}}}}}}}"#
        );
        assert_eq!(server.handle(&message).unwrap()["error"]["code"], -32600);
    }
    assert_eq!(call(&mut server, arguments())["isError"], false);
}

#[test]
fn raw_fields_preserve_exact_numeric_tokens_and_the_domain_diagnostic() {
    let args = r#"{"protocol":"krly3d.brep.planar/1","request_id":"raw","input":{"polygons":[[[0,0],[6,0],[6,4],[0,4]]],"height":18446744073709551616,"length_unit":"model","source":"raw"}}"#;
    let params = format!(r#"{{"name":"brep_planar","arguments":{args}}}"#);
    super::json::validate(&params).unwrap_or_else(|_| panic!("valid JSON rejected"));
    let fields = super::json::Fields::read(&params, &["name", "arguments"])
        .unwrap_or_else(|_| panic!("raw fields rejected"));
    assert_eq!(fields.required("arguments").unwrap_or_else(|_| panic!("missing arguments")).get(), args);
    // Compare the actual call against the API's interpretation of the original
    // token. The transport must neither rewrite it nor impose geometry policy.
    let expected = decode_call(args).and_then(build_planar).and_then(|result| encode_result(&result));
    let mut server = Server::new();
    ready(&mut server);
    let request = format!(r#"{{"jsonrpc":"2.0","id":17,"method":"tools/call","params":{params}}}"#);
    let response = server.handle(&request).unwrap();
    match expected {
        Err(error) => {
            assert_eq!(response["result"]["isError"], true);
            assert_eq!(
                payload(&response["result"]),
                json!({"diagnostic":{"code":error.code.as_str(),"message":error.message}})
            );
        }
        Ok(_) => assert_eq!(response["result"]["isError"], false),
    }
}

#[test]
fn notifications_and_unsolicited_responses_are_silent_and_do_not_build() {
    let mut server = Server::new();
    ready(&mut server);
    for message in [
        r#"{"jsonrpc":"2.0","method":"ping"}"#,
        r#"{"jsonrpc":"2.0","method":"anything"}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":17}}"#,
        r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"brep_planar","arguments":{}}}"#,
        r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"unsolicited"}}"#,
    ] {
        assert!(server.handle(message).is_none());
    }
    assert_eq!(call(&mut server, arguments())["isError"], false);
    for params in [
        json!({"name":"missing","arguments":{}}),
        json!({"name":"brep_planar","arguments":[]}),
        json!({"name":"brep_planar","arguments":{},"task":{"ttl":1}}),
        json!({"name":"brep_planar","arguments":{},"_meta":null}),
    ] {
        assert_eq!(rpc(&mut server, "tools/call", params)["error"]["code"], -32602);
    }
}

#[test]
fn domain_failures_are_tool_errors_and_the_same_request_can_succeed_afterward() {
    let mut server = Server::new();
    ready(&mut server);
    for change in [
        json!({"protocol":"unknown"}),
        json!({"input":{"polygons":[],"height":2,"length_unit":"model","source":"bad"}}),
        json!({"input":{"polygons":[[[0,0],[1,0],[0,1]]],"height":0,"length_unit":"model","source":"bad"}}),
        json!({"input":{"polygons":[[[0,0],[1,1],[0,1],[1,0]]],"height":2,"length_unit":"model","source":"bad"}}),
    ] {
        let mut bad = arguments();
        for (key, value) in change.as_object().unwrap() {
            bad[key] = value.clone();
        }
        let error = decode_call(&bad.to_string()).and_then(build_planar).unwrap_err();
        let result = call(&mut server, bad);
        assert_eq!(result["isError"], true);
        assert_eq!(result["content"].as_array().unwrap().len(), 1);
        assert_eq!(
            payload(&result),
            json!({"diagnostic":{"code":error.code.as_str(),"message":error.message}})
        );
    }
    let mut previous_session = None;
    for _ in 0..2 {
        let result = call(&mut server, arguments());
        assert_eq!(result["isError"], false);
        assert_eq!(result["content"].as_array().unwrap().len(), 1);
        assert!(result.get("structuredContent").is_none());
        let value = payload(&result);
        assert_eq!(value["request_id"], "domain-id");
        assert_eq!(value["node"], "part@1");
        assert!(value["model"].as_str().unwrap().starts_with("k3d 1\n"));
        assert_eq!(value["source"], "mcp-test");
        assert!(value["provenance"].is_object());
        assert!(value["quality"].is_object());
        let session = value["provenance"]["session"].as_str().unwrap().to_owned();
        assert!(!session.is_empty());
        assert_ne!(previous_session.as_ref(), Some(&session));
        previous_session = Some(session);
    }
    assert!(call(&mut server, json!({}))["isError"].as_bool().unwrap());
    assert_eq!(call(&mut server, arguments())["isError"], false);
}

#[test]
fn canonical_text_is_not_parsed_and_reencoded_by_the_content_wrapper() {
    let built = build_planar(decode_call(&arguments().to_string()).unwrap()).unwrap();
    let canonical = encode_result(&built).unwrap();
    let wrapped = text_result(canonical.clone(), false);
    let roundtrip: Value = serde_json::from_str(&wrapped.to_string()).unwrap();
    assert_eq!(roundtrip["content"][0]["text"], canonical);
    let tokens = "{\"negative_zero\":-0.0,\"wide_integer\":18446744073709551616,\"model\":\"k3d 1\\n\"}";
    let roundtrip: Value = serde_json::from_str(&text_result(tokens.to_owned(), false).to_string()).unwrap();
    assert_eq!(roundtrip["content"][0]["text"], tokens);
}

#[test]
fn transport_recovers_from_bad_frames_and_preserves_order_with_crlf() {
    let tool = json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"brep_planar","arguments":arguments()}});
    let mut input = format!("{HELLO}\n{READY}\n{{\n").into_bytes();
    input.extend_from_slice(b"\xff\n");
    input.extend_from_slice(
        format!("{tool}\n{{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"ping\"}}\r\n").as_bytes(),
    );
    let mut output = Vec::new();
    serve(Server::new(), BufReader::with_capacity(7, Cursor::new(input)), &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.ends_with('\n'));
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
fn transport_accepts_exact_bound_and_rejects_oversize_or_missing_newline() {
    let ping = br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    let mut exact = ping.to_vec();
    exact.resize(MAX_MESSAGE_BYTES, b' ');
    exact.push(b'\n');
    let mut output = Vec::new();
    serve(Server::new(), BufReader::with_capacity(31, Cursor::new(exact)), &mut output).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&output).unwrap()["result"], json!({}));
    for frame in [
        vec![b' '; MAX_MESSAGE_BYTES + 1],
        [vec![b' '; MAX_MESSAGE_BYTES + 1], b"\n".to_vec()].concat(),
        [ping.as_slice(), b"\n", ping].concat(),
        ping.to_vec(),
    ] {
        let error =
            serve(Server::new(), BufReader::with_capacity(31, Cursor::new(frame)), Vec::new()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
    serve(Server::new(), Cursor::new(Vec::<u8>::new()), Vec::new()).unwrap();
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
    assert_eq!(serve(Server::new(), input, BrokenOutput).unwrap_err().kind(), io::ErrorKind::BrokenPipe);
}
