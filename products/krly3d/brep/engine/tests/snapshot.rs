#![allow(clippy::unwrap_used)]

use krly3d_brep::{Diagnostic, Id};
use krly3d_brep_engine::{BrepEngine, Snapshot};
use krly3d_core::{Accuracy, Attempt, Error};
use krly3d_kernel::model::{Document, NodeRef, Statement, GEOMETRY};
use krly3d_kernel::ports::interpret::{body_of, mesh_of};
use krly3d_kernel::ports::measure::measure;
use krly3d_kernel::schema::Params;
use krly3d_kernel::Kernel;
use krlyxd::{CallControl, CallStop, FakeClock};
use std::sync::Arc;
use std::time::Duration;

const BOX: &str = "k3d 1\nlayer geo\npart = box w=6 d=4 h=2\n";

fn block(width: f64) -> Statement {
    Statement::new("part", "box", Params::new().with("w", width).with("d", 4.0).with("h", 2.0), &[]).unwrap()
}

fn document(text: &str) -> Document {
    krly3d_core::document::text::parse(text, &[GEOMETRY]).unwrap()
}

fn volume(snapshot: &Snapshot) -> f64 {
    let body = body_of(snapshot.legacy_context(), snapshot.legacy_shape()).into_result().unwrap();
    measure(snapshot.legacy_context(), "volume", &body).into_result().unwrap().value
}

fn assert_volumes(snapshots: &[&Snapshot], expected: &[f64]) {
    for (snapshot, expected) in snapshots.iter().zip(expected) {
        let measured = volume(snapshot);
        assert!((measured - expected).abs() < 1e-10, "volume {measured}, expected {expected}");
    }
}

fn assert_legacy_diagnostic(diagnostic: Diagnostic, error: Error) {
    assert_eq!(diagnostic.code.as_str(), error.kind.label());
    assert_eq!(diagnostic.message, error.message);
}

