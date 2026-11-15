//! Public legacy adapters select the new backend without changing default rendering.
#![allow(clippy::unwrap_used, clippy::panic)]

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn run(mcp: bool, calls: &[(&str, Value)]) -> Vec<Value> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_krly3d"));
    if mcp {
        command.arg("--mcp");
    }
    let mut child =
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    for (index, (method, args)) in calls.iter().enumerate() {
        let request = if mcp {
            json!({"jsonrpc":"2.0","id":index,"method":"tools/call","params":{"name":method,"arguments":args}})
        } else {
            json!({"jsonrpc":"2.0","id":index,"method":method,"params":args})
        };
        writeln!(input, "{request}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).unwrap().lines().map(|s| serde_json::from_str(s).unwrap()).collect()
}

fn box_args() -> Value {
    json!({"id":"part","w":6,"d":4,"h":2})
}

#[test]
fn mesh_preview_is_opt_in_and_reports_quality_without_changing_ray_results() {
    let replies = run(
        false,
        &[
            ("box", box_args()),
            ("render", json!({"width":48,"height":36})),
            ("render", json!({"width":48,"height":36,"renderer":"raycast"})),
            ("render", json!({"width":48,"height":36,"renderer":"mesh"})),
            ("save", json!({})),
        ],
    );
    assert_eq!(replies[1]["result"], replies[2]["result"]);
    assert!(replies[1]["result"]["data"].get("renderer").is_none());
    let result = &replies[3]["result"];
    assert_eq!(result["ok"], true, "{}", replies[3]);
    assert_eq!(result["data"]["renderer"], "mesh");
    assert_eq!(result["data"]["quality"]["tolerance"], 0.05);
    assert_eq!(result["data"]["quality"]["achieved_error"], "unknown");
    assert!(result["data"]["timing"]["mesh_and_raster_ms"].as_f64().unwrap() >= 0.0);
    assert_eq!(result["data"]["timing"]["includes_encoding"], false);
    assert!(result["text"].as_str().unwrap().contains("triangle preview"));
    assert!(result["data"]["png"].as_str().unwrap().starts_with("iVBOR"));
    assert!(replies[4]["result"]["text"].as_str().unwrap().contains("part = box"));
}

#[test]
fn mesh_preview_reaches_mcp_with_the_same_png_and_readable_quality() {
    let calls = [("box", box_args()), ("render", json!({"width":48,"height":36,"renderer":"mesh"}))];
    let plain = run(false, &calls);
    let mcp = run(true, &calls);
    let result = &mcp[1]["result"];
    assert_eq!(result["isError"], false, "{result}");
    assert!(result["content"][0]["text"].as_str().unwrap().contains("achieved error unknown"));
    assert_eq!(result["content"][1]["data"], plain[1]["result"]["data"]["png"]);
}

#[test]
fn preview_face_reporting_and_silhouette_work_while_pick_stays_exact() {
    let replies = run(
        false,
        &[
            ("box", box_args()),
            ("view", json!({"width":48,"height":36,"renderer":"mesh"})),
            ("silhouette", json!({"width":48,"height":36,"renderer":"mesh"})),
            ("pick", json!({"width":48,"height":36,"pixels":[24,18]})),
            ("pick", json!({"width":48,"height":36,"pixels":[24,18],"renderer":"mesh"})),
            ("render", json!({"renderer":"typo"})),
        ],
    );
    for reply in &replies[1..4] {
        assert_eq!(reply["result"]["ok"], true, "{reply}");
    }
    assert!(!replies[1]["result"]["data"]["faces"].as_array().unwrap().is_empty());
    assert!(!replies[2]["result"]["data"]["spans"].as_array().unwrap().is_empty());
    assert!(replies[4].get("error").is_some());
    assert!(replies[5]["error"]["message"].as_str().unwrap().contains("unknown renderer"));
}
