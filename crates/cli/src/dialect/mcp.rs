//! The Model Context Protocol over stdio: every method of the endpoint is an MCP tool.
//!
//! ```text
//! > {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18",…}}
//! > {"jsonrpc":"2.0","method":"notifications/initialized"}
//! > {"jsonrpc":"2.0","id":2,"method":"tools/list"}
//! > {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"box","arguments":{"w":60,"d":40,"h":10}}}
//! < {"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"ok box1@1 box. …"}],"isError":false}}
//! ```
//!
//! An agent reads the text of a reply; the data of a reply is not sent, except for an
//! image, which goes out as image content. A call that did not do what was asked is a
//! result marked `isError`, so that the agent reads why and tries again.

use super::Dialect;
use crate::endpoint::Endpoint;
use crate::json;
use crate::rpc::Failure;
use krly3d_kernel::schema::{Params, Value};
use krly3d_kernel::{Method, MethodKind};
use serde_json::{json, Map, Value as Json};

pub struct Mcp;

/// The revisions of the protocol this server speaks, newest first. What it uses of the
/// protocol — the handshake, ping, and tools answered with text and images — reads the
/// same in all of them.
const REVISIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// The entry of a reply's data that holds an image, and how the base64 text of every
/// PNG file starts (its eight signature bytes).
const IMAGE_ENTRY: &str = "png";
const PNG_START: &str = "iVBORw0KGgo";

impl Dialect for Mcp {
    fn name(&self) -> &'static str {
        "mcp"
    }

    fn about(&self) -> &'static str {
        "Model Context Protocol: every method of the session as a tool for an agent"
    }

    fn answer(
        &mut self,
        session: &mut dyn Endpoint,
        method: &str,
        params: Option<&Json>,
    ) -> Result<Json, Failure> {
        match method {
            "initialize" => Ok(greeting(session, params)),
            "ping" => Ok(json!({})),
            "tools/list" => tools(session),
            "tools/call" => call(session, params),
            // Whatever a client notifies of, nothing here depends on it.
            notification if notification.starts_with("notifications/") => Ok(Json::Null),
            other => Err(Failure::method_not_found(other)),
        }
    }
}

/// The answer to `initialize`: the revision the client asked for when this server speaks
/// it, else the newest one it does speak, and what the server offers.
fn greeting(session: &dyn Endpoint, params: Option<&Json>) -> Json {
    let asked = params.and_then(|params| params.get("protocolVersion")).and_then(Json::as_str);
    let revision = REVISIONS.iter().find(|revision| Some(**revision) == asked).unwrap_or(&REVISIONS[0]);
    json!({
        "protocolVersion": revision,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": session.name(), "version": env!("CARGO_PKG_VERSION") },
        "instructions": session.instructions(),
    })
}

fn tools(session: &dyn Endpoint) -> Result<Json, Failure> {
    let tools = session.methods().iter().map(tool).collect::<Result<Vec<Json>, Failure>>()?;
    Ok(json!({ "tools": tools }))
}

fn tool(method: &Method) -> Result<Json, Failure> {
    let schema = json::schema_to_json(&method.schema)
        .map_err(|misfit| Failure::internal(format!("the schema of `{}`: {misfit}", method.name)))?;
    Ok(json!({
        "name": method.name,
        "description": description(method),
        "inputSchema": schema,
        "annotations": { "readOnlyHint": method.kind == MethodKind::Query },
    }))
}

/// What the method is for, what kind of method it is and, for a tool that reads nodes,
/// how many.
fn description(method: &Method) -> String {
    let reads = match method.schema.inputs {
        (_, 0) => String::new(),
        (least, most) if least == most => format!(", reads {least} node(s)"),
        (least, most) => format!(", reads {least} to {most} nodes"),
    };
    format!("{} [{}{reads}]", method.schema.about, method.kind.name())
}

fn call(session: &mut dyn Endpoint, params: Option<&Json>) -> Result<Json, Failure> {
    let Some(Json::Object(members)) = params else {
        return Err(Failure::invalid_params("`tools/call` takes `name` and `arguments`"));
    };
    let Some(Json::String(name)) = members.get("name") else {
        return Err(Failure::invalid_params("`name` must be the name of a tool, as a string"));
    };
    if !session.methods().iter().any(|listed| listed.name == *name) {
        return Err(Failure::invalid_params(format!("Unknown tool: {name}")));
    }
    let arguments = match members.get("arguments") {
        None => Ok(Params::new()),
        Some(arguments @ Json::Object(_)) => json::params_from_json(arguments),
        Some(_) => return Err(Failure::invalid_params("`arguments` must be an object")),
    };
    // What is wrong with the arguments is for the agent to read and put right, so it
    // travels as the outcome of the tool and not as a failure of the protocol.
    Ok(match arguments {
        Ok(arguments) => {
            let reply = session.call(name, &arguments);
            said(&reply.text, image(&reply.data), !reply.ok)
        }
        Err(misfit) => said(&format!("input: {misfit}"), None, true),
    })
}

