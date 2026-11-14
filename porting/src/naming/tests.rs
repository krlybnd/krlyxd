use crate::brep::Accuracy;
use crate::world_graph::{CreateOptions, EditScope, Primitive, WorldGraph};

fn block() -> (WorldGraph, String) {
    let mut w = WorldGraph::new(Accuracy::STANDARD).unwrap();
    let (id, _) = w
        .create_primitive(
            Primitive::Cuboid { width: 1.0, height: 2.0, depth: 3.0 },
            CreateOptions { node_id: Some("block".into()), ..CreateOptions::default() },
        )
        .unwrap();
    (w, id)
}

fn face_names(w: &WorldGraph, id: &str) -> Vec<String> {
    let mut v: Vec<String> = w.brep(id).unwrap().topology.faces.iter().map(|f| f.key.clone()).collect();
    v.sort();
    v
}

#[test]
fn edges_are_named_by_their_faces() {
    let (w, id) = block();
    let names = super::edge_names(&w.brep(&id).unwrap());
    assert_eq!(names.len(), 12);
    assert!(names.contains(&"front|top".to_string()) || names.contains(&"back|top".to_string()));
    let unique: std::collections::BTreeSet<_> = names.iter().collect();
    assert_eq!(unique.len(), 12, "{names:?}");
}

#[test]
fn chamfer_keeps_face_names_and_names_the_new_face() {
    let (mut w, id) = block();
    let edge = super::edge_names(&w.brep(&id).unwrap())
        .into_iter()
        .find(|n| n.contains("top"))
        .unwrap();
    w.chamfer_named(&id, &[edge.as_str()], 0.2, EditScope::Node).unwrap();
    let mut expected = vec!["back", "bottom", "front", "left", "right", "top"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    expected.push(format!("chamfer({edge})"));
    expected.sort();
    assert_eq!(face_names(&w, &id), expected);
    // the chamfered edge is gone, the new face's edges are addressable by name
    let after = super::edge_names(&w.brep(&id).unwrap());
    assert!(!after.contains(&edge));
    assert!(after.iter().any(|n| n.contains("chamfer(")));
}

#[test]
fn fillet_keeps_face_names() {
    let (mut w, id) = block();
    let edge = super::edge_names(&w.brep(&id).unwrap())
        .into_iter()
        .find(|n| n.contains("top"))
        .unwrap();
    w.fillet_named(&id, &[edge.as_str()], 0.2, EditScope::Node).unwrap();
    let names = face_names(&w, &id);
    for original in ["back", "bottom", "front", "left", "right", "top"] {
        assert!(names.contains(&original.to_string()), "{original} lost: {names:?}");
    }
    assert!(names.contains(&format!("fillet({edge})")), "{names:?}");
}

#[test]
fn chained_blends_stay_addressable() {
    let (mut w, id) = block();
    let names = super::edge_names(&w.brep(&id).unwrap());
    let first = names.iter().find(|n| n.contains("top")).unwrap().clone();
    w.chamfer_named(&id, &[first.as_str()], 0.2, EditScope::Node).unwrap();
    let second = super::edge_names(&w.brep(&id).unwrap())
        .into_iter()
        .find(|n| n.contains("bottom") && !n.contains("chamfer"))
        .unwrap();
    w.chamfer_named(&id, &[second.as_str()], 0.1, EditScope::Node).unwrap();
    let names = face_names(&w, &id);
    assert!(names.contains(&format!("chamfer({first})")), "{names:?}");
    assert!(names.contains(&format!("chamfer({second})")), "{names:?}");
}
