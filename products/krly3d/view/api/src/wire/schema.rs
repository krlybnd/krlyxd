use serde_json::{json, Value};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn vector(size: u32) -> Value {
    json!({"type":"array","items":{"type":"number"},"minItems":size,"maxItems":size})
}
fn positive() -> Value {
    json!({"type":"number","exclusiveMinimum":0})
}
fn nonnegative() -> Value {
    json!({"type":"number","minimum":0})
}
fn string() -> Value {
    json!({"type":"string"})
}
fn nonempty() -> Value {
    json!({"type":"string","minLength":1})
}
fn nullable(value: Value) -> Value {
    json!({"anyOf":[value,{"type":"null"}]})
}
fn version() -> Value {
    object(
        json!({"major":{"type":"integer","const":1},"minor":{"type":"integer","const":0}}),
        &["major", "minor"],
    )
}
fn dimension() -> Value {
    json!({"type":"integer","minimum":1,"maximum":2048})
}
fn length() -> Value {
    object(json!({"value":positive(),"unit":{"type":"string","const":"model"}}), &["value", "unit"])
}
fn camera() -> Value {
    json!({"oneOf":[
        object(json!({"kind":{"const":"named"},"name":{"enum":["iso","front","back","left","right","top","bottom"]}}), &["kind","name"]),
        object(json!({"kind":{"const":"orthographic"},"toward":vector(3),"up":vector(3)}), &["kind","toward","up"]),
        object(json!({"kind":{"const":"pinhole"},"eye":vector(3),"toward":vector(3),"up":vector(3),"focal":positive(),"centre":vector(2)}), &["kind","eye","toward","up","focal"])
    ]})
}
fn input() -> Value {
    object(
        json!({"node":nonempty(),"width":dimension(),"height":dimension(),"camera":camera(),
        "backend":{"enum":["raycast","mesh"]},"tolerance":length()}),
        &["node"],
    )
}
fn quality() -> Value {
    let quantity =
        object(json!({"value":nonnegative(),"unit":{"enum":["model","metre"]}}), &["value", "unit"]);
    object(
        json!({"exactness":{"oneOf":[
        object(json!({"kind":{"const":"exact"}}), &["kind"]),
        object(json!({"kind":{"const":"numeric"},"tolerance":positive()}), &["kind","tolerance"]),
        object(json!({"kind":{"const":"sampled"},"samples":{"type":"integer","minimum":1,"maximum":4294967295_u64}}), &["kind","samples"])
    ]},"requested_tolerance":nullable(quantity.clone()),"positional_uncertainty":nullable(quantity),
       "confidence":nullable(json!({"type":"number","minimum":0,"maximum":1}))}),
        &["exactness", "requested_tolerance", "positional_uncertainty", "confidence"],
    )
}
fn output() -> Value {
    let source = object(
        json!({"source":nonempty(),"session":nonempty(),"node":nonempty(),"build_key":string()}),
        &["source", "session", "node", "build_key"],
    );
    object(
        json!({"protocol":version(),"api":version(),"request_id":nonempty(),"width":dimension(),"height":dimension(),
        "image":object(json!({"mime_type":{"const":"image/png"},"encoding":{"const":"base64"},"data":{"type":"string","contentEncoding":"base64","contentMediaType":"image/png"}}), &["mime_type","encoding","data"]),
        "faces":{"type":"array","items":object(json!({"name":string(),"pixels":{"type":"integer","minimum":0,"maximum":4194304}}), &["name","pixels"])},
        "quality":quality(),"provenance":object(json!({"source":source,"input":input(),"engine_revision":nonempty()}), &["source","input","engine_revision"]),
        "timing":object(json!({"render_ms":nonnegative(),"encoding_ms":nonnegative()}), &["render_ms","encoding_ms"])}),
        &[
            "protocol",
            "api",
            "request_id",
            "width",
            "height",
            "image",
            "faces",
            "quality",
            "provenance",
            "timing",
        ],
    )
}

/// JSON Schema describes the structure; semantic validation additionally checks
/// nonzero directions, finite numbers and JSON-safe integer tokens.
pub fn descriptor() -> Value {
    let metadata = super::metadata();
    json!({"package":metadata.package.name,"package_version":metadata.package.version,
        "protocol":super::version(),"api":super::version(),"engine_revision":metadata.engine_revision,
        "operation":"view_render","synchronous":true,"read_only":true,"cancellation":false,"timeouts":false,
        "supported_events":[],"coordinates":"right-handed Z-up","length_unit":"model",
        "artifact_format":"image/png","max_side":crate::MAX_SIDE,
        "call_schema":object(json!({"protocol":version(),"api":version(),"request_id":nonempty(),"input":input()}), &["protocol","api","request_id","input"]),
        "input_schema":input(),"output_schema":output()})
}
