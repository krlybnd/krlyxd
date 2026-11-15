#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::{CallControl, Error};
use krly3d_kernel::model::{Document, Statement, GEOMETRY};
use krly3d_kernel::ports::query::ask;
use krly3d_kernel::schema::{Params, Value};
use krly3d_kernel::Kernel;
use krly3d_view::ports::render::Render;
use krly3d_view_api::{
    ApiVersion, Backend, Camera, Diagnostic, Exactness, FaceCoverage, Id, NamedView, RenderCall, RenderInput,
    RequestId, ENGINE_REVISION,
};
use krly3d_view_renderer::ViewEngine;
use std::io::Cursor;
use std::sync::Arc;

const BOX: &str = "k3d 1\nlayer geo\npart = box w=6 d=4 h=2\n";

fn document(text: &str) -> Document {
    krly3d_core::document::text::parse(text, &[GEOMETRY]).unwrap()
}

fn block(width: f64) -> Statement {
    Statement::new("part", "box", Params::new().with("w", width).with("d", 4.0).with("h", 2.0), &[]).unwrap()
}

fn call(backend: Backend) -> RenderCall {
    let mut input = RenderInput::new("part");
    input.width = 80;
    input.height = 60;
    input.backend = backend;
    RenderCall::new(RequestId::new("render-test"), input)
}

fn legacy(text: &str) -> Kernel {
    let mut builder = krly3d_std::standard();
    krly3d_view::register(&mut builder);
    builder.plugin::<Render>(krly3d_view_mesh::Raster);
    let mut kernel = builder.build();
    assert!(!kernel.open(document(text)).unwrap().refused());
    kernel
}

fn entry<'a>(value: &'a Value, name: &str) -> &'a Value {
    let Value::Map(entries) = value else { panic!("not a map: {value:?}") };
    entries.get(name).unwrap()
}

fn face_counts(value: &Value) -> Vec<FaceCoverage> {
    let Value::List(faces) = entry(value, "faces") else { panic!("no faces") };
    let mut counts: Vec<_> = faces
        .iter()
        .map(|face| {
            let Value::Text(name) = entry(face, "face") else { panic!("no face name") };
            let Value::Number(pixels) = entry(face, "pixels") else { panic!("no pixel count") };
            FaceCoverage { name: name.clone(), pixels: *pixels as u64 }
        })
        .collect();
    counts.sort_by(|a, b| a.name.cmp(&b.name));
    counts
}

fn camera_pairs() -> Vec<(Camera, Value)> {
    let mut pairs: Vec<_> = [
        NamedView::Iso,
        NamedView::Front,
        NamedView::Back,
        NamedView::Left,
        NamedView::Right,
        NamedView::Top,
        NamedView::Bottom,
    ]
    .into_iter()
    .map(|name| (Camera::Named(name), Value::from(name.as_str())))
    .collect();
    pairs.push((
        Camera::Orthographic { toward: [2.0, -3.0, -1.0], up: [0.0, 0.0, 1.0] },
        Value::from([2.0, -3.0, -1.0]),
    ));
    for centre in [None, Some([35.25, 20.75])] {
        let mut fields = vec![
            ("eye", Value::from([10.0, -14.0, 9.0])),
            ("toward", Value::from([-10.0, 14.0, -8.0])),
            ("up", Value::from([0.0, 0.0, 1.0])),
            ("focal", Value::from(70.0)),
        ];
        if let Some(centre) = centre {
            fields.push(("centre", Value::List(centre.into_iter().map(Value::from).collect())));
        }
        pairs.push((
            Camera::Pinhole {
                eye: [10.0, -14.0, 9.0],
                toward: [-10.0, 14.0, -8.0],
                up: [0.0, 0.0, 1.0],
                focal: 70.0,
                centre,
            },
            Value::Map(fields.into_iter().map(|(name, value)| (name.to_string(), value)).collect()),
        ));
    }
    pairs
}

