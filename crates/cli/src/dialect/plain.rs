//! The plain dialect: every method of the session is a JSON-RPC method of the same name.
//!
//! ```text
//! > {"jsonrpc":"2.0","id":1,"method":"box","params":{"id":"plate","w":60,"d":40,"h":10}}
//! < {"jsonrpc":"2.0","id":1,"result":{"ok":true,"text":"ok plate@1 box. …","data":{…},"exact":"="}}
//! ```
//!
//! The result is the whole reply. A call that did not do what was asked is an error whose
//! message is the text of the reply and whose data is the reply, so nothing is lost.

use super::Dialect;
use crate::endpoint::Endpoint;
use crate::json;
use crate::rpc::{Failure, CALL_FAILED, INVALID_PARAMS, METHOD_NOT_FOUND};
use krly3d_core::ErrorKind;
use krly3d_kernel::schema::{Params, Value};
use krly3d_kernel::Reply;
use serde_json::Value as Json;

pub struct Plain;

impl Dialect for Plain {
    fn name(&self) -> &'static str {
        "plain"
    }

    fn about(&self) -> &'static str {
        "JSON-RPC 2.0: every method of the session under its own name (the default)"
    }

    fn answer(
        &mut self,
        session: &mut dyn Endpoint,
        method: &str,
        params: Option<&Json>,
    ) -> Result<Json, Failure> {
        let args = match params {
            Some(params) => json::params_from_json(params).map_err(Failure::invalid_params)?,
            None => Params::new(),
        };
        let known = session.methods().iter().any(|listed| listed.name == method);
        let reply = session.call(method, &args);
        let whole = json::reply_to_json(&reply).map_err(Failure::internal)?;
        if reply.ok {
            return Ok(whole);
        }
        let code = match known {
            false => METHOD_NOT_FOUND,
            true if is_about_the_arguments(&reply) => INVALID_PARAMS,
            true => CALL_FAILED,
        };
        Err(Failure::new(code, reply.text.trim_end()).with_data(whole))
    }
}

/// Whether a failed reply blames what the caller sent rather than what came of it.
fn is_about_the_arguments(reply: &Reply) -> bool {
    let Value::Map(data) = &reply.data else { return false };
    data.get("error") == Some(&Value::from(ErrorKind::Input.label()))
}

#[cfg(test)]
mod tests {
    use super::Plain;
    use crate::dialect::testing::{exchange, session};
    use serde_json::{json, Value as Json};

    const PLATE: &str =
        r#"{"jsonrpc":"2.0","id":1,"method":"box","params":{"id":"plate","w":60,"d":40,"h":10}}"#;

    fn ask(lines: &[&str]) -> Vec<Json> {
        exchange(&mut Plain, &mut session(), &lines.join("\n"))
    }

    #[test]
    fn a_call_is_answered_with_the_whole_reply() {
        let answers = ask(&[PLATE]);

        assert_eq!(answers[0]["id"], json!(1));
        assert_eq!(
            answers[0]["result"]["text"],
            json!("ok plate@1 box. +back +bottom +front +left +right +top. faces 0>6. volume =24000\n")
        );
        assert_eq!(answers[0]["result"]["ok"], json!(true));
        assert_eq!(answers[0]["result"]["exact"], json!("="));
        assert_eq!(answers[0]["result"]["data"]["id"], json!("plate"));
        assert_eq!(answers[0]["result"]["data"]["nodes"][0]["volume"], json!(24000));
    }

