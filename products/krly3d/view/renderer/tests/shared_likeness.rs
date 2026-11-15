//! Native snapshot likeness agrees with the existing station provider, without
//! registering view/peer services on the shared B-rep owner.
#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_brep_engine::{BrepEngine, Snapshot};
use krly3d_core::{CallControl, Clock, Exactness, FakeClock};
use krly3d_kernel::model::Statement;
use krly3d_kernel::ports::likeness::report::{
    station_likeness, DrawingEvidence, DrawingFrame, StairPolicy, StationLikeness, StationOptions,
    StationPrecision, StationReport, StationRequest,
};
use krly3d_kernel::ports::likeness::{Drawing, LikenessRequest};
use krly3d_kernel::ports::peer::Peer;
use krly3d_kernel::schema::Params;
use krly3d_view_api::likeness::{
    encode_result, ComparisonScope, LikenessCall, LikenessResult, RasterEvidence, StationDifference,
    ENGINE_REVISION, MAX_ABS_COORDINATE, MAX_EVIDENCE_VERTICES, MAX_NODE_BYTES, MAX_PIXEL_UNIT,
    MAX_REQUEST_ID_BYTES, MAX_SOURCE_BYTES, REPORT_VERSION,
};
use krly3d_view_api::{ApiVersion, Length, LengthUnit, RequestId};
use krly3d_view_renderer::likeness::compare_snapshot;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const BOX: &str = "k3d 1\nlayer geo\npart = box w=40 d=20 h=3 at=20,10,0\n";
const TRANSLATED: &str = "k3d 1\nlayer geo\npart = box w=40 d=20 h=3 at=27,-1,0\n";
const HOLED: &str = "k3d 1\nlayer geo\noutline = profile plane=xy path=\"M 0,0 L 40,0 L 40,20 L 0,20 Z M 10,5 L 30,5 L 30,15 L 10,15 Z\"\npart = extrude outline h=3\n";

fn rectangle(shift: [f64; 2]) -> Vec<[f64; 2]> {
    [[0., 0.], [40., 0.], [40., 20.], [0., 20.]].map(|[x, y]| [x + shift[0], y + shift[1]]).to_vec()
}

fn evidence() -> RasterEvidence {
    RasterEvidence {
        polygons: vec![rectangle([0., 0.])],
        pixel_unit: 1.,
        source: "sha256:caller-supplied-image-digest".into(),
    }
}

fn call(evidence: RasterEvidence) -> LikenessCall {
    LikenessCall::new(RequestId::new("native-likeness"), "part", evidence)
}

fn compare(snapshot: Snapshot, evidence: RasterEvidence) -> LikenessResult {
    compare_snapshot(snapshot, call(evidence), Arc::new(CallControl::new())).unwrap()
}

fn registered_engine(text: &str) -> BrepEngine {
    let mut builder = krly3d_std::standard();
    krly3d_view::register(&mut builder);
    let mut engine = BrepEngine::new(builder.build()).unwrap();
    let document = krly3d_core::document::text::parse(text, &[krly3d_kernel::model::GEOMETRY]).unwrap();
    let delta = engine.open(document).unwrap();
    assert!(!delta.refused(), "{}", delta.text());
    engine
}

fn existing(snapshot: &Snapshot, evidence: &RasterEvidence) -> StationReport {
    station_likeness(
        snapshot.legacy_context(),
        "raster",
        &StationRequest {
            comparison: LikenessRequest {
                shapes: vec![Arc::clone(snapshot.legacy_shape())],
                drawing: Drawing {
                    axes: [0, 1],
                    unit: evidence.pixel_unit,
                    polygons: evidence.polygons.clone(),
                },
            },
            options: StationOptions {
                frame: DrawingFrame::default(),
                precision: StationPrecision::Contour,
                tolerance: Some(evidence.pixel_unit),
                stairs: StairPolicy::None,
            },
            evidence: DrawingEvidence {
                vector: false,
                source: Some(evidence.source.clone()),
                uncertainty: None,
            },
        },
    )
    .into_result()
    .unwrap()
}

fn assert_difference(
    native: &StationDifference,
    old: &krly3d_kernel::ports::likeness::report::StationDifference,
) {
    assert_eq!(native.id, format!("{}:{}", ["u", "v"][old.station.along], old.index));
    assert_eq!(native.index, old.index);
    assert_eq!(native.station.along, old.station.along);
    assert_eq!(native.station.at, old.station.at);
    assert_eq!(native.station.model, old.station.model);
    assert_eq!(native.station.drawing, old.station.drawing);
    assert_eq!(native.station.off, old.station.off());
    assert_eq!(native.delta, old.delta);
    assert_eq!(native.tolerance, old.tolerance);
    assert_eq!(native.matches, old.matches);
}

