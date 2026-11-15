#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::{Accuracy, Exactness, Id};
use krly3d_kernel::model::Statement;
use krly3d_kernel::ports::query::{ask, Answer};
use krly3d_kernel::schema::{Params, Value};
use krly3d_kernel::Kernel;
use krly3d_mesh::codec::encode_result;
use krly3d_mesh::{descriptor, LengthUnit, MeshCall, MeshInput, MeshResult, RequestId};
use krly3d_mesh_engine::MeshEngine;
use std::f64::consts::PI;

fn block(width: f64) -> Statement {
    Statement::new("part", "box", Params::new().with("w", width).with("d", 4.0).with("h", 2.0), &[]).unwrap()
}

fn kernel() -> Kernel {
    let mut builder = krly3d_std::standard();
    builder.accuracy(Accuracy::for_scale(100.0).unwrap());
    let mut kernel = builder.build();
    kernel.apply(block(6.0)).unwrap();
    kernel
}

fn engine() -> MeshEngine {
    MeshEngine::new(kernel()).unwrap()
}

fn call(node: &str) -> MeshCall {
    MeshCall::new(RequestId::new("r1"), MeshInput::new(node))
}

fn legacy(engine: &MeshEngine, params: &Params) -> Result<Answer, krly3d_core::Error> {
    let (document, model) = engine.kernel().snapshot();
    ask(engine.kernel().context(), &Id::new("mesh"), params, &document, &model).into_result()
}

fn compare(engine: &MeshEngine, call: MeshCall) -> MeshResult {
    let params = Params::new()
        .with("node", call.input.node.as_str())
        .with("tolerance", call.input.tolerance.value)
        .with("triangles", call.input.triangles);
    let legacy = legacy(engine, &params).unwrap();
    let result = engine.call(call).unwrap();
    let Value::Map(data) = legacy.data else { panic!("mesh data is not a map") };
    for (name, number) in [
        ("points", result.points as f64),
        ("triangles", result.triangles as f64),
        ("volume", result.volume),
        ("area", result.area),
        ("open", result.open as f64),
    ] {
        assert_eq!(data[name], Value::Number(number), "{name}");
    }
    assert_eq!(legacy.exact, result.quality.exactness);
    assert!(result.quality.positional_uncertainty.is_none());
    assert!(result.quality.confidence.is_none());
    if let Some(mesh) = &result.mesh {
        assert_eq!(mesh.revision, 1);
        assert_eq!(mesh.data.points.len() as u64, result.points);
        assert_eq!(mesh.data.triangles.len() as u64, result.triangles);
        let coordinates = mesh.data.points.iter().flatten().copied().map(Value::from).collect();
        let corners = mesh.data.triangles.iter().flatten().map(|i| Value::from(f64::from(*i))).collect();
        assert_eq!(data["coordinates"], Value::List(coordinates));
        assert_eq!(data["corners"], Value::List(corners));
        assert!(mesh.data.triangles.iter().flatten().all(|i| u64::from(*i) < result.points));
    } else {
        assert!(!data.contains_key("coordinates") && !data.contains_key("corners"));
    }
    assert!(encode_result(&result).is_ok());
    result
}

#[test]
fn default_and_explicit_inputs_match_legacy_and_the_independent_box_oracle() {
    let engine = engine();
    let default = compare(&engine, call("part"));
    assert_eq!((default.points, default.triangles, default.open), (8, 12, 0));
    assert_eq!((default.volume, default.area), (48.0, 88.0));
    assert_eq!(default.quality.exactness, Exactness::Sampled { samples: 12 });
    assert!(default.mesh.is_none());
    let explicit = compare(&engine, call("part@1"));
    assert_eq!(default, explicit);
    let implicit = legacy(&engine, &Params::new().with("node", "part")).unwrap();
    let explicit =
        legacy(&engine, &Params::new().with("node", "part").with("tolerance", 0.05).with("triangles", false))
            .unwrap();
    assert_eq!(implicit, explicit);
    assert_eq!(implicit.text, "12 triangles, 8 points, volume 48, area 88, 0 open sides");
}

