//! View operation protocol/API 2.0. Completed payloads retain render result 1.0.
//! Only Started and one terminal event are produced; no partial images/progress.
use crate::wire::request::{input, parse, version, Object};
use crate::{diagnostic, ApiVersion, Diagnostic, Provenance, RenderInput, RenderResult, RequestId};
pub use krlyxd::OperationId;
use serde_json::{json, value::RawValue, Value};

pub const API_VERSION: ApiVersion = ApiVersion { major: 2, minor: 0 };
pub const MAX_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;

#[derive(Clone, Debug, PartialEq)]
pub struct OperationCall {
    pub request_id: RequestId,
    pub protocol: ApiVersion,
    pub api: ApiVersion,
    pub timeout_ms: Option<u64>,
    pub input: RenderInput,
}
impl OperationCall {
    pub fn new(request_id: RequestId, input: RenderInput) -> Self {
        Self { request_id, protocol: API_VERSION, api: API_VERSION, timeout_ms: None, input }
    }
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.protocol != API_VERSION || self.api != API_VERSION {
            return Err(diagnostic("unsupported_version", "view operations require protocol/API 2.0"));
        }
        if self.request_id.as_str().is_empty() || self.timeout_ms.is_some_and(|v| v > MAX_SAFE_INTEGER) {
            return Err(diagnostic("input", "nonempty request_id and JSON-safe timeout_ms required"));
        }
        self.input.validate()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderEvent {
    pub request_id: RequestId,
    pub operation_id: OperationId,
    pub seq: u64,
    pub body: RenderEventBody,
}
#[derive(Clone, Debug, PartialEq)]
pub enum RenderEventBody {
    Started(Provenance),
    Completed(RenderResult),
    Failed(Diagnostic),
    Cancelled,
}

pub fn decode_operation_call(text: &str) -> Result<OperationCall, Diagnostic> {
    let mut fields = Object::read(text, &["protocol", "api", "request_id", "input", "timeout_ms"])?;
    let call = OperationCall {
        protocol: version(fields.take("protocol")?.get())?,
        api: version(fields.take("api")?.get())?,
        request_id: RequestId::new(&parse::<String>(fields.take("request_id")?.get())?),
        timeout_ms: fields.0.remove("timeout_ms").map(|raw| parse(raw.get())).transpose()?.flatten(),
        input: input(fields.take("input")?.get())?,
    };
    call.validate()?;
    Ok(call)
}

pub fn encode_operation_call(call: &OperationCall) -> Result<String, Diagnostic> {
    call.validate()?;
    Ok(json!({"protocol":{"major":2,"minor":0},"api":{"major":2,"minor":0},
        "request_id":call.request_id.as_str(),"timeout_ms":call.timeout_ms,
        "input":crate::wire::input(&call.input)})
    .to_string())
}

pub fn encode_event(event: &RenderEvent) -> Result<String, Diagnostic> {
    if event.request_id.as_str().is_empty()
        || event.operation_id.as_str().is_empty()
        || event.seq > MAX_SAFE_INTEGER
    {
        return Err(diagnostic("output", "invalid view event identity or sequence"));
    }
    let body = match &event.body {
        RenderEventBody::Started(p) => {
            p.input.validate()?;
            if [
                p.source.source.as_str(),
                p.source.session.as_str(),
                &p.source.node,
                p.engine_revision.as_str(),
            ]
            .iter()
            .any(|v| v.is_empty())
            {
                return Err(diagnostic("output", "empty view provenance identity"));
            }
            json!({"kind":"started","provenance":{
                "source":{"source":p.source.source.as_str(),"session":p.source.session.as_str(),"node":p.source.node,"build_key":p.source.build_key},
                "input":crate::wire::input(&p.input),"engine_revision":p.engine_revision.as_str()
            }}).to_string()
        }
        RenderEventBody::Completed(result) => {
            if result.request_id != event.request_id {
                return Err(diagnostic("output", "completed request_id does not match event"));
            }
            format!(r#"{{"kind":"completed","result":{}}}"#, crate::wire::encode_result(result)?)
        }
        RenderEventBody::Failed(error) => {
            if error.code.as_str().is_empty() {
                return Err(diagnostic("output", "empty diagnostic code"));
            }
            json!({"kind":"failed","diagnostic":{"code":error.code.as_str(),"message":error.message}})
                .to_string()
        }
        RenderEventBody::Cancelled => r#"{"kind":"cancelled"}"#.to_owned(),
    };
    // Preserve the completed codec's floating-point tokens verbatim.
    #[derive(serde::Serialize)]
    struct Envelope<'a> {
        api: Value,
        body: &'a RawValue,
        operation_id: &'a str,
        protocol: Value,
        request_id: &'a str,
        seq: u64,
    }
    let body = RawValue::from_string(body).map_err(|e| diagnostic("output", e.to_string()))?;
    serde_json::to_string(&Envelope {
        api: json!({"major":2,"minor":0}),
        body: &body,
        operation_id: event.operation_id.as_str(),
        protocol: json!({"major":2,"minor":0}),
        request_id: event.request_id.as_str(),
        seq: event.seq,
    })
    .map_err(|e| diagnostic("output", e.to_string()))
}

/// Independent descriptor: v1 remains synchronous and rejects operation fields.
pub fn descriptor() -> Value {
    let base = crate::wire::descriptor();
    let mut call = base["call_schema"].clone();
    for key in ["protocol", "api"] {
        call["properties"][key]["properties"]["major"]["const"] = json!(2);
    }
    call["properties"]["timeout_ms"] =
        json!({"type":["integer","null"],"minimum":0,"maximum":MAX_SAFE_INTEGER});
    let id = json!({"type":"string","minLength":1});
    let version = json!({"type":"object","additionalProperties":false,"required":["major","minor"],"properties":{"major":{"const":2,"type":"integer"},"minor":{"const":0,"type":"integer"}}});
    let output = json!({"type":"object","additionalProperties":false,
    "required":["api","protocol","request_id","operation_id","seq","body"],"properties":{
        "api":version,"protocol":version,"request_id":id,"operation_id":id,
        "seq":{"type":"integer","minimum":0,"maximum":MAX_SAFE_INTEGER},
        "body":{"oneOf":[
            {"type":"object","additionalProperties":false,"required":["kind","provenance"],"properties":{"kind":{"const":"started"},"provenance":base["output_schema"]["properties"]["provenance"]}},
            {"type":"object","additionalProperties":false,"required":["kind","result"],"properties":{"kind":{"const":"completed"},"result":base["output_schema"]}},
            {"type":"object","additionalProperties":false,"required":["kind","diagnostic"],"properties":{"kind":{"const":"failed"},"diagnostic":{"type":"object","additionalProperties":false,"required":["code","message"],"properties":{"code":id,"message":{"type":"string"}}}}},
            {"type":"object","additionalProperties":false,"required":["kind"],"properties":{"kind":{"const":"cancelled"}}}
        ]}
    }});
    let mut descriptor = base;
    descriptor["protocol"] = json!({"major":2,"minor":0});
    descriptor["api"] = json!({"major":2,"minor":0});
    descriptor["operation"] = json!("view_start");
    descriptor["synchronous"] = json!(false);
    descriptor["cancellation"] = json!(true);
    descriptor["timeouts"] = json!(true);
    descriptor["supported_events"] = json!(["started", "completed", "failed", "cancelled"]);
    descriptor["call_schema"] = call;
    descriptor["output_schema"] = output;
    descriptor
}

pub fn metadata() -> krlyxd::ContractMetadata<'static> {
    krlyxd::ContractMetadata { protocol: API_VERSION, api: API_VERSION, ..crate::wire::metadata() }
}