fn assert_report(native: &LikenessResult, old: &StationReport) {
    assert_eq!(native.iou, old.resemblance.iou);
    assert_eq!(native.model_area, old.resemblance.model_area);
    assert_eq!(native.drawing_area, old.resemblance.drawing_area);
    assert_eq!(native.raster_worst.len(), old.resemblance.worst.len());
    for (native, old) in native.raster_worst.iter().zip(&old.resemblance.worst) {
        assert_eq!(native.along, old.along);
        assert_eq!(native.at, old.at);
        assert_eq!(native.model, old.model);
        assert_eq!(native.drawing, old.drawing);
        assert_eq!(native.off, old.off());
    }
    assert_eq!(native.stations.len(), old.stations.len());
    assert_eq!(native.worst.len(), old.worst.len());
    for (native, old) in native.stations.iter().zip(&old.stations) {
        assert_difference(native, old);
    }
    for (native, old) in native.worst.iter().zip(&old.worst) {
        assert_difference(native, old);
    }
    assert_grid_and_policy(native, old);
}

fn assert_grid_and_policy(native: &LikenessResult, old: &StationReport) {
    assert_eq!(native.compared, old.compared);
    assert_eq!(native.failed, old.failed);
    assert_eq!(native.matches, old.matches);
    assert_eq!(native.grid.low, old.grid_low);
    assert_eq!(native.grid.cell, old.cell);
    assert_eq!(native.grid.size, old.size);
    assert_eq!(native.grid.samples, old.samples);
    assert_eq!(native.frame.origin, old.options.frame.origin);
    assert_eq!(native.frame.u, old.options.frame.u);
    assert_eq!(native.frame.v, old.options.frame.v);
    assert_eq!(native.frame.world_axes, [0, 1]);
    assert_eq!(native.policy.tolerance, old.options.tolerance.unwrap());
    assert_eq!(native.policy.raster_precision_floor, old.cell);
    assert_eq!(native.policy.stair_allowance, 0.0);
    assert_eq!(native.policy.scope, ComparisonScope::SampledOuterExtents);
}

#[test]
fn native_matches_existing_station_provider_for_matching_translated_and_holed_shapes() {
    for text in [BOX, TRANSLATED, HOLED] {
        let native = BrepEngine::from_k3d(text).unwrap();
        let legacy = registered_engine(text);
        let snapshot = native.snapshot("part").unwrap();
        assert!(snapshot.legacy_context().registry::<StationLikeness>().is_none());
        assert!(snapshot.legacy_context().registry::<Peer>().is_none());
        let mut drawing = evidence();
        if text == TRANSLATED {
            drawing.polygons[0] = rectangle([7., -11.]);
        }
        if text == HOLED {
            drawing.polygons.push(vec![[10., 5.], [30., 5.], [30., 15.], [10., 15.]]);
        }
        for shift in [[0., 0.], [0.25, 0.125], [3., -2.], [60., 0.]] {
            let mut shifted = drawing.clone();
            for point in shifted.polygons.iter_mut().flatten() {
                point[0] += shift[0];
                point[1] += shift[1];
            }
            let old = existing(&legacy.snapshot("part").unwrap(), &shifted);
            let result = compare(snapshot.clone(), shifted.clone());
            assert_report(&result, &old);
            assert_eq!(result.matches, Some(shift[0].abs() <= 1. && shift[1].abs() <= 1.));
            assert_eq!(result.provenance.source, *snapshot.identity());
            assert_eq!(result.provenance.node, "part@1");
            assert_eq!(result.provenance.evidence, shifted);
            assert_eq!(result.report_version, REPORT_VERSION);
            assert_eq!(result.request_id, RequestId::new("native-likeness"));
            assert_eq!(result.provenance.engine_revision.as_str(), ENGINE_REVISION);
            assert_eq!(result.provenance.mesh_achieved_error, None);
            assert_eq!(result.provenance.drawing_uncertainty, None);
            assert_eq!(result.quality.exactness, Exactness::Sampled { samples: old.samples });
            assert_eq!(result.quality.positional_uncertainty, None);
            assert_eq!(result.quality.confidence, None);
            assert_eq!(
                result.quality.requested_tolerance,
                Some(Length { value: 1., unit: LengthUnit::Model })
            );
            assert!(encode_result(&result).is_ok());
        }
    }
}