#[test]
fn optional_artifacts_match_the_legacy_flattened_encoding() {
    let engine = engine();
    let mut request = call("part");
    request.input.triangles = true;
    let first = compare(&engine, request.clone());
    let second = compare(&engine, request);
    let one = first.mesh.as_ref().unwrap();
    let two = second.mesh.as_ref().unwrap();
    assert_ne!(one.id, two.id);
    assert_eq!(one.data, two.data);
    assert_eq!(first.provenance, second.provenance);
}

#[test]
fn sphere_matches_legacy_and_independent_volume_error_bound() {
    let mut engine = engine();
    engine.apply(Statement::new("ball", "sphere", Params::new().with("r", 5.0), &[]).unwrap()).unwrap();
    let mut request = call("ball");
    request.input.tolerance.value = 0.01;
    request.input.triangles = true;
    let result = compare(&engine, request);
    assert_eq!(result.open, 0);
    assert!((result.volume - 4.0 / 3.0 * PI * 125.0).abs() <= 4.0 * PI * 25.0 * 0.01);
}

#[test]
fn prepared_snapshot_keeps_its_version_and_build_key_through_edits_and_rollback() {
    let mut engine = engine();
    let mark = engine.checkpoint();
    let first = engine.call(call("part")).unwrap();
    let prepared_first = engine.prepare(call("part")).unwrap();
    engine.apply(block(8.0)).unwrap();
    let prepared_second = engine.prepare(call("part")).unwrap();
    let second = engine.call(call("part")).unwrap();
    assert_eq!(second.volume, 64.0);
    assert_eq!(second.provenance.input.node, "part@2");
    assert_ne!(first.provenance.source, second.provenance.source);
    assert_eq!(engine.execute(prepared_first).unwrap(), first);
    engine.rollback(mark.clone());
    assert_eq!(engine.call(call("part")).unwrap(), first);
    engine.apply(block(10.0)).unwrap();
    let branch = engine.call(call("part")).unwrap();
    assert_eq!(branch.volume, 80.0);
    assert_eq!(branch.provenance.input.node, "part@2");
    assert_ne!(branch.provenance.source, second.provenance.source);
    assert_eq!(engine.execute(prepared_second).unwrap(), second);
    let resolved =
        engine.kernel().model().built(&krly3d_kernel::model::NodeRef::parse("part").unwrap()).unwrap();
    assert!(branch.provenance.source.as_str().ends_with(&format!(":part@2:{:032x}", resolved.key)));
    engine.rollback(mark);
    assert_eq!(engine.call(call("part@1")).unwrap().provenance, first.provenance);
}

#[test]
fn session_identity_is_engine_owned_and_prepared_work_cannot_cross_sessions() {
    let first = engine();
    let second = engine();
    assert_ne!(
        first.call(call("part")).unwrap().provenance.source,
        second.call(call("part")).unwrap().provenance.source
    );
    let prepared = first.prepare(call("part")).unwrap();
    let Err(error) = second.execute(prepared) else { panic!("cross-session work accepted") };
    assert_eq!(error.code.as_str(), "input");
    let result = first.call(call("part")).unwrap();
    let metadata = descriptor();
    assert_eq!(result.provenance.package_version, metadata.package_version);
    assert_eq!(result.provenance.engine_revision, metadata.engine_revision);
}

#[test]
fn errors_match_legacy_for_missing_versioned_nodes_and_tolerance() {
    let engine = engine();
    for node in ["missing", "part@3", "part@0", "part@4294967296", "part#face"] {
        let old = legacy(&engine, &Params::new().with("node", node)).unwrap_err();
        let new = engine.call(call(node)).unwrap_err();
        assert_eq!(new.code.as_str(), old.kind.label(), "{node}");
        assert_eq!(new.message, old.message, "{node}");
    }
    let minimum = engine.kernel().context().accuracy().length();
    for tolerance in [0.0, -0.01, minimum, minimum / 2.0] {
        let old =
            legacy(&engine, &Params::new().with("node", "part").with("tolerance", tolerance)).unwrap_err();
        let mut request = call("part");
        request.input.tolerance.value = tolerance;
        let new = engine.call(request).unwrap_err();
        assert_eq!(new.code.as_str(), old.kind.label());
        assert_eq!(new.message, old.message);
    }
}

