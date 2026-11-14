use super::{Attempt, BooleanRequest, BooleanStrategy, Chain, Legacy, Named};
use crate::kernel::Kernel;
use crate::world_graph::{CreateOptions, EditScope, ModifyingOperation, Plane, Primitive};

fn at(p: [f64; 3]) -> Option<Plane> {
    Some(Plane { origin: Some(p), normal: Some([0.0, 1.0, 0.0]), x_direction: Some([1.0, 0.0, 0.0]) })
}

fn block_and_drill(kernel: &mut Kernel) {
    let w = kernel.world_mut();
    let o = |id: &str, p| CreateOptions { node_id: Some(id.into()), plane: at(p), ..Default::default() };
    w.create_primitive(Primitive::Cuboid { width: 4.0, height: 2.0, depth: 3.0 }, o("block", [0.0, 0.0, 0.0])).unwrap();
    w.create_primitive(Primitive::Cylinder { radius: 0.5, height: 3.0 }, o("drill", [0.5, -0.5, 0.0])).unwrap();
}

fn names(kernel: &Kernel, id: &str) -> Vec<String> {
    let mut v: Vec<String> = kernel.world().brep(id).unwrap().topology.faces.iter().map(|f| f.key.clone()).collect();
    v.sort();
    v
}

#[test]
fn a_drilled_block_keeps_its_face_names_and_names_the_hole() {
    let mut kernel = Kernel::builder().build().unwrap();
    block_and_drill(&mut kernel);
    kernel.operate("block", ModifyingOperation::Subtract, &["drill".to_string()], EditScope::Node).unwrap();
    let names = names(&kernel, "block");
    for kept in ["top", "bottom", "left", "right", "front", "back"] {
        assert!(names.iter().any(|n| n == kept || n.starts_with(&format!("{kept}#"))), "{kept} lost: {names:?}");
    }
    assert!(names.iter().any(|n| n.starts_with("drill:")), "the hole wall is named after the tool: {names:?}");
    assert!(!names.iter().any(|n| n.starts_with("vertical-") || n.starts_with("face-")), "no anonymous faces: {names:?}");
    // the edges of the hole are therefore addressable
    let edges = crate::naming::edge_names(&kernel.world().brep("block").unwrap());
    assert!(edges.iter().any(|e| e.contains("drill:") && e.contains("top")), "{edges:?}");
}

struct Refuses;
impl BooleanStrategy for Refuses {
    fn name(&self) -> &'static str { "refuses" }
    fn apply(&self, _: &BooleanRequest) -> Attempt { Attempt::Declined("never my case".into()) }
}

struct KeepsTarget;
impl BooleanStrategy for KeepsTarget {
    fn name(&self) -> &'static str { "keeps-target" }
    fn apply(&self, request: &BooleanRequest) -> Attempt { Attempt::Done(Box::new(request.target.clone())) }
}

#[test]
fn a_chain_falls_through_to_the_next_strategy() {
    let mut kernel = Kernel::builder().boolean(Named(Chain::new().then(Refuses).then(Legacy))).build().unwrap();
    block_and_drill(&mut kernel);
    kernel.operate("block", ModifyingOperation::Subtract, &["drill".to_string()], EditScope::Node).unwrap();
    assert!(names(&kernel, "block").iter().any(|n| n.starts_with("drill:")));
}

#[test]
fn a_new_strategy_is_added_without_touching_the_others() {
    let mut kernel = Kernel::builder().boolean(Chain::new().then(KeepsTarget).then(Legacy)).build().unwrap();
    block_and_drill(&mut kernel);
    let before = kernel.world().measure("block", None).unwrap().volume;
    kernel.operate("block", ModifyingOperation::Subtract, &["drill".to_string()], EditScope::Node).unwrap();
    let after = kernel.world().measure("block", None).unwrap().volume;
    assert!((before - after).abs() < 1e-12, "the injected strategy answered first");
}

#[test]
fn an_empty_chain_declines_with_a_reason() {
    let mut kernel = Kernel::builder().boolean(Chain::new().then(Refuses)).build().unwrap();
    block_and_drill(&mut kernel);
    let error = kernel.operate("block", ModifyingOperation::Subtract, &["drill".to_string()], EditScope::Node).unwrap_err();
    assert!(format!("{error:?}").contains("never my case"), "{error:?}");
}

#[test]
fn the_protocol_cuts_through_the_injected_strategy() {
    use serde_json::json;
    let mut s = crate::protocol::Session::new();
    s.call("create", &json!({"id":"block","primitive":{"kind":"Cuboid","width":4,"height":2,"depth":3}})).unwrap();
    s.call("create", &json!({"id":"drill","at":[0.5,-0.5,0.0],"primitive":{"kind":"Cylinder","radius":0.5,"height":3}})).unwrap();
    let r = s.call("subtract", &json!({"id":"block","tools":["drill"]})).unwrap();
    assert!(r["added"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().starts_with("drill:")), "{r}");
}
