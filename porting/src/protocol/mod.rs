//! krly3d protocol: JSON-RPC 2.0, one request per line (stdio, socket or WASM).
//!
//! Any client – a UI, a test, an AI agent – talks to the kernel the same way.
//! Results carry both structured data and a short `text` view meant for LLMs.
//!
//! Methods
//!   create   {id?, at?: [x,y,z], primitive: {kind:"Cuboid", width, height, depth} …}
//!   chamfer  {id, edges:["front|top"], distance}
//!   fillet   {id, edges:[…], radius}
//!   subtract | union | intersect {id, tools:[ids]}
//!   faces    {id}                 one line per face
//!   edges    {id}                 edge names
//!   here     {id, face}           the view from a face
//!   ray      {id, origin:[x,y,z], direction:[x,y,z]}
//!   find     {id, query:"cylinder near:n:+z", k?}
//!   keys     {id, face}           the sparse keys of one face
//!   measure  {id}
//!   mark     {}                   -> {mark}
//!   rollback {mark}
//!   changes  {since}

use crate::kernel::Kernel;
use crate::perception::{self, Index, KeyLayer, Query};
use crate::query::PointClassification;
use crate::world_graph::{ChangeSet, CreateOptions, EditScope, MarkId, ModifyingOperation, Primitive, WorldGraph};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub struct Session {
    kernel: Kernel,
    marks: BTreeMap<u64, MarkId>,
    next_mark: u64,
}

#[derive(Debug)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError { code: -32602, message: message.into() }
}

fn kernel<E: std::fmt::Debug>(e: E) -> RpcError {
    RpcError { code: -32000, message: format!("{e:?}") }
}

fn str_param<'a>(p: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    p.get(key).and_then(Value::as_str).ok_or_else(|| invalid(format!("missing string `{key}`")))
}

fn num_param(p: &Value, key: &str) -> Result<f64, RpcError> {
    p.get(key).and_then(Value::as_f64).ok_or_else(|| invalid(format!("missing number `{key}`")))
}

