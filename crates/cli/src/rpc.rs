//! JSON-RPC 2.0: the envelope every dialect speaks.
//!
//! This module knows requests, notifications, batches and error objects, and nothing of
//! what a method means. A dialect supplies the meaning as a function from a method name
//! and its parameters to a result or a [`Failure`].

use serde_json::{json, Map, Value as Json};
use std::fmt;

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
/// The first of the codes the specification leaves to the server: the request was
/// understood and the call it asked for did not succeed.
pub const CALL_FAILED: i64 = -32000;

/// The error member of a response.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub code: i64,
    pub message: String,
    pub data: Option<Json>,
}

impl Failure {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), data: None }
    }

    pub fn with_data(mut self, data: Json) -> Self {
        self.data = Some(data);
        self
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(METHOD_NOT_FOUND, format!("Method not found: `{method}`"))
    }

    pub fn invalid_params(reason: impl fmt::Display) -> Self {
        Self::new(INVALID_PARAMS, format!("Invalid params: {reason}"))
    }

    pub fn internal(reason: impl fmt::Display) -> Self {
        Self::new(INTERNAL_ERROR, format!("Internal error: {reason}"))
    }

    fn invalid_request(reason: &str) -> Self {
        Self::new(INVALID_REQUEST, format!("Invalid Request: {reason}"))
    }
}

/// What a dialect does with one request: the method, its `params` member if there is
/// one (an object or an array), and the result or the failure to send back.
pub type Handler<'a> = dyn FnMut(&str, Option<&Json>) -> Result<Json, Failure> + 'a;

/// Answers one message as it arrived: a request, a notification or a batch of them.
/// `None` means nothing is sent back, as for a notification.
pub fn answer(message: &[u8], handle: &mut Handler<'_>) -> Option<Json> {
    match serde_json::from_slice::<Json>(message) {
        Err(error) => Some(failed(Json::Null, Failure::new(PARSE_ERROR, format!("Parse error: {error}")))),
        Ok(Json::Array(batch)) if batch.is_empty() => {
            Some(failed(Json::Null, Failure::invalid_request("a batch must hold at least one request")))
        }
        Ok(Json::Array(batch)) => {
            let answers: Vec<Json> = batch.iter().filter_map(|one| answer_one(one, handle)).collect();
            (!answers.is_empty()).then_some(Json::Array(answers))
        }
        Ok(one) => answer_one(&one, handle),
    }
}

fn answer_one(message: &Json, handle: &mut Handler<'_>) -> Option<Json> {
    let request = match Request::read(message) {
        Ok(request) => request,
        Err((id, failure)) => return Some(failed(id, failure)),
    };
    let outcome = handle(request.method, request.params);
    // A notification is carried out and never answered, whatever came of it.
    let id = request.id?;
    Some(match outcome {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(failure) => failed(id.clone(), failure),
    })
}

struct Request<'a> {
    /// `None` for a notification.
    id: Option<&'a Json>,
    method: &'a str,
    params: Option<&'a Json>,
}

impl<'a> Request<'a> {
    /// The parts of a request, or the failure to report with the id to report it under.
    fn read(message: &'a Json) -> Result<Self, (Json, Failure)> {
        let Json::Object(members) = message else {
            return Err((Json::Null, Failure::invalid_request("a request is a JSON object")));
        };
        let id = match members.get("id") {
            None => None,
            Some(id @ (Json::String(_) | Json::Number(_) | Json::Null)) => Some(id),
            Some(_) => {
                return Err((Json::Null, Failure::invalid_request("`id` must be a string or a number")))
            }
        };
        let refused = |reason: &str| (id.cloned().unwrap_or(Json::Null), Failure::invalid_request(reason));
        if members.get("jsonrpc") != Some(&Json::from("2.0")) {
            return Err(refused("`jsonrpc` must be \"2.0\""));
        }
        let Some(Json::String(method)) = members.get("method") else {
            return Err(refused("`method` must be a string"));
        };
        let params = match members.get("params") {
            None => None,
            Some(params @ (Json::Object(_) | Json::Array(_))) => Some(params),
            Some(_) => return Err(refused("`params` must be an object or an array")),
        };
        Ok(Self { id, method, params })
    }
}

/// The answer to a message that cannot be taken as a request at all.
pub fn refused(reason: &str) -> Json {
    failed(Json::Null, Failure::invalid_request(reason))
}