#[test]
fn unsupported_features_and_nonfinite_values_are_rejected_without_changing_the_session() {
    let engine = engine();
    let before = engine.call(call("part")).unwrap();
    for timeout in [0, 1] {
        let mut request = call("part");
        request.timeout_ms = Some(timeout);
        assert_eq!(engine.call(request).unwrap_err().code.as_str(), "unsupported");
    }
    let mut request = call("part");
    request.protocol.minor = 1;
    assert_eq!(engine.call(request).unwrap_err().code.as_str(), "unsupported_version");
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut request = call("part");
        request.input.tolerance.value = value;
        assert_eq!(engine.call(request).unwrap_err().code.as_str(), "input");
    }
    let mut request = call("part");
    request.input.tolerance.unit = LengthUnit::Metre;
    assert_eq!(engine.call(request).unwrap_err().code.as_str(), "input");
    assert_eq!(engine.call(call("part")).unwrap(), before);
}

#[test]
fn exhausted_mesh_declines_keep_the_legacy_unsupported_diagnostic() {
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::interpret::{MeshRequest, ToMesh};
    use krly3d_kernel::{Context, KernelBuilder, Plugin};
    use std::sync::Arc;

    struct Decline;
    impl Plugin<ToMesh> for Decline {
        fn id(&self) -> Id {
            Id::new("brep")
        }

        fn call(&self, _: &Context, _: &MeshRequest) -> Attempt<Arc<krly3d_kernel::mesh::Mesh>> {
            Attempt::declined("fixture cannot tessellate this shape")
        }
    }

    let mut builder = KernelBuilder::new();
    krly3d_std::sweep::register(&mut builder);
    krly3d_std::tool::register(&mut builder);
    krly3d_std::query::register(&mut builder);
    krly3d_std::stage::register(&mut builder);
    builder.plugin::<ToMesh>(Decline);
    let mut kernel = builder.build();
    kernel.apply(block(6.0)).unwrap();
    let engine = MeshEngine::new(kernel).unwrap();
    let old = legacy(&engine, &Params::new().with("node", "part")).unwrap_err();
    let new = engine.call(call("part")).unwrap_err();
    assert_eq!(old.kind, krly3d_core::ErrorKind::Unsupported);
    assert_eq!(new.code.as_str(), old.kind.label());
    assert_eq!(new.message, old.message);
    assert!(new.message.contains("fixture cannot tessellate this shape"));
}

#[test]
fn controlled_execution_preserves_sync_results_and_snapshot_identity() {
    use krlyxd::CallControl;
    use std::sync::Arc;

    let mut engine = engine();
    let expected = engine.call(call("part")).unwrap();
    let prepared = engine.prepare(call("part")).unwrap();
    engine.apply(block(8.0)).unwrap();
    let result = engine.execute_controlled(prepared, Arc::new(CallControl::new())).unwrap();
    assert_eq!(result, expected);
    assert_eq!(engine.call(call("part")).unwrap().volume, 64.0);
}

#[test]
fn cancelled_calls_reject_even_cached_results_and_do_not_poison_the_session() {
    use krlyxd::CallControl;
    use std::sync::Arc;

    let engine = engine();
    let control = Arc::new(CallControl::new());
    control.cancel();
    for cached in [false, true] {
        if cached {
            assert_eq!(engine.call(call("part")).unwrap().volume, 48.0);
        }
        let prepared = engine.prepare(call("part")).unwrap();
        assert_eq!(
            engine.execute_controlled(prepared, Arc::clone(&control)).unwrap_err().code.as_str(),
            "cancelled"
        );
    }
    let result = engine
        .execute_controlled(engine.prepare(call("part")).unwrap(), Arc::new(CallControl::new()))
        .unwrap();
    assert_eq!(result.volume, 48.0);
}

