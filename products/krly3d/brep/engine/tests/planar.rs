#![allow(clippy::unwrap_used)]

use krly3d_brep::planar::{decode_call, encode_result, PlanarCall, PlanarInput, PlanarResult, PROTOCOL};
use krly3d_brep::Diagnostic;
use krly3d_brep_engine::{build_planar, build_planar_retained, BrepEngine, Snapshot};
use krly3d_core::math::Point3;
use krly3d_kernel::ports::interpret::{body_of, mesh_of};
use krly3d_kernel::ports::measure::measure;
use krly3d_kernel::ports::trim::{place, Place, TrimRequest};
use krly3d_kernel::topo::check;
use serde_json::Value;
use std::sync::Arc;

fn call(polygons: Vec<Vec<[f64; 2]>>, height: f64) -> PlanarCall {
    PlanarCall {
        protocol: PROTOCOL.into(),
        request_id: "engine-planar".into(),
        input: PlanarInput {
            polygons,
            height,
            length_unit: "model".into(),
            source: "caller:unverified-image-claim".into(),
        },
    }
}

fn rectangle(x: f64, y: f64, w: f64, d: f64) -> Vec<[f64; 2]> {
    vec![[x, y], [x + w, y], [x + w, y + d], [x, y + d]]
}

fn build_with_retained_parity(call: PlanarCall) -> PlanarResult {
    let retained = build_planar_retained(call.clone()).unwrap();
    assert!(retained.engine.owns(&retained.snapshot));
    assert_eq!(retained.request_id, call.request_id);
    assert_eq!(retained.source, call.input.source);
    let result = build_planar(call).unwrap();
    assert_eq!(result.model, retained.engine.to_k3d().unwrap());
    assert_eq!(result.node, retained.snapshot.identity().node);
    assert_eq!(result.quality, retained.quality);
    assert_eq!(result.provenance.build_key, retained.snapshot.identity().build_key);
    assert_ne!(result.provenance.session, retained.snapshot.identity().session);
    result
}