#[test]
fn hole_changes_raster_iou_without_promoting_outer_extent_matches_to_hole_agreement() {
    let engine = BrepEngine::from_k3d(BOX).unwrap();
    let mut drawing = evidence();
    drawing.polygons.push(vec![[10., 5.], [30., 5.], [30., 15.], [10., 15.]]);
    let result = compare(engine.snapshot("part").unwrap(), drawing);
    assert_eq!(result.matches, Some(true));
    assert_eq!(result.failed, [0, 0]);
    assert_eq!(result.iou, 0.75);
    assert_eq!(result.model_area, 800.);
    assert_eq!(result.drawing_area, 600.);
    assert_eq!(result.policy.scope, ComparisonScope::SampledOuterExtents);
    let encoded = encode_result(&result).unwrap();
    assert!(encoded.contains("holes and continuous agreement not tested"));
    assert!(encoded.contains("\"mesh_achieved_error\":null"));
    assert!(encoded.contains("\"drawing_uncertainty\":null"));
    assert!(encoded.contains("not authenticated"));
}

fn block(width: f64) -> Statement {
    Statement::new(
        "part",
        "box",
        Params::new().with("w", width).with("d", 20.).with("h", 3.).with("at", [20., 10., 0.]),
        &[],
    )
    .unwrap()
}

#[test]
fn retained_source_identity_survives_edits_rollback_branching_open_and_owner_drop() {
    let mut engine = BrepEngine::from_k3d(BOX).unwrap();
    let first = engine.snapshot("part").unwrap();
    let before = compare(first.clone(), evidence());
    let checkpoint = engine.checkpoint();
    assert!(!engine.apply(block(48.)).unwrap().refused());
    let second = engine.snapshot("part").unwrap();
    let changed = compare(second.clone(), evidence());
    assert_ne!(before.provenance.source.source, changed.provenance.source.source);
    assert_ne!(before.iou, changed.iou);
    assert_eq!(changed.provenance.node, "part@2");
    engine.rollback(checkpoint);
    assert!(!engine.apply(block(24.)).unwrap().refused());
    let branch = compare(engine.snapshot("part").unwrap(), evidence());
    assert_eq!(branch.provenance.node, changed.provenance.node);
    assert_ne!(branch.provenance.source.source, changed.provenance.source.source);
    let other = krly3d_core::document::text::parse(
        "k3d 1\nlayer geo\nother = sphere r=2\n",
        &[krly3d_kernel::model::GEOMETRY],
    )
    .unwrap();
    assert!(!engine.open(other).unwrap().refused());
    drop(engine);
    assert_eq!(compare(first, evidence()), before);
    assert_eq!(compare(second, evidence()), changed);

    let other = BrepEngine::from_k3d(BOX).unwrap();
    let separate = compare(other.snapshot("part").unwrap(), evidence());
    assert_eq!(before.provenance.source.build_key, separate.provenance.source.build_key);
    assert_ne!(before.provenance.source.session, separate.provenance.source.session);
    assert_ne!(before.provenance.source.source, separate.provenance.source.source);
    let mut other_claim = evidence();
    other_claim.source = "sha256:a-different-image-claim".into();
    let relabelled = compare(other.snapshot("part").unwrap(), other_claim.clone());
    assert_eq!(relabelled.iou, separate.iou);
    assert_eq!(relabelled.provenance.source, separate.provenance.source);
    assert_eq!(relabelled.provenance.evidence, other_claim);
}

#[test]
fn requested_node_and_version_must_match_the_pinned_snapshot() {
    let mut engine = BrepEngine::from_k3d(BOX).unwrap();
    let pinned = engine.snapshot("part").unwrap();
    engine.apply(block(48.)).unwrap();
    for node in ["part", "part@1", "part@0001"] {
        let call = LikenessCall::new(RequestId::new("pinned"), node, evidence());
        let result = compare_snapshot(pinned.clone(), call, Arc::new(CallControl::new())).unwrap();
        assert_eq!(result.provenance.node, "part@1");
        assert_eq!(result.matches, Some(true));
    }
    for node in ["other", "other@1", "part@2", "part@0", "part@two", "part#top", ""] {
        let call = LikenessCall::new(RequestId::new("mismatch"), node, evidence());
        let error = compare_snapshot(pinned.clone(), call, Arc::new(CallControl::new())).unwrap_err();
        assert_eq!(error.code.as_str(), "input", "{node}: {error:?}");
    }
}