fn point_param(p: &Value, key: &str) -> Result<[f64; 3], RpcError> {
    serde_json::from_value(p.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|_| invalid(format!("`{key}` must be [x,y,z]")))
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Kernel::builder()
            .layer(KeyLayer::default())
            .build()
            .map(Self::with_kernel)
            .unwrap_or_else(|_| unreachable_kernel())
    }

    /// Run the protocol on a kernel assembled elsewhere (dependency injection).
    pub fn with_kernel(kernel: Kernel) -> Self {
        Self { kernel, marks: BTreeMap::new(), next_mark: 0 }
    }

    pub fn world(&self) -> &WorldGraph {
        self.kernel.world()
    }

    pub fn kernel(&self) -> &Kernel {
        &self.kernel
    }

    /// Handle one JSON-RPC message; returns the response line (None for notifications).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":e.to_string()}})
                        .to_string(),
                )
            }
        };
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));
        let outcome = self.call(method, &params);
        let id = id?;
        Some(
            match outcome {
                Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                Err(e) => json!({"jsonrpc":"2.0","id":id,"error":{"code":e.code,"message":e.message}}),
            }
            .to_string(),
        )
    }

    pub fn call(&mut self, method: &str, p: &Value) -> Result<Value, RpcError> {
        match method {
            "create" => {
                let primitive: Primitive = serde_json::from_value(p.get("primitive").cloned().unwrap_or(Value::Null))
                    .map_err(|e| invalid(format!("primitive: {e}")))?;
                // optional placement: the body's base point in world coordinates (Y up)
                let plane = match p.get("at") {
                    Some(_) => Some(crate::world_graph::Plane {
                        origin: Some(point_param(p, "at")?),
                        normal: Some([0.0, 1.0, 0.0]),
                        x_direction: Some([1.0, 0.0, 0.0]),
                    }),
                    None => None,
                };
                let options = CreateOptions {
                    node_id: p.get("id").and_then(Value::as_str).map(String::from),
                    plane,
                    ..CreateOptions::default()
                };
                let (id, changes) = self.kernel.world.create_primitive(primitive, options).map_err(kernel)?;
                let brep = self.kernel.world.placed_brep(&id).map_err(kernel)?;
                let text = perception::overview(&brep).map_err(kernel)?;
                Ok(json!({"id": id, "changes": changes, "text": text}))
            }
            "chamfer" | "fillet" => {
                let id = str_param(p, "id")?.to_string();
                let edges: Vec<String> = serde_json::from_value(p.get("edges").cloned().unwrap_or(Value::Null))
                    .map_err(|_| invalid("`edges` must be a list of edge names"))?;
                let refs: Vec<&str> = edges.iter().map(String::as_str).collect();
                let before = self.kernel.world.placed_brep(&id).map_err(kernel)?;
                let changes = if method == "chamfer" {
                    self.kernel.world.chamfer_named(&id, &refs, num_param(p, "distance")?, EditScope::Node)
                } else {
                    self.kernel.world.fillet_named(&id, &refs, num_param(p, "radius")?, EditScope::Node)
                }
                .map_err(kernel)?;
                self.edit_result(&id, &before, changes)
            }
            "subtract" | "union" | "intersect" => {
                let id = str_param(p, "id")?.to_string();
                let tools: Vec<String> = serde_json::from_value(p.get("tools").cloned().unwrap_or(Value::Null))
                    .map_err(|_| invalid("`tools` must be a list of body ids"))?;
                let operation = match method {
                    "subtract" => ModifyingOperation::Subtract,
                    "union" => ModifyingOperation::Union,
                    _ => ModifyingOperation::Intersect,
                };
                let before: std::sync::Arc<crate::brep::BrepEnvelope> = self.kernel.world.placed_brep(&id).map_err(kernel)?;
                let changes = self.kernel.operate(&id, operation, &tools, EditScope::Node).map_err(kernel)?;
                self.edit_result(&id, &before, changes)
            }
            "faces" => {
                let brep = self.kernel.world.placed_brep(str_param(p, "id")?).map_err(kernel)?;
                Ok(json!({"text": perception::overview(&brep).map_err(kernel)?}))
            }
            "edges" => {
                let brep = self.kernel.world.placed_brep(str_param(p, "id")?).map_err(kernel)?;
                let names = crate::naming::edge_names(&brep);
                Ok(json!({"edges": names, "text": names.join("\n")}))
            }
            "here" => {
                let brep = self.kernel.world.placed_brep(str_param(p, "id")?).map_err(kernel)?;
                Ok(json!({"text": perception::here(&brep, str_param(p, "face")?).map_err(kernel)?}))
            }
            "ray" => {
                let id = str_param(p, "id")?;
                let origin = point_param(p, "origin")?;
                let hits = self.kernel.ray(id, origin, point_param(p, "direction")?).map_err(kernel)?;
                let placed = self.kernel.world.placed_brep(id).map_err(kernel)?;
                let inside = crate::query::classify_point(&placed, origin).map_err(kernel)? == PointClassification::Inside;
                let list: Vec<Value> = hits
                    .iter()
                    .map(|h| json!({"t": h.t, "face": h.face, "entering": h.entering, "point": h.point}))
                    .collect();
                Ok(json!({"hits": list, "text": perception::sentence(&hits, inside)}))
            }
            "find" => {
                let id = str_param(p, "id")?.to_string();
                let k = p.get("k").and_then(Value::as_u64).unwrap_or(5) as usize;
                let query = Query::parse(str_param(p, "query")?);
                let found = self.index(&id)?.find(&query, k);
                let text = found.iter().map(|(n, s)| format!("{n} ({s:.2})")).collect::<Vec<_>>().join("\n");
                Ok(json!({"matches": found, "text": text}))
            }
            "keys" => {
                let id = str_param(p, "id")?.to_string();
                let face = str_param(p, "face")?;
                let index = self.index(&id)?;
                let i = index.elements.iter().position(|e| e == face).ok_or_else(|| invalid("no such face"))?;
                let keys = index.element_keys[i].clone();
                let near = index.neighbours(i).join(", ");
                Ok(json!({"keys": keys, "text": format!("{}\nnear: {near}", keys.join(" "))}))
            }
            "measure" => {
                let m = self.kernel.world.measure(str_param(p, "id")?, None).map_err(kernel)?;
                Ok(json!({"volume": m.volume, "area": m.area, "text": format!("volume {:.6} area {:.6}", m.volume, m.area)}))
            }
            "mark" => {
                let mark = self.kernel.world.mark().map_err(kernel)?;
                self.next_mark += 1;
                self.marks.insert(self.next_mark, mark);
                Ok(json!({"mark": self.next_mark}))
            }
            "rollback" => {
                let key = p.get("mark").and_then(Value::as_u64).ok_or_else(|| invalid("missing `mark`"))?;
                let mark = self.marks.remove(&key).ok_or_else(|| invalid("unknown mark"))?;
                Ok(json!({"changes": self.kernel.world.rollback(mark).map_err(kernel)?}))
            }
            "changes" => {
                let since = p.get("since").and_then(Value::as_u64).unwrap_or(0);
                Ok(json!({"changes": self.kernel.world.changes_since(since).map_err(kernel)?, "revision": self.kernel.world.revision()}))
            }
            _ => Err(RpcError { code: -32601, message: format!("unknown method `{method}`") }),
        }
    }

    fn edit_result(
        &mut self,
        id: &str,
        before: &crate::brep::BrepEnvelope,
        changes: ChangeSet,
    ) -> Result<Value, RpcError> {
        let after = self.kernel.world.placed_brep(id).map_err(kernel)?;
        let (added, removed) = perception::face_delta(before, &after);
        let text = format!(
            "ok. new faces: {}. gone: {}.",
            if added.is_empty() { "-".into() } else { added.join(", ") },
            if removed.is_empty() { "-".into() } else { removed.join(", ") }
        );
        Ok(json!({"changes": changes, "added": added, "removed": removed, "text": text}))
    }

    /// The key layer is refreshed only when the body's shape revision changed.
    fn index(&mut self, id: &str) -> Result<&Index, RpcError> {
        self.kernel.refresh(id).map_err(kernel)?;
        self.kernel
            .layer::<KeyLayer>()
            .and_then(|l| l.index(id))
            .ok_or_else(|| invalid("the key layer is not installed in this kernel"))
    }
}

#[allow(clippy::panic)]
fn unreachable_kernel() -> Session {
    panic!("standard accuracy is always valid")
}