fn failed(id: Json, failure: Failure) -> Json {
    let mut error = Map::new();
    error.insert("code".to_string(), Json::from(failure.code));
    error.insert("message".to_string(), Json::from(failure.message));
    if let Some(data) = failure.data {
        error.insert("data".to_string(), data);
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

#[cfg(test)]
mod tests {
    use super::{answer, Failure, CALL_FAILED};
    use serde_json::{json, Value as Json};

    /// Answers `echo` with its parameters and fails everything else.
    fn echo(method: &str, params: Option<&Json>) -> Result<Json, Failure> {
        match method {
            "echo" => Ok(params.cloned().unwrap_or(Json::Null)),
            other => Err(Failure::method_not_found(other)),
        }
    }

    fn answered(message: &str) -> Option<Json> {
        answer(message.as_bytes(), &mut echo)
    }

    #[test]
    fn a_request_is_answered_under_its_id() {
        let reply = answered(r#"{"jsonrpc":"2.0","id":7,"method":"echo","params":{"a":1}}"#);

        assert_eq!(reply, Some(json!({ "jsonrpc": "2.0", "id": 7, "result": { "a": 1 } })));
    }

    #[test]
    fn a_text_id_comes_back_as_text() {
        let reply = answered(r#"{"jsonrpc":"2.0","id":"abc","method":"echo"}"#);

        assert_eq!(reply, Some(json!({ "jsonrpc": "2.0", "id": "abc", "result": null })));
    }

    #[test]
    fn a_notification_is_carried_out_and_not_answered() {
        let mut seen = Vec::new();
        let mut record = |method: &str, _: Option<&Json>| {
            seen.push(method.to_string());
            Err(Failure::new(CALL_FAILED, "nobody hears this"))
        };

        let reply = answer(br#"{"jsonrpc":"2.0","method":"note"}"#, &mut record);

        assert_eq!(reply, None);
        assert_eq!(seen, ["note"]);
    }

    #[test]
    fn text_that_is_not_json_is_a_parse_error_without_an_id() {
        let reply = answered(r#"{"jsonrpc":"2.0","id":1,"method":"#).unwrap();

        assert_eq!(reply["id"], Json::Null);
        assert_eq!(reply["error"]["code"], json!(-32700));
    }

    #[test]
    fn bytes_that_are_not_text_are_a_parse_error() {
        let reply = answer(&[0xff, 0xfe, b'{', b'}'], &mut echo).unwrap();

        assert_eq!(reply["error"]["code"], json!(-32700));
    }

    #[test]
    fn a_message_that_is_not_a_request_is_an_invalid_request() {
        for message in [
            r#"1"#,
            r#""echo""#,
            r#"{"id":1,"method":"echo"}"#,
            r#"{"jsonrpc":"1.0","id":1,"method":"echo"}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":5}"#,
            r#"{"jsonrpc":"2.0","id":1}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"echo","params":"text"}"#,
            r#"{"jsonrpc":"2.0","id":[1],"method":"echo"}"#,
            r#"{"jsonrpc":"2.0","method":1,"params":"bar"}"#,
        ] {
            let reply = answered(message).unwrap();

            assert_eq!(reply["error"]["code"], json!(-32600), "{message} gave {reply}");
        }
    }

    #[test]
    fn an_invalid_request_is_answered_under_its_id_when_that_can_be_read() {
        let readable = answered(r#"{"jsonrpc":"2.0","id":4,"method":5}"#).unwrap();
        let unreadable = answered(r#"{"jsonrpc":"2.0","id":{},"method":"echo"}"#).unwrap();

        assert_eq!((readable["id"].clone(), unreadable["id"].clone()), (json!(4), Json::Null));
    }

    #[test]
    fn a_failure_carries_its_code_message_and_data() {
        let mut fail = |_: &str, _: Option<&Json>| {
            Err(Failure::new(CALL_FAILED, "the call failed").with_data(json!({ "why": "because" })))
        };

        let reply = answer(br#"{"jsonrpc":"2.0","id":1,"method":"x"}"#, &mut fail);

        assert_eq!(
            reply,
            Some(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": { "code": -32000, "message": "the call failed", "data": { "why": "because" } },
            }))
        );
    }

    #[test]
    fn a_batch_is_answered_as_a_list_without_its_notifications() {
        let reply = answered(
            r#"[{"jsonrpc":"2.0","id":1,"method":"echo","params":[1]},
                {"jsonrpc":"2.0","method":"echo"},
                {"jsonrpc":"2.0","id":2,"method":"missing"},
                7]"#,
        )
        .unwrap();

        let Json::Array(answers) = reply else { panic!("not a list") };
        assert_eq!(answers.len(), 3);
        assert_eq!(answers[0], json!({ "jsonrpc": "2.0", "id": 1, "result": [1] }));
        assert_eq!(answers[1]["error"]["code"], json!(-32601));
        assert_eq!(answers[2]["error"]["code"], json!(-32600));
    }

    #[test]
    fn an_empty_batch_is_an_invalid_request() {
        let reply = answered("[]").unwrap();

        assert_eq!(reply["error"]["code"], json!(-32600));
    }

    #[test]
    fn a_batch_of_notifications_is_not_answered() {
        let reply = answered(r#"[{"jsonrpc":"2.0","method":"echo"},{"jsonrpc":"2.0","method":"echo"}]"#);

        assert_eq!(reply, None);
    }
}