#[test]
fn every_camera_matches_legacy_png_and_face_counts_on_both_backends() {
    // The typed owner deliberately has no view or render plugins registered.
    let engine = ViewEngine::from_k3d(BOX).unwrap();
    assert!(engine.brep().kernel().context().registry::<Render>().is_none());
    let legacy = legacy(BOX);
    let (document, model) = legacy.snapshot();
    for backend in [Backend::Raycast, Backend::Mesh] {
        for (camera, old_camera) in camera_pairs() {
            let mut call = call(backend);
            call.input.camera = camera;
            let params = Params::new()
                .with("nodes", "part")
                .with("view", old_camera)
                .with("width", call.input.width as usize)
                .with("height", call.input.height as usize)
                .with("renderer", if backend == Backend::Mesh { "mesh" } else { "raycast" });
            let result = engine.call(call.clone()).unwrap();
            let picture =
                ask(legacy.context(), &Id::new("render"), &params, &document, &model).into_result().unwrap();
            let visible =
                ask(legacy.context(), &Id::new("view"), &params, &document, &model).into_result().unwrap();
            assert_eq!(entry(&picture.data, "png"), &Value::from(krly3d_view::base64::encode(&result.png)));
            assert_eq!(result.faces, face_counts(&visible.data));
            assert!(!result.faces.is_empty(), "{:?}", call.input.camera);
            let mut reader = png::Decoder::new(Cursor::new(&result.png)).read_info().unwrap();
            let mut pixels = vec![0; reader.output_buffer_size()];
            let frame = reader.next_frame(&mut pixels).unwrap();
            assert_eq!((frame.width, frame.height), (call.input.width, call.input.height));
            assert_eq!(frame.color_type, png::ColorType::Rgb);
            assert_eq!(result.request_id, call.request_id);
            assert_eq!(result.quality.exactness, Exactness::Sampled { samples: 80 * 60 });
            assert_eq!(
                result.quality.requested_tolerance,
                (backend == Backend::Mesh).then_some(call.input.tolerance)
            );
            assert_eq!(result.quality.positional_uncertainty, None);
            assert_eq!(result.quality.confidence, None);
            call.input.node = "part@1".into();
            assert_eq!(result.provenance.input, call.input);
            assert_eq!(result.provenance.engine_revision.as_str(), ENGINE_REVISION);
            assert_eq!(&result.provenance.source, engine.brep().snapshot("part").unwrap().identity());
        }
    }
}

#[test]
fn prepared_latest_and_explicit_versions_survive_edits_rollback_branching_and_open() {
    for backend in [Backend::Raycast, Backend::Mesh] {
        let mut engine = ViewEngine::from_k3d(BOX).unwrap();
        let first = engine.call(call(backend)).unwrap();
        let pinned_first = engine.prepare(call(backend)).unwrap();
        let checkpoint = engine.brep().checkpoint();
        engine.brep_mut().apply(block(12.0)).unwrap();
        let second = engine.call(call(backend)).unwrap();
        let pinned_second = engine.prepare(call(backend)).unwrap();
        let mut explicit = call(backend);
        explicit.input.node = "part@0001".into();
        let old = engine.call(explicit).unwrap();
        assert_eq!(old.png, first.png);
        assert_eq!(old.provenance, first.provenance);
        assert_ne!(second.png, first.png);
        assert_ne!(second.provenance.source.source, first.provenance.source.source);
        assert_eq!(second.provenance.input.node, "part@2");
        engine.brep_mut().rollback(checkpoint);
        assert!(engine.brep().snapshot("part@2").is_err());
        assert_eq!(engine.call(call(backend)).unwrap().provenance, first.provenance);
        engine.brep_mut().apply(block(3.0)).unwrap();
        let branch = engine.call(call(backend)).unwrap();
        assert_eq!(branch.provenance.source.node, second.provenance.source.node);
        assert_ne!(branch.provenance.source.source, second.provenance.source.source);
        assert_ne!(branch.png, second.png);
        engine.brep_mut().open(document("k3d 1\nlayer geo\nother = sphere r=2\n")).unwrap();
        assert!(engine.brep().snapshot("part").is_err());
        let retained_first = engine.execute(pinned_first).unwrap();
        let retained_second = engine.execute(pinned_second).unwrap();
        assert_eq!((retained_first.png, retained_first.faces), (first.png, first.faces));
        assert_eq!(retained_first.provenance, first.provenance);
        assert_eq!((retained_second.png, retained_second.faces), (second.png, second.faces));
        assert_eq!(retained_second.provenance, second.provenance);
    }
}