#[test]
fn invalid_typed_evidence_and_api_are_rejected_before_station_comparison() {
    let engine = BrepEngine::from_k3d(BOX).unwrap();
    let snapshot = engine.snapshot("part").unwrap();
    let mut invalid = Vec::new();
    for pixel_unit in [0., -1., f64::NAN, f64::INFINITY, MAX_PIXEL_UNIT + 1.] {
        invalid.push(RasterEvidence { pixel_unit, ..evidence() });
    }
    for source in ["", " \t\n"] {
        invalid.push(RasterEvidence { source: source.into(), ..evidence() });
    }
    invalid.push(RasterEvidence { source: "s".repeat(MAX_SOURCE_BYTES + 1), ..evidence() });
    for polygons in [
        vec![],
        vec![vec![]],
        vec![vec![[0., 0.], [1., 1.]]],
        vec![vec![[0., 0.], [1., 0.], [f64::NAN, 1.]]],
        vec![vec![[0., 0.], [1., f64::INFINITY], [1., 1.]]],
        vec![vec![[0., 0.], [MAX_ABS_COORDINATE + 1., 0.], [1., 1.]]],
        vec![vec![[0., 0.], [-MAX_ABS_COORDINATE - 1., 0.], [1., 1.]]],
        vec![vec![[0., 0.]; MAX_EVIDENCE_VERTICES + 1]],
        vec![vec![[0., 0.]; MAX_EVIDENCE_VERTICES / 2 + 1]; 2],
        vec![vec![[0., 0.]; 3]; MAX_EVIDENCE_VERTICES / 3 + 1],
    ] {
        invalid.push(RasterEvidence { polygons, ..evidence() });
    }
    for evidence in invalid {
        let request = call(evidence);
        let expected = request.validate().unwrap_err();
        assert_eq!(
            compare_snapshot(snapshot.clone(), request, Arc::new(CallControl::new())).unwrap_err(),
            expected
        );
    }
    let mut unsupported = call(evidence());
    unsupported.api = ApiVersion { major: 1, minor: 1 };
    assert_eq!(
        compare_snapshot(snapshot.clone(), unsupported, Arc::new(CallControl::new()))
            .unwrap_err()
            .code
            .as_str(),
        "unsupported_version"
    );
    let empty_id = LikenessCall::new(RequestId::new(""), "part", evidence());
    assert_eq!(
        compare_snapshot(snapshot.clone(), empty_id, Arc::new(CallControl::new())).unwrap_err().code.as_str(),
        "input"
    );
    for request in [
        LikenessCall::new(RequestId::new(&"r".repeat(MAX_REQUEST_ID_BYTES + 1)), "part", evidence()),
        LikenessCall::new(RequestId::new("long-node"), "n".repeat(MAX_NODE_BYTES + 1), evidence()),
    ] {
        let expected = request.validate().unwrap_err();
        assert_eq!(
            compare_snapshot(snapshot.clone(), request, Arc::new(CallControl::new())).unwrap_err(),
            expected
        );
    }
    assert_eq!(compare(snapshot, evidence()).matches, Some(true));
}

#[test]
fn evidence_bounds_apply_to_total_vertices_and_accept_the_documented_boundaries() {
    let mut drawing = evidence();
    drawing.polygons = vec![vec![[0., 0.]; MAX_EVIDENCE_VERTICES]];
    drawing.validate().unwrap();
    drawing.polygons.push(vec![[0., 0.]; 3]);
    assert_eq!(drawing.validate().unwrap_err().code.as_str(), "resource_limit");
    drawing.polygons = vec![vec![[0., 0.]; MAX_EVIDENCE_VERTICES / 2]; 2];
    drawing.validate().unwrap();
    drawing.polygons[1].push([0., 0.]);
    assert_eq!(drawing.validate().unwrap_err().code.as_str(), "resource_limit");
    drawing.polygons = vec![vec![[-MAX_ABS_COORDINATE, 0.], [MAX_ABS_COORDINATE, 0.], [0., 1.]]];
    drawing.pixel_unit = MAX_PIXEL_UNIT;
    drawing.source = "s".repeat(MAX_SOURCE_BYTES);
    drawing.validate().unwrap();
}