#[test]
fn controlled_deadlines_use_the_injected_clock_and_remain_distinct_from_cancel() {
    use krlyxd::{CallControl, FakeClock};
    use std::sync::Arc;
    use std::time::Duration;

    let engine = engine();
    let clock = Arc::new(FakeClock::default());
    let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_millis(5)));
    let prepared = engine.prepare(call("part")).unwrap();
    clock.advance(Duration::from_millis(5));
    assert_eq!(
        engine.execute_controlled(prepared, Arc::clone(&control)).unwrap_err().code.as_str(),
        "deadline_exceeded"
    );
    control.cancel();
    let prepared = engine.prepare(call("part")).unwrap();
    assert_eq!(engine.execute_controlled(prepared, control).unwrap_err().code.as_str(), "cancelled");
    assert_eq!(engine.call(call("part")).unwrap().volume, 48.0);
}

#[test]
fn cancellation_after_tessellation_prevents_late_success_caching() {
    use krly3d_core::Attempt;
    use krly3d_kernel::ports::interpret::{MeshRequest, ToMesh};
    use krly3d_kernel::{Context, KernelBuilder, Plugin};
    use krlyxd::CallControl;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CancelAfterMesh {
        control: Arc<CallControl>,
        calls: Arc<AtomicUsize>,
    }
    impl Plugin<ToMesh> for CancelAfterMesh {
        fn id(&self) -> Id {
            Id::new("brep")
        }
        fn call(&self, cx: &Context, request: &MeshRequest) -> Attempt<Arc<krly3d_kernel::mesh::Mesh>> {
            let result = krly3d_std::interpret::mesh::BrepMesh.call(cx, request);
            if self.calls.fetch_add(1, Ordering::Relaxed) == 0 {
                self.control.cancel();
            }
            result
        }
    }
    let control = Arc::new(CallControl::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = KernelBuilder::new();
    builder.plugin::<ToMesh>(CancelAfterMesh { control: control.clone(), calls: calls.clone() });
    krly3d_std::sweep::register(&mut builder);
    krly3d_std::tool::register(&mut builder);
    krly3d_std::stage::register(&mut builder);
    let mut kernel = builder.build();
    kernel.apply(block(6.0)).unwrap();
    let engine = MeshEngine::new(kernel).unwrap();
    let prepared = engine.prepare(call("part")).unwrap();
    assert_eq!(engine.execute_controlled(prepared, control).unwrap_err().code.as_str(), "cancelled");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let retry = engine.call(call("part")).unwrap();
    assert_eq!((retry.volume, retry.area), (48.0, 88.0));
    assert_eq!(calls.load(Ordering::Relaxed), 2, "the cancelled result must not be cached");
    assert_eq!(engine.call(call("part")).unwrap(), retry);
    assert_eq!(calls.load(Ordering::Relaxed), 2, "the successful retry should be cached");
}

#[test]
fn composing_an_existing_brep_owner_keeps_source_identity_and_geometry() {
    let brep = krly3d_brep_engine::BrepEngine::new(kernel()).unwrap();
    let snapshot = brep.snapshot("part").unwrap();
    let identity = snapshot.identity().clone();
    let mut engine = MeshEngine::from_brep(brep);
    assert!(engine.brep().owns(&snapshot));
    let old = engine.prepare(call("part")).unwrap();
    engine.apply(block(8.0)).unwrap();
    let result = engine.execute(old).unwrap();
    assert_eq!(result.provenance.source, identity.source);
    assert_eq!(result.provenance.input.node, identity.node);
    assert_eq!(result.volume, 48.0);
    let current = engine.call(call("part")).unwrap();
    assert_eq!(current.volume, 64.0);
    assert_ne!(current.provenance.source, identity.source);
}