#[test]
fn prepared_work_rejects_other_renderers_and_replaced_or_transferred_brep_owners() {
    let mut first = ViewEngine::from_k3d(BOX).unwrap();
    let mut other = ViewEngine::from_k3d(BOX).unwrap();
    let request = call(Backend::Mesh);
    let a = first.call(request.clone()).unwrap();
    let b = other.call(request.clone()).unwrap();
    assert_eq!(a.png, b.png);
    assert_eq!(a.provenance.source.build_key, b.provenance.source.build_key);
    assert_ne!(a.provenance.source.session, b.provenance.source.session);
    assert_ne!(a.provenance.source.source, b.provenance.source.source);
    let prepared = first.prepare(request.clone()).unwrap();
    assert_eq!(other.execute(prepared).unwrap_err().code.as_str(), "input");
    let prepared = first.prepare(request.clone()).unwrap();
    std::mem::swap(first.brep_mut(), other.brep_mut());
    // One renderer has the correct identity, the other has the original B-rep:
    // neither alone is sufficient authorization to execute this handle.
    assert_eq!(other.execute(prepared).unwrap_err().code.as_str(), "input");
    let prepared = first.prepare(request.clone()).unwrap();
    *first.brep_mut() = krly3d_brep_engine::BrepEngine::from_k3d(BOX).unwrap();
    assert_eq!(first.execute(prepared).unwrap_err().code.as_str(), "input");
    assert!(first.call(request).is_ok());
}

fn assert_legacy(diagnostic: Diagnostic, error: Error) {
    assert_eq!(diagnostic.code.as_str(), error.kind.label());
    assert_eq!(diagnostic.message, error.message);
}

#[test]
fn invalid_inputs_and_api_errors_propagate_without_a_result_and_valid_retry_succeeds() {
    let engine = ViewEngine::from_k3d(BOX).unwrap();
    for node in ["part@0", "part@two", "part#top", "missing", "part@3"] {
        let mut request = call(Backend::Raycast);
        request.input.node = node.into();
        assert_legacy(engine.call(request).unwrap_err(), engine.brep().kernel().body(node).unwrap_err());
    }
    for dimensions in [(0, 60), (80, 0), (2049, 60), (80, 2049), (u32::MAX, u32::MAX)] {
        let mut request = call(Backend::Mesh);
        (request.input.width, request.input.height) = dimensions;
        let expected = request.validate().unwrap_err();
        assert_eq!(engine.prepare(request).unwrap_err(), expected);
    }
    let mut unsupported = call(Backend::Raycast);
    unsupported.api = ApiVersion { major: 1, minor: 1 };
    assert_eq!(engine.call(unsupported).unwrap_err().code.as_str(), "unsupported_version");
    for backend in [Backend::Raycast, Backend::Mesh] {
        let mut parallel = call(backend);
        parallel.input.camera = Camera::Orthographic { toward: [0.0, 1.0, 0.0], up: [0.0, 2.0, 0.0] };
        assert_eq!(engine.call(parallel).unwrap_err().code.as_str(), "input");
        let mut invalid = call(backend);
        invalid.input.tolerance.value = f64::NAN;
        assert_eq!(engine.call(invalid).unwrap_err().code.as_str(), "input");
        for factor in [0.5, 1.0] {
            let mut tiny = call(backend);
            tiny.input.tolerance.value = engine.brep().kernel().context().accuracy().length() * factor;
            if backend == Backend::Mesh {
                let error = engine.prepare(tiny).unwrap_err();
                assert_eq!(error.code.as_str(), "input");
                assert_eq!(
                    error.message,
                    "a mesh preview tolerance must be finite and above the length tolerance"
                );
            } else {
                assert_eq!(engine.call(tiny).unwrap().quality.requested_tolerance, None);
            }
        }
        let valid = engine.call(call(backend)).unwrap();
        assert!(!valid.png.is_empty());
        assert!(!valid.faces.is_empty());
    }
    for text in ["", "not k3d", "k3d 1\nlayer geo\npart = box w=1 d=2 h=3\n  expect volume=999\n"] {
        assert_eq!(
            ViewEngine::from_k3d(text).err().unwrap(),
            krly3d_brep_engine::BrepEngine::from_k3d(text).err().unwrap()
        );
    }
    let cancelled = Arc::new(CallControl::new());
    cancelled.cancel();
    assert_eq!(
        engine
            .execute_controlled(engine.prepare(call(Backend::Mesh)).unwrap(), cancelled)
            .unwrap_err()
            .code
            .as_str(),
        "cancelled"
    );
}