#[test]
fn coarsened_grid_keeps_the_existing_raster_floor_instead_of_faking_a_one_pixel_verdict() {
    let engine = BrepEngine::from_k3d(BOX).unwrap();
    let legacy = registered_engine(BOX);
    let drawing = RasterEvidence {
        polygons: vec![vec![[0., 0.], [10_000_000., 0.], [10_000_000., 1.], [0., 1.]]],
        ..evidence()
    };
    let request = StationRequest {
        comparison: LikenessRequest {
            shapes: vec![Arc::clone(legacy.snapshot("part").unwrap().legacy_shape())],
            drawing: Drawing { axes: [0, 1], unit: 1., polygons: drawing.polygons.clone() },
        },
        options: StationOptions {
            precision: StationPrecision::Contour,
            tolerance: Some(1.),
            ..StationOptions::default()
        },
        evidence: DrawingEvidence { vector: false, source: Some(drawing.source.clone()), uncertainty: None },
    };
    let expected = station_likeness(legacy.kernel().context(), "raster", &request).into_result().unwrap_err();
    let error =
        compare_snapshot(engine.snapshot("part").unwrap(), call(drawing), Arc::new(CallControl::new()))
            .unwrap_err();
    assert_eq!(error.code.as_str(), expected.kind.label());
    assert_eq!(error.message, expected.message);
    assert!(error.message.contains("effective raster cell"));
}

#[test]
fn finite_but_empty_contours_and_unrepresentable_resolution_keep_provider_diagnostics() {
    let engine = BrepEngine::from_k3d(BOX).unwrap();
    let snapshot = engine.snapshot("part").unwrap();
    for drawing in [
        RasterEvidence { polygons: vec![vec![[0., 0.], [1., 0.], [2., 0.]]], ..evidence() },
        RasterEvidence { pixel_unit: f64::MIN_POSITIVE, ..evidence() },
    ] {
        // These inputs are structurally valid; the existing provider determines
        // whether a drawing extent or finite grid can actually be established.
        drawing.validate().unwrap();
        let error =
            compare_snapshot(snapshot.clone(), call(drawing), Arc::new(CallControl::new())).unwrap_err();
        assert!(matches!(error.code.as_str(), "geometry" | "input"));
    }
}

struct CheckClock {
    checks: AtomicUsize,
    expire_at: usize,
}

impl Clock for CheckClock {
    fn now(&self) -> Duration {
        if self.checks.fetch_add(1, Ordering::SeqCst) + 1 >= self.expire_at {
            Duration::from_secs(1)
        } else {
            Duration::ZERO
        }
    }
}

#[test]
fn cancellation_and_deadlines_return_no_result_and_allow_clean_retry() {
    let engine = BrepEngine::from_k3d(BOX).unwrap();
    let snapshot = engine.snapshot("part").unwrap();
    let cancelled = Arc::new(CallControl::new());
    cancelled.cancel();
    let error = compare_snapshot(snapshot.clone(), call(evidence()), cancelled).unwrap_err();
    assert_eq!(error.code.as_str(), "cancelled");
    let expired = Arc::new(CallControl::with_deadline(Arc::new(FakeClock::default()), Duration::ZERO));
    let error = compare_snapshot(snapshot.clone(), call(evidence()), expired).unwrap_err();
    assert_eq!(error.code.as_str(), "deadline_exceeded");

    // Warm the shared mesh cache so the counted calls follow the same path. An
    // injected clock then expires during stations and at final guarded publication.
    let before = compare(snapshot.clone(), evidence());
    let counter = Arc::new(CheckClock { checks: AtomicUsize::new(0), expire_at: usize::MAX });
    let control = Arc::new(CallControl::with_deadline(counter.clone(), Duration::from_secs(1)));
    let result = compare_snapshot(snapshot.clone(), call(evidence()), control).unwrap();
    assert_eq!(result, before);
    let checks = counter.checks.load(Ordering::SeqCst);
    assert!(checks > before.stations.len());
    for expire_at in [1, checks / 2, checks - 1, checks] {
        let clock = Arc::new(CheckClock { checks: AtomicUsize::new(0), expire_at });
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
        let error = compare_snapshot(snapshot.clone(), call(evidence()), control).unwrap_err();
        assert_eq!(error.code.as_str(), "deadline_exceeded");
        assert_eq!(clock.checks.load(Ordering::SeqCst), expire_at);
        assert_eq!(compare(snapshot.clone(), evidence()), before);
    }
}

#[test]
fn cli_encoder_rejects_nonfinite_metadata_instead_of_silently_encoding_null() {
    let engine = BrepEngine::from_k3d(BOX).unwrap();
    let result = compare(engine.snapshot("part").unwrap(), evidence());
    let mut invalid = result.clone();
    invalid.stations[0].delta = Some([f64::NAN, 0.]);
    assert_eq!(encode_result(&invalid).unwrap_err().code.as_str(), "output");
    let mut invalid = result.clone();
    invalid.worst[0].station.along = 2;
    assert_eq!(encode_result(&invalid).unwrap_err().code.as_str(), "output");
    let mut invalid = result;
    invalid.grid.cell = f64::INFINITY;
    assert_eq!(encode_result(&invalid).unwrap_err().code.as_str(), "output");
}