fn said(text: &str, image: Option<&str>, is_error: bool) -> Json {
    let mut content = vec![json!({ "type": "text", "text": text })];
    if let Some(image) = image {
        content.push(json!({ "type": "image", "data": image, "mimeType": "image/png" }));
    }
    let mut result = Map::new();
    result.insert("content".to_string(), Json::Array(content));
    result.insert("isError".to_string(), Json::Bool(is_error));
    Json::Object(result)
}

/// The PNG image a reply carries, as base64 text.
fn image(data: &Value) -> Option<&str> {
    let Value::Map(entries) = data else { return None };
    match entries.get(IMAGE_ENTRY) {
        Some(Value::Text(text)) if text.starts_with(PNG_START) => Some(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::Mcp;
    use crate::dialect::plain::Plain;
    use crate::dialect::testing::{exchange, session};
    use serde_json::{json, Value as Json};

    const HELLO: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;
    const READY: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

    fn ask(lines: &[&str]) -> Vec<Json> {
        exchange(&mut Mcp, &mut session(), &lines.join("\n"))
    }

    fn call(id: u32, name: &str, arguments: Json) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": { "name": name, "arguments": arguments } })
        .to_string()
    }

    fn listed() -> Vec<Json> {
        let answers = ask(&[r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#]);
        answers[0]["result"]["tools"].as_array().unwrap().clone()
    }

    #[test]
    fn the_handshake_states_the_revision_the_tools_and_the_server() {
        let answers = ask(&[HELLO, READY]);

        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0]["result"]["protocolVersion"], json!("2025-06-18"));
        assert_eq!(answers[0]["result"]["capabilities"], json!({ "tools": {} }));
        assert_eq!(answers[0]["result"]["serverInfo"]["name"], json!("krly3d"));
        assert_eq!(answers[0]["result"]["serverInfo"]["version"], json!(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn a_revision_the_server_does_not_speak_is_answered_with_its_newest() {
        let hello = HELLO.replace("2025-06-18", "1999-01-01");

        let answers = ask(&[&hello]);

        assert_eq!(answers[0]["result"]["protocolVersion"], json!("2025-11-25"));
    }

    #[test]
    fn ping_is_answered_with_an_empty_result() {
        let answers = ask(&[r#"{"jsonrpc":"2.0","id":"p","method":"ping"}"#]);

        assert_eq!(answers[0], json!({ "jsonrpc": "2.0", "id": "p", "result": {} }));
    }

    #[test]
    fn every_method_of_the_session_is_listed_as_a_tool_with_its_schema() {
        let methods = session().methods();

        let tools = listed();

        let names: Vec<&str> = tools.iter().map(|tool| tool["name"].as_str().unwrap()).collect();
        let expected: Vec<&str> = methods.iter().map(|method| method.name.as_str()).collect();
        assert_eq!(names, expected);
        for tool in &tools {
            assert_eq!(tool["inputSchema"]["type"], json!("object"), "{tool}");
            assert!(!tool["description"].as_str().unwrap().is_empty(), "{tool}");
        }
    }

    #[test]
    fn a_tool_is_described_by_the_schema_of_its_method() {
        let tools = listed();

        let cuboid = tools.iter().find(|tool| tool["name"] == json!("box")).unwrap();
        let extrude = tools.iter().find(|tool| tool["name"] == json!("extrude")).unwrap();
        assert_eq!(cuboid["inputSchema"]["required"], json!(["w", "d", "h"]));
        assert_eq!(
            cuboid["inputSchema"]["properties"]["w"],
            json!({ "type": "number", "description": "width along x" })
        );
        assert_eq!(cuboid["inputSchema"]["properties"]["dry"]["type"], json!("boolean"));
        assert!(cuboid["description"].as_str().unwrap().ends_with(" [tool]"), "{cuboid}");
        assert!(extrude["description"].as_str().unwrap().ends_with(" [tool, reads 1 node(s)]"), "{extrude}");
        assert_eq!(cuboid["annotations"], json!({ "readOnlyHint": false }));
    }

    #[test]
    fn every_tool_name_is_one_an_mcp_client_accepts() {
        for tool in listed() {
            let name = tool["name"].as_str().unwrap();

            let plain = name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'));
            assert!(plain && (1..=64).contains(&name.len()), "`{name}`");
        }
    }

    #[test]
    fn a_tool_call_is_answered_with_the_text_of_the_reply() {
        let answers = ask(&[&call(3, "box", json!({ "id": "plate", "w": 60, "d": 40, "h": 10 }))]);

        assert_eq!(
            answers[0]["result"],
            json!({
                "content": [{
                    "type": "text",
                    "text": "ok plate@1 box. +back +bottom +front +left +right +top. faces 0>6. volume =24000\n",
                }],
                "isError": false,
            })
        );
    }

    #[test]
    fn a_tool_call_without_arguments_takes_the_defaults() {
        let answers = ask(&[r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"mark"}}"#]);

        assert_eq!(answers[0]["result"]["content"][0]["text"], json!("mark 1"));
    }

    #[test]
    fn a_failed_call_is_a_result_marked_as_an_error_with_the_reason() {
        let answers = ask(&[&call(1, "box", json!({ "id": "plate", "w": 60 }))]);

        assert_eq!(answers[0]["result"]["isError"], json!(true));
        assert_eq!(
            answers[0]["result"]["content"][0]["text"],
            json!("input: plate: `box` needs `d`: depth along y")
        );
    }

    #[test]
    fn a_null_argument_is_a_result_marked_as_an_error_that_names_it() {
        let answers = ask(&[&call(1, "box", json!({ "w": null }))]);

        assert_eq!(answers[0]["result"]["isError"], json!(true));
        assert_eq!(
            answers[0]["result"]["content"][0]["text"],
            json!("input: `w`: null has no counterpart here; leave the entry out instead")
        );
    }

    #[test]
    fn a_reply_that_carries_a_picture_is_answered_with_its_text_and_the_image() {
        let answers = ask(&[
            &call(1, "box", json!({ "id": "plate", "w": 60, "d": 40, "h": 10 })),
            &call(2, "render", json!({ "width": 40, "height": 30 })),
        ]);

        let content = answers[1]["result"]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2, "{}", answers[1]);
        assert_eq!(content[0]["type"], json!("text"));
        assert!(content[0]["text"].as_str().unwrap().starts_with("render iso 40x30 of plate: 3 faces seen"));
        assert_eq!((&content[1]["type"], &content[1]["mimeType"]), (&json!("image"), &json!("image/png")));
        assert!(content[1]["data"].as_str().unwrap().starts_with("iVBORw0KGgo"));
        assert_eq!(answers[1]["result"]["isError"], json!(false));
    }

    #[test]
    fn the_image_of_a_tool_call_is_the_picture_the_plain_dialect_carries_as_data() {
        let lines = [
            call(1, "box", json!({ "id": "plate", "w": 60, "d": 40, "h": 10 })),
            call(2, "render", json!({ "width": 40, "height": 30 })),
        ];
        let plain = r#"{"jsonrpc":"2.0","id":1,"method":"box","params":{"id":"plate","w":60,"d":40,"h":10}}
{"jsonrpc":"2.0","id":2,"method":"render","params":{"width":40,"height":30}}"#;

        let as_tool = ask(&[&lines[0], &lines[1]]);
        let as_method = exchange(&mut Plain, &mut session(), plain);

        assert_eq!(as_tool[1]["result"]["content"][1]["data"], as_method[1]["result"]["data"]["png"]);
    }

    #[test]
    fn an_unknown_tool_is_a_protocol_error() {
        let answers = ask(&[&call(1, "frobnicate", json!({}))]);

        assert_eq!(answers[0]["error"]["code"], json!(-32602));
        assert_eq!(answers[0]["error"]["message"], json!("Invalid params: Unknown tool: frobnicate"));
    }

    #[test]
    fn a_tool_call_of_the_wrong_shape_is_a_protocol_error() {
        let answers = ask(&[
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":7}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"box","arguments":"w=1"}}"#,
        ]);

        let codes: Vec<&Json> = answers.iter().map(|answer| &answer["error"]["code"]).collect();
        assert_eq!(codes, [&json!(-32602), &json!(-32602), &json!(-32602)]);
    }

    #[test]
    fn a_method_outside_the_protocol_is_not_found() {
        let answers = ask(&[r#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#]);

        assert_eq!(answers[0]["error"]["code"], json!(-32601));
    }

    #[test]
    fn calls_on_one_connection_share_the_model() {
        let answers = ask(&[
            HELLO,
            READY,
            &call(2, "box", json!({ "id": "plate", "w": 60, "d": 40, "h": 10 })),
            &call(3, "save", json!({})),
        ]);

        assert_eq!(
            answers[2]["result"]["content"][0]["text"],
            json!("k3d 1\nlayer geo\nplate = box d=40 h=10 w=60\n")
        );
    }
}