fn reload(call: PlanarCall, expected_volume: f64) -> Snapshot {
    let request_id = call.request_id.clone();
    let source = call.input.source.clone();
    let result = build_with_retained_parity(call);
    assert_eq!(result.protocol, PROTOCOL);
    assert_eq!(result.request_id, request_id);
    assert_eq!(result.node, "part@1");
    assert_eq!(result.source, source);
    let wire: Value = serde_json::from_str(&encode_result(&result).unwrap()).unwrap();
    assert!(wire["quality"]["positional_uncertainty"].is_null());
    assert!(wire["quality"]["confidence"].is_null());
    assert!(wire["quality"]["assumptions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|text| text.as_str().unwrap().contains("Uniform extrusion")));
    let engine = BrepEngine::from_k3d(&result.model).unwrap();
    let statements = &engine.kernel().document().layers[0].statements;
    assert_eq!(statements.len(), 2);
    assert_eq!(statements[0].name, "outline_profile");
    assert_eq!(statements[0].op.tool.as_str(), "profile");
    assert_eq!(statements[1].name, "part");
    assert_eq!(statements[1].op.tool.as_str(), "extrude");
    assert_eq!(statements[1].op.inputs[0].to_string(), "outline_profile");
    let snapshot = engine.snapshot("part@1").unwrap();
    assert_eq!(snapshot.identity().build_key, result.provenance.build_key);
    assert_ne!(snapshot.identity().session, result.provenance.session);
    let cx = snapshot.legacy_context();
    let body = body_of(cx, snapshot.legacy_shape()).into_result().unwrap();
    check(&body, cx.accuracy()).unwrap();
    assert!(body.solid);
    let volume = measure(cx, "volume", &body).into_result().unwrap().value;
    assert!((volume - expected_volume).abs() < 1e-8, "{volume} vs {expected_volume}");
    let mesh = mesh_of(cx, snapshot.legacy_shape(), 0.01).into_result().unwrap();
    assert!(mesh.open_sides().is_empty());
    assert!((mesh.volume() - expected_volume).abs() < 1e-8);
    snapshot
}

fn rejection(call: PlanarCall) -> Diagnostic {
    let retained = build_planar_retained(call.clone()).err().unwrap();
    let adapted = build_planar(call).unwrap_err();
    assert_eq!(retained, adapted);
    adapted
}

#[test]
fn retained_snapshot_pins_the_native_shape_and_meshes_after_owner_drop_without_export() {
    let retained = krly3d_brep_engine::planar::build_planar_retained(call(
        vec![rectangle(10.0, 20.0, 10.0, 8.0), rectangle(12.0, 22.0, 2.0, 2.0)],
        3.0,
    ))
    .unwrap();
    let snapshot = retained.snapshot.clone();
    let identity = snapshot.identity().clone();
    let engine = retained.engine;
    assert!(engine.owns(&snapshot));
    assert_eq!(snapshot.identity(), engine.snapshot("part@1").unwrap().identity());
    assert!(Arc::ptr_eq(snapshot.legacy_shape(), engine.snapshot("part@1").unwrap().legacy_shape()));
    drop(retained.snapshot);
    drop(engine);

    std::thread::spawn(move || {
        assert_eq!(snapshot.identity(), &identity);
        let cx = snapshot.legacy_context();
        let body = body_of(cx, snapshot.legacy_shape()).into_result().unwrap();
        check(&body, cx.accuracy()).unwrap();
        assert!(body.solid);
        let volume = measure(cx, "volume", &body).into_result().unwrap().value;
        assert!((volume - 228.0).abs() < 1e-8);
        let mesh = mesh_of(cx, snapshot.legacy_shape(), 0.01).into_result().unwrap();
        assert!(mesh.open_sides().is_empty());
        assert!((mesh.volume() - 228.0).abs() < 1e-8);
    })
    .join()
    .unwrap();
}

#[test]
fn nonzero_coordinates_and_height_survive_native_save_reload_and_meshing() {
    let snapshot = reload(call(vec![rectangle(10.0, -4.0, 6.0, 4.0)], 2.0), 48.0);
    let body = body_of(snapshot.legacy_context(), snapshot.legacy_shape()).into_result().unwrap();
    assert_eq!((body.faces.len(), body.edges.len(), body.vertices.len()), (6, 12, 8));
    for point in &body.vertices {
        assert!(point.x == 10.0 || point.x == 16.0);
        assert!(point.y == -4.0 || point.y == 0.0);
        assert!(point.z == 0.0 || point.z == 2.0);
    }
}

#[test]
fn holes_are_through_holes_for_either_winding_and_reload_as_closed_solids() {
    for reverse_outer in [false, true] {
        for reverse_hole in [false, true] {
            let mut outer = rectangle(10.0, 20.0, 10.0, 8.0);
            let mut hole = rectangle(12.0, 22.0, 2.0, 2.0);
            if reverse_outer {
                outer.reverse();
            }
            if reverse_hole {
                hole.reverse();
            }
            let snapshot = reload(call(vec![outer, hole], 3.0), 228.0);
            let cx = snapshot.legacy_context();
            let body = body_of(cx, snapshot.legacy_shape()).into_result().unwrap();
            let (top, face) = body.face("top").unwrap();
            assert_eq!(face.loops.len(), 2);
            let at_hole = place(
                cx,
                &TrimRequest { body: Arc::clone(&body), face: top, point: Point3::new(13.0, 23.0, 3.0) },
            )
            .into_result()
            .unwrap();
            assert_eq!(at_hole, Place::Outside);
            let material = place(
                cx,
                &TrimRequest { body: Arc::clone(&body), face: top, point: Point3::new(11.0, 21.0, 3.0) },
            )
            .into_result()
            .unwrap();
            assert_eq!(material, Place::Inside);
        }
    }
}

#[test]
fn multiple_holes_and_concave_contours_use_native_geometry() {
    reload(
        call(
            vec![
                rectangle(10.0, 20.0, 10.0, 8.0),
                rectangle(12.0, 22.0, 2.0, 2.0),
                rectangle(16.0, 24.0, 3.0, 3.0),
            ],
            3.0,
        ),
        201.0,
    );
    reload(
        call(
            vec![vec![[10.0, 20.0], [14.0, 20.0], [14.0, 22.0], [12.0, 22.0], [12.0, 24.0], [10.0, 24.0]]],
            3.0,
        ),
        36.0,
    );
}

#[test]
fn repeated_closing_vertex_and_collinear_boundary_vertices_are_supported() {
    let closed = vec![[10.0, 20.0], [12.0, 20.0], [14.0, 20.0], [14.0, 24.0], [10.0, 24.0], [10.0, 20.0]];
    reload(call(vec![closed], 2.0), 32.0);
}

#[test]
fn calls_are_stateless_and_deterministic_except_for_snapshot_session_provenance() {
    let request = call(vec![rectangle(10.0, -4.0, 6.0, 4.0)], 2.0);
    let a = build_planar(request.clone()).unwrap();
    let b = build_planar(request).unwrap();
    assert_eq!(a.model, b.model);
    assert_eq!(a.node, b.node);
    assert_eq!(a.source, b.source);
    assert_eq!(a.quality, b.quality);
    assert_eq!(a.provenance.build_key, b.provenance.build_key);
    assert_ne!(a.provenance.session, b.provenance.session);
    assert_ne!(a.provenance.source, b.provenance.source);
}

#[test]
fn outside_touching_crossing_overlapping_nested_or_reversed_outer_holes_are_refused() {
    let outer = rectangle(10.0, 20.0, 10.0, 10.0);
    let invalid = [
        vec![outer.clone(), rectangle(21.0, 21.0, 1.0, 1.0)],
        vec![outer.clone(), rectangle(10.0, 22.0, 2.0, 2.0)],
        vec![outer.clone(), rectangle(19.0, 22.0, 2.0, 2.0)],
        vec![outer.clone(), rectangle(12.0, 22.0, 4.0, 4.0), rectangle(14.0, 24.0, 4.0, 4.0)],
        vec![outer.clone(), rectangle(12.0, 22.0, 4.0, 4.0), rectangle(13.0, 23.0, 1.0, 1.0)],
        vec![outer.clone(), rectangle(12.0, 22.0, 2.0, 2.0), rectangle(14.0, 22.0, 2.0, 2.0)],
        vec![rectangle(12.0, 22.0, 2.0, 2.0), outer],
    ];
    for polygons in invalid {
        let error = rejection(call(polygons, 2.0));
        assert_eq!(error.code.as_str(), "unsupported", "{error:?}");
        assert!(error.message.contains("native planar providers"));
    }
}

#[test]
fn self_crossing_zero_area_and_collapsed_profiles_do_not_publish_a_result() {
    for polygon in [
        vec![[0.0, 0.0], [4.0, 4.0], [0.0, 3.0], [3.0, 0.0]],
        vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]],
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
    ] {
        rejection(call(vec![polygon], 2.0));
    }
}

#[test]
fn engine_revalidates_direct_rust_calls_including_numeric_extremes() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e308, -1.0, 0.0] {
        let mut request = call(vec![rectangle(10.0, 20.0, 6.0, 4.0)], value);
        rejection(request.clone());
        request.input.height = 2.0;
        request.input.polygons[0][0][0] = value;
        if !value.is_finite() || value.abs() > 1e9 {
            rejection(request);
        }
    }
    let mut request = call(vec![rectangle(10.0, 20.0, 6.0, 4.0)], 2.0);
    request.protocol = "wrong".into();
    rejection(request);
    let mut request = call(vec![rectangle(10.0, 20.0, 6.0, 4.0)], 2.0);
    request.input.length_unit = "metre".into();
    rejection(request);
    let mut request = call(vec![rectangle(10.0, 20.0, 6.0, 4.0)], 2.0);
    request.input.source.clear();
    rejection(request);
    let raw = r#"{"protocol":"krly3d.brep.planar/1","request_id":"wire","input":{"polygons":[[[10,20],[16,20],[16,24],[10,24]]],"height":2,"length_unit":"model","source":"wire-claim"}}"#;
    assert!(build_planar(decode_call(raw).unwrap()).is_ok());
}