    #[test]
    fn calls_on_one_connection_share_the_model() {
        let answers = ask(&[PLATE, r#"{"jsonrpc":"2.0","id":2,"method":"save"}"#]);

        assert_eq!(answers[1]["result"]["text"], json!("k3d 1\nlayer geo\nplate = box d=40 h=10 w=60\n"));
    }

    #[test]
    fn a_call_without_params_takes_the_defaults() {
        let answers = ask(&[r#"{"jsonrpc":"2.0","id":"a","method":"mark"}"#]);

        assert_eq!(answers[0]["result"]["data"], json!({ "mark": 1 }));
    }

    #[test]
    fn an_unknown_method_is_method_not_found_with_the_reason_of_the_session() {
        let answers = ask(&[r#"{"jsonrpc":"2.0","id":1,"method":"frobnicate"}"#]);

        assert_eq!(answers[0]["error"]["code"], json!(-32601));
        assert_eq!(
            answers[0]["error"]["message"],
            json!("not found: there is no method `frobnicate`; `methods` lists them")
        );
    }

    #[test]
    fn a_wrong_argument_is_invalid_params_with_the_reply_as_data() {
        let answers = ask(&[r#"{"jsonrpc":"2.0","id":1,"method":"box","params":{"id":"plate","w":"wide"}}"#]);

        assert_eq!(answers[0]["error"]["code"], json!(-32602));
        assert_eq!(
            answers[0]["error"]["message"],
            json!("input: plate: `box`: `w` must be a number, got Text(\"wide\")")
        );
        assert_eq!(answers[0]["error"]["data"]["ok"], json!(false));
        assert_eq!(answers[0]["error"]["data"]["data"], json!({ "error": "input" }));
    }

    #[test]
    fn a_call_that_fails_for_another_reason_is_a_server_error() {
        let answers = ask(&[r#"{"jsonrpc":"2.0","id":1,"method":"rollback","params":{"mark":3}}"#]);

        assert_eq!(answers[0]["error"]["code"], json!(-32000));
        assert_eq!(answers[0]["error"]["message"], json!("not found: there is no mark 3; 0 were set"));
    }

    #[test]
    fn an_expectation_that_does_not_hold_is_an_error_that_keeps_what_was_found() {
        let refused = r#"{"jsonrpc":"2.0","id":1,"method":"box","params":{"id":"plate","w":60,"d":40,"h":10,"expect":["faces=7"]}}"#;

        let answers = ask(&[refused]);

        assert_eq!(answers[0]["error"]["code"], json!(-32000));
        assert_eq!(answers[0]["error"]["data"]["data"]["verdicts"][0]["holds"], json!(false));
        assert_eq!(answers[0]["error"]["data"]["data"]["committed"], json!(false));
    }

    #[test]
    fn positional_params_are_invalid_params() {
        let answers = ask(&[r#"{"jsonrpc":"2.0","id":1,"method":"box","params":[60,40,10]}"#]);

        assert_eq!(answers[0]["error"]["code"], json!(-32602));
        assert_eq!(
            answers[0]["error"]["message"],
            json!("Invalid params: arguments are given by name, as a JSON object")
        );
    }

    #[test]
    fn a_null_argument_is_invalid_params_that_names_it() {
        let answers = ask(&[r#"{"jsonrpc":"2.0","id":1,"method":"box","params":{"w":null}}"#]);

        assert_eq!(answers[0]["error"]["code"], json!(-32602));
        assert_eq!(
            answers[0]["error"]["message"],
            json!("Invalid params: `w`: null has no counterpart here; leave the entry out instead")
        );
    }

    #[test]
    fn a_number_too_large_for_json_is_a_parse_error_and_the_connection_goes_on() {
        let huge = r#"{"jsonrpc":"2.0","id":1,"method":"box","params":{"w":1e999}}"#;

        let answers = ask(&[huge, PLATE]);

        assert_eq!(answers[0]["error"]["code"], json!(-32700));
        assert_eq!(answers[1]["result"]["ok"], json!(true));
    }

    #[test]
    fn a_nested_batch_argument_reaches_the_session_as_it_was_sent() {
        let batch = r#"{"jsonrpc":"2.0","id":1,"method":"batch","params":{"calls":[
            {"method":"box","args":{"id":"plate","w":60,"d":40,"h":10}},
            {"method":"cylinder","args":{"id":"peg","r":3,"h":6,"at":[0,0,10]}}]}}"#
            .replace('\n', " ");

        let answers = ask(&[&batch]);

        assert_eq!(answers[0]["result"]["ok"], json!(true));
        assert_eq!(answers[0]["result"]["data"]["replies"][1]["data"]["id"], json!("peg"));
    }

    #[test]
    fn a_notification_changes_the_model_and_is_not_answered() {
        let silent = r#"{"jsonrpc":"2.0","method":"box","params":{"id":"plate","w":60,"d":40,"h":10}}"#;

        let answers = ask(&[silent, r#"{"jsonrpc":"2.0","id":2,"method":"save"}"#]);

        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0]["result"]["text"], json!("k3d 1\nlayer geo\nplate = box d=40 h=10 w=60\n"));
    }
}