#[test]
fn real_box_pins_the_built_shape_and_records_native_provenance() {
    let engine = BrepEngine::from_k3d(BOX).unwrap();
    let built_count = engine.kernel().built_nodes();
    let snapshot = engine.snapshot("part").unwrap();
    let built = engine.kernel().model().built(&NodeRef::parse("part@1").unwrap()).unwrap();
    assert!(Arc::ptr_eq(snapshot.legacy_shape(), &built.shape));
    assert_eq!(snapshot.identity().node, "part@1");
    assert_eq!(&snapshot.identity().session, engine.session_id());
    assert!(engine.session_id().as_str().starts_with("brep-session:"));
    assert_eq!(snapshot.identity().build_key, format!("{:032x}", built.key));
    assert_eq!(snapshot.identity().build_key.len(), 32);
    assert!(snapshot.identity().build_key.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
    assert_eq!(
        snapshot.identity().source,
        Id::new(&format!("{}:part@1:{}", engine.session_id(), snapshot.identity().build_key))
    );
    let body = body_of(snapshot.legacy_context(), snapshot.legacy_shape()).into_result().unwrap();
    assert!(Arc::ptr_eq(&body, &built.body));
    assert!(body.solid);
    assert_eq!(body.faces.len(), 6);
    assert_eq!(volume(&snapshot), 48.0);
    assert_eq!(measure(snapshot.legacy_context(), "area", &body).into_result().unwrap().value, 88.0);
    assert_eq!(engine.kernel().built_nodes(), built_count);
    assert_eq!(engine.snapshot("part").unwrap().identity(), snapshot.identity());
}

#[test]
fn explicit_native_export_tracks_the_current_document_and_preserves_snapshots() {
    let mut engine = BrepEngine::from_k3d(BOX).unwrap();
    let original = engine.snapshot("part").unwrap();
    let first_export = engine.to_k3d().unwrap();
    engine.apply(block(8.0)).unwrap();
    let current = engine.snapshot("part").unwrap();
    let built_count = engine.kernel().built_nodes();
    let exported = engine.to_k3d().unwrap();
    assert_ne!(exported, first_export);
    assert_eq!(engine.to_k3d().unwrap(), exported);
    assert_eq!(engine.kernel().built_nodes(), built_count);
    assert_eq!(engine.snapshot("part").unwrap().identity(), current.identity());
    assert!(engine.owns(&original));
    let reloaded = BrepEngine::from_k3d(&exported).unwrap().snapshot("part").unwrap();
    assert_eq!(reloaded.identity().build_key, current.identity().build_key);
    assert_volumes(&[&original, &current, &reloaded], &[48.0, 64.0, 64.0]);
}

#[test]
fn explicit_native_export_preserves_writer_diagnostics() {
    let engine = BrepEngine::new(krly3d_kernel::KernelBuilder::new().build()).unwrap();
    let expected =
        krly3d_kernel::ports::write::write(engine.kernel().context(), "k3d", engine.kernel().document())
            .into_result()
            .unwrap_err();
    assert_legacy_diagnostic(engine.to_k3d().unwrap_err(), expected);
}

#[test]
fn latest_and_explicit_versions_are_normalized_once() {
    let mut engine = BrepEngine::from_k3d(BOX).unwrap();
    let first = engine.snapshot("part").unwrap();
    assert_eq!(engine.snapshot("part@0001").unwrap().identity(), first.identity());
    engine.apply(block(8.0)).unwrap();
    let latest = engine.snapshot("part").unwrap();
    let explicit = engine.snapshot("part@2").unwrap();
    assert_eq!(latest.identity(), explicit.identity());
    assert_eq!(latest.identity().node, "part@2");
    assert_ne!(latest.identity().source, first.identity().source);
    assert_ne!(latest.identity().build_key, first.identity().build_key);
    assert_eq!(engine.snapshot("part@1").unwrap().identity(), first.identity());
    assert_volumes(&[&first, &latest], &[48.0, 64.0]);
}

#[test]
fn edits_rollback_branching_and_document_replacement_do_not_rebind_snapshots() {
    let mut engine = BrepEngine::from_k3d(BOX).unwrap();
    let first = engine.snapshot("part").unwrap();
    let checkpoint = engine.checkpoint();
    engine.apply(block(8.0)).unwrap();
    let edited = engine.snapshot("part").unwrap();
    let edited_identity = edited.identity().clone();
    engine.rollback(checkpoint);
    assert_eq!(engine.snapshot("part").unwrap().identity(), first.identity());
    assert!(engine.snapshot("part@2").is_err());
    assert!(engine.owns(&edited));
    engine.apply(block(3.0)).unwrap();
    let branch = engine.snapshot("part").unwrap();
    assert_eq!(branch.identity().node, edited.identity().node);
    assert_ne!(branch.identity().source, edited.identity().source);
    assert_volumes(&[&first, &edited, &branch], &[48.0, 64.0, 24.0]);
    engine.open(document("k3d 1\nlayer geo\nother = sphere r=2\n")).unwrap();
    assert!(engine.snapshot("part").is_err());
    assert_eq!(edited.identity(), &edited_identity);
    assert_volumes(&[&first, &edited, &branch], &[48.0, 64.0, 24.0]);
    assert!(engine.owns(&first) && engine.owns(&edited) && engine.owns(&branch));
}

#[test]
fn matching_documents_cannot_share_session_ownership() {
    let first = BrepEngine::from_k3d(BOX).unwrap();
    let second = BrepEngine::from_k3d(BOX).unwrap();
    let snapshot = first.snapshot("part").unwrap();
    let other = second.snapshot("part").unwrap();
    assert_ne!(first.session_id(), second.session_id());
    assert_eq!(snapshot.identity().node, other.identity().node);
    assert_eq!(snapshot.identity().build_key, other.identity().build_key);
    assert_ne!(snapshot.identity().source, other.identity().source);
    assert!(first.owns(&snapshot));
    assert!(!second.owns(&snapshot));
    assert!(!first.owns(&other));
    assert!(second.owns(&other));
}

#[test]
fn detached_snapshot_supports_cold_conversion_after_owner_drop_on_a_worker() {
    let engine = BrepEngine::from_k3d(BOX).unwrap();
    let snapshot = engine.snapshot("part").unwrap();
    let identity = snapshot.identity().clone();
    let detached = snapshot.clone();
    drop(snapshot);
    drop(engine);
    let (actual_identity, points, triangles, volume, area) = std::thread::spawn(move || {
        let mesh = mesh_of(detached.legacy_context(), detached.legacy_shape(), 0.05).into_result().unwrap();
        (detached.identity().clone(), mesh.points.len(), mesh.triangles.len(), mesh.volume(), mesh.area())
    })
    .join()
    .unwrap();
    assert_eq!(actual_identity, identity);
    assert_eq!((points, triangles, volume, area), (8, 12, 48.0, 88.0));
}

#[test]
fn originating_context_retains_accuracy_clock_stages_cache_and_control() {
    let clock = Arc::new(FakeClock::default());
    let control = Arc::new(CallControl::new());
    let accuracy = Accuracy::for_scale(100.0).unwrap();
    let mut builder = krly3d_std::standard();
    builder.accuracy(accuracy).clock(clock.clone());
    let mut kernel = Kernel::new(builder.context().with_call_control(control.clone()));
    kernel.apply(block(6.0)).unwrap();
    let engine = BrepEngine::new(kernel).unwrap();
    let snapshot = engine.snapshot("part").unwrap();
    let body = engine.kernel().body("part").unwrap();
    let cached = engine.kernel().context().derived(&body, || Attempt::Done(17_u32)).into_result().unwrap();
    let detached_cached =
        snapshot.legacy_context().derived(&body, || Attempt::Done(99_u32)).into_result().unwrap();
    assert!(Arc::ptr_eq(&cached, &detached_cached));
    assert_eq!(snapshot.legacy_context().accuracy(), &accuracy);
    assert_eq!(snapshot.legacy_context().node_stages(), engine.kernel().context().node_stages());
    assert_eq!(snapshot.legacy_context().edit_stages(), engine.kernel().context().edit_stages());
    drop(engine);
    clock.advance(Duration::from_secs(3));
    assert_eq!(snapshot.legacy_context().clock().now(), Duration::from_secs(3));
    assert_eq!(volume(&snapshot), 48.0);
    control.cancel();
    assert_eq!(snapshot.legacy_context().check_control(), Err(Error::from(CallStop::Cancelled)));
}

#[test]
fn invalid_and_missing_nodes_preserve_legacy_diagnostic_labels_and_messages() {
    let engine = BrepEngine::from_k3d(BOX).unwrap();
    for node in
        ["", "part@0", "part@two", "part@4294967296", "part@1@2", "part#top", "2part", "missing", "part@3"]
    {
        let error = engine.kernel().body(node).unwrap_err();
        assert_legacy_diagnostic(engine.snapshot(node).unwrap_err(), error);
    }
    let empty = BrepEngine::new(krly3d_std::standard().build()).unwrap();
    let error = empty.snapshot("part").unwrap_err();
    assert_eq!(error.code.as_str(), "not found");
    assert_eq!(error.message, "there is no node `part`");
}

#[test]
fn invalid_models_preserve_parse_build_and_refused_expectation_errors() {
    for text in [
        "",
        "not a native document",
        "k3d 1\nlayer geo\npart = box w=1\n",
        "k3d 1\nlayer geo\npart = no_such_tool\n",
        "k3d 1\nlayer geo\npart = translate missing by=1,0,0\n",
        "k3d 1\nlayer geo\npart = box w=1 d=2 h=3\n  expect volume=999\n",
    ] {
        let expected = krly3d_core::document::text::parse(text, &[GEOMETRY])
            .and_then(|document| krly3d_std::standard().build().open(document));
        let actual = BrepEngine::from_k3d(text).err().unwrap();
        match expected {
            Err(error) => assert_legacy_diagnostic(actual, error),
            Ok(delta) => {
                assert!(delta.refused());
                assert_eq!(actual.code.as_str(), "input");
                assert_eq!(actual.message, delta.text());
            }
        }
    }
}

#[test]
fn failed_and_refused_edits_and_open_leave_the_previous_snapshot_current() {
    let mut engine = BrepEngine::from_k3d(BOX).unwrap();
    let before = engine.snapshot("part").unwrap();
    assert!(engine.apply(Statement::new("part", "no_such_tool", Params::new(), &[]).unwrap()).is_err());
    assert!(engine.open(document("k3d 1\nlayer geo\npart = no_such_tool\n")).is_err());
    let mut refused = block(8.0);
    refused.expect.push("volume=999".to_string());
    assert!(engine.apply(refused.clone()).unwrap().refused());
    let refused_document = Document::default().with_in(GEOMETRY, refused);
    assert!(engine.open(refused_document).unwrap().refused());
    assert_eq!(engine.snapshot("part").unwrap().identity(), before.identity());
    assert_eq!(volume(&before), 48.0);
}
