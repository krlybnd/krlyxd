#![allow(clippy::unwrap_used)]

use krly3d_mesh::{MeshCall, MeshInput, RequestId};
use krly3d_mesh_engine::MeshEngine;

#[test]
fn native_document_versions_are_available_to_both_transport_roots() {
    let engine =
        MeshEngine::from_k3d("k3d 1\nlayer geo\npart = box w=6 d=4 h=2\npart = box w=3 d=4 h=2\n").unwrap();
    for (node, volume) in [("part@1", 48.0), ("part", 24.0)] {
        let result = engine.call(MeshCall::new(RequestId::new(node), MeshInput::new(node))).unwrap();
        assert_eq!(result.volume, volume);
        assert_eq!(result.open, 0);
    }
}

#[test]
fn invalid_native_documents_fail_before_an_engine_is_exposed() {
    for text in [
        "not a native document",
        "k3d 1\nlayer geo\npart = box w=1\n",
        "k3d 1\nlayer geo\npart = no_such_tool\n",
        "k3d 1\nlayer geo\npart = box w=1 d=2 h=3\n  expect volume=999\n",
    ] {
        assert!(MeshEngine::from_k3d(text).is_err(), "unexpected success: {text}");
    }
}
