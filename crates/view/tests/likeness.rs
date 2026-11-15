//! Geometric and facade regressions for issue #7's additive station evidence.
#![allow(clippy::unwrap_used, clippy::panic)]
use krly3d_core::{Attempt, Exactness, Id};
use krly3d_kernel::ports::likeness::report::{
    station_likeness, DrawingEvidence, DrawingFrame, StairPolicy, StationOptions, StationPrecision,
    StationReport, StationRequest,
};
use krly3d_kernel::ports::likeness::{Drawing, LikenessRequest};
use krly3d_kernel::ports::peer::{Peer, PeerCall};
use krly3d_kernel::ports::shape::Leaf;
use krly3d_kernel::schema::{Params, Value};
use krly3d_kernel::{Context, KernelBuilder, Plugin, Reply};
use krly3d_std::testing::made;
use std::sync::Arc;

fn builder() -> KernelBuilder {
    let mut builder = krly3d_std::standard();
    krly3d_view::register(&mut builder);
    builder
}

fn rectangle(unit: f64) -> Drawing {
    Drawing { axes: [0, 1], unit, polygons: vec![vec![[0., 0.], [40., 0.], [40., 20.], [0., 20.]]] }
}

fn vector() -> DrawingEvidence {
    DrawingEvidence { vector: true, source: Some("analytic-fixture/1".into()), uncertainty: Some(0.001) }
}

fn block(cx: &Context, shift: [f64; 2]) -> Arc<krly3d_kernel::topo::Body> {
    made(
        cx,
        "box",
        Params::new()
            .with("w", 40.)
            .with("d", 20.)
            .with("h", 3.)
            .with("at", [20. + shift[0], 10. + shift[1], 0.]),
        &[],
    )
}

fn request(cx: &Context, shift: [f64; 2]) -> StationRequest {
    StationRequest {
        comparison: LikenessRequest { shapes: vec![Leaf::of(block(cx, shift))], drawing: rectangle(1.) },
        options: StationOptions { tolerance: Some(1.), ..StationOptions::default() },
        evidence: DrawingEvidence::default(),
    }
}

fn compare(cx: &Context, request: &StationRequest) -> StationReport {
    station_likeness(cx, "raster", request).into_result().unwrap()
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

#[test]
fn rotated_non_axis_aligned_drawing_reports_both_local_axes_and_their_true_worst_stations() {
    let cx = builder().context();
    let theta = 33_f64.to_radians();
    let (s, c) = theta.sin_cos();
    let frame = DrawingFrame { origin: [83., -27.], u: [c, s], v: [-s, c] };
    let world = |p: [f64; 2]| [frame.origin[0] + c * p[0] - s * p[1], frame.origin[1] + s * p[0] + c * p[1]];
    let original = block(&cx, [1., 2.]);
    let original_vertices = original.vertices.clone();
    let turned = made(
        &cx,
        "transform",
        Params::new().with("turn", 33.).with("move", [83., -27., 0.]),
        &[Arc::clone(&original)],
    );
    let before = turned.vertices.clone();
    let mut req = request(&cx, [0., 0.]);
    req.comparison.shapes = vec![Leaf::of(Arc::clone(&turned))];
    req.comparison.drawing.polygons[0] =
        req.comparison.drawing.polygons[0].iter().map(|p| world(*p)).collect();
    req.options.frame = frame;
    req.options.precision = StationPrecision::Contour;
    req.evidence = vector();
    let found = compare(&cx, &req);
    assert_eq!(found.matches, Some(false));
    for along in 0..2 {
        let reported = found
            .worst
            .iter()
            .filter(|s| s.station.along == along)
            .map(|s| s.station.off())
            .max_by(f64::total_cmp)
            .unwrap();
        let actual = found
            .stations
            .iter()
            .filter(|s| s.station.along == along)
            .map(|s| s.station.off())
            .max_by(f64::total_cmp)
            .unwrap();
        close(reported, actual);
        let interior = found
            .stations
            .iter()
            .find(|s| s.station.along == along && s.station.at > 8. && s.station.at < 12.)
            .unwrap();
        close(interior.station.off(), [2., 1.][along]);
        let delta = interior.delta.unwrap();
        close(delta[0], [2., 1.][along]);
        close(delta[1], [2., 1.][along]);
        assert!(found.failed[along] > 0);
    }
    assert_eq!(turned.vertices, before, "reporting edited the candidate");
    assert_eq!(original.vertices, original_vertices);
}

#[test]
fn vector_evidence_reports_fractional_endpoint_differences_without_claiming_mesh_accuracy() {
    let cx = builder().context();
    let mut req = request(&cx, [0.25, 0.125]);
    req.options.precision = StationPrecision::Contour;
    req.options.tolerance = Some(0.3);
    req.evidence = vector();
    let accepted = compare(&cx, &req);
    assert_eq!(accepted.matches, Some(true));
    for station in &accepted.stations {
        close(station.station.off(), [0.125, 0.25][station.station.along]);
        assert_eq!(station.matches, Some(true));
    }
    req.options.tolerance = Some(0.2);
    let refused = compare(&cx, &req);
    assert_eq!(refused.failed[0], 0);
    assert!(refused.failed[1] > 0);
    assert_eq!(refused.matches, Some(false));
}

#[test]
fn diagonal_stairs_are_an_explicit_allowance_and_never_rewrite_the_reference() {
    let cx = builder().context();
    // A 45-degree square and a pixel staircase around its four diagonal edges.
    let square = made(
        &cx,
        "box",
        Params::new().with("w", 10. * 2_f64.sqrt()).with("d", 10. * 2_f64.sqrt()).with("h", 2.),
        &[],
    );
    let diamond =
        made(&cx, "transform", Params::new().with("turn", 45.).with("move", [10., 10., 0.]), &[square]);
    // Trace the staircase from successive horizontal strips of an expanded diamond.
    let mut left = Vec::new();
    let mut right = Vec::new();
    for row in 0..20 {
        let half = 10. - (row as f64 + 0.5 - 10.).abs();
        let lo = (10. - half).floor() - 2.;
        let hi = (10. + half).ceil() + 2.;
        left.extend([[lo, row as f64], [lo, row as f64 + 1.]]);
        right.extend([[hi, row as f64], [hi, row as f64 + 1.]]);
    }
    right.reverse();
    left.extend(right);
    let mut req = request(&cx, [0., 0.]);
    req.comparison.shapes = vec![Leaf::of(diamond)];
    req.comparison.drawing.polygons = vec![left];
    let drawing = req.comparison.drawing.clone();
    let strict = compare(&cx, &req);
    req.options.stairs = StairPolicy::PixelDiagonal;
    let tolerant = compare(&cx, &req);
    assert_eq!(strict.resemblance, tolerant.resemblance);
    assert_eq!(strict.stations.len(), tolerant.stations.len());
    let mut newly_accepted = 0;
    for (a, b) in strict.stations.iter().zip(&tolerant.stations) {
        assert_eq!(a.station, b.station);
        assert_eq!(a.delta, b.delta);
        close(b.tolerance.unwrap(), 1. + 2_f64.sqrt());
        if a.matches == Some(false) && b.matches == Some(true) {
            newly_accepted += 1;
        }
    }
    assert!(newly_accepted > 0, "no diagonal station exercised the allowance");
    assert_eq!(req.comparison.drawing, drawing);
}

#[test]
fn thresholds_and_evidence_are_validated_instead_of_inventing_precision() {
    let cx = builder().context();
    let req = request(&cx, [0., 0.]);
    for tolerance in [-1., f64::NAN, f64::INFINITY, 0.5] {
        let mut invalid = req.clone();
        invalid.options.tolerance = Some(tolerance);
        let error = station_likeness(&cx, "raster", &invalid).into_result().unwrap_err();
        assert!(error.message.contains("tolerance"), "{error}");
    }
    let mut invalid = req.clone();
    invalid.options.precision = StationPrecision::Contour;
    let error = station_likeness(&cx, "raster", &invalid).into_result().unwrap_err();
    assert!(error.message.contains("explicit evidence source"), "{error}");
    invalid.evidence = vector();
    invalid.options.tolerance = Some(0.0001);
    assert!(station_likeness(&cx, "raster", &invalid)
        .into_result()
        .unwrap_err()
        .message
        .contains("uncertainty"));
    invalid.options.tolerance = Some(1.);
    invalid.options.stairs = StairPolicy::PixelDiagonal;
    assert!(station_likeness(&cx, "raster", &invalid)
        .into_result()
        .unwrap_err()
        .message
        .contains("only to raster"));
    let mut invalid = req;
    invalid.options.frame.u = [2., 0.];
    assert!(station_likeness(&cx, "raster", &invalid)
        .into_result()
        .unwrap_err()
        .message
        .contains("orthonormal"));
}

#[test]
fn a_missing_side_fails_even_with_a_large_threshold_and_omitting_threshold_has_no_verdict() {
    let cx = builder().context();
    let mut req = request(&cx, [50., 0.]);
    req.options.tolerance = Some(1e6);
    let found = compare(&cx, &req);
    assert_eq!(found.matches, Some(false));
    assert!(found.stations.iter().any(|s| s.delta.is_none() && s.matches == Some(false)));
    req.options.tolerance = None;
    let unjudged = compare(&cx, &req);
    assert_eq!(unjudged.matches, None);
    assert!(unjudged.stations.iter().all(|s| s.matches.is_none() && s.tolerance.is_none()));
    assert!(unjudged.worst.iter().any(|s| s.station.along == 0));
    assert!(unjudged.worst.iter().any(|s| s.station.along == 1));
}

#[derive(Clone)]
struct Drawer {
    data: Value,
}
impl Plugin<Peer> for Drawer {
    fn id(&self) -> Id {
        Id::new("vis")
    }
    fn call(&self, _: &Context, _: &PeerCall) -> Attempt<Reply> {
        Attempt::Done(Reply {
            ok: true,
            text: String::new(),
            data: self.data.clone(),
            exact: Exactness::Sampled { samples: 800 },
        })
    }
}
fn drawing_value(drawing: &Drawing) -> Value {
    Value::map([
        ("axes", Value::List(drawing.axes.map(|a| Value::from(["x", "y", "z"][a])).to_vec())),
        ("unit", drawing.unit.into()),
        (
            "polygons",
            Value::List(
                drawing
                    .polygons
                    .iter()
                    .map(|p| {
                        Value::List(p.iter().map(|p| Value::List(p.map(Value::from).to_vec())).collect())
                    })
                    .collect(),
            ),
        ),
    ])
}
fn entry<'a>(value: &'a Value, key: &str) -> &'a Value {
    let Value::Map(map) = value else { panic!("not a map: {value:?}") };
    &map[key]
}

#[test]
fn legacy_facade_is_unchanged_and_opt_in_evidence_pins_nodes_and_retains_reference() {
    let mut b = builder();
    let reference = drawing_value(&rectangle(1.));
    b.plugin::<Peer>(Drawer { data: reference.clone() });
    let mut session = b.session();
    let params = Params::new()
        .with("id", "part")
        .with("w", 40.)
        .with("d", 20.)
        .with("h", 3.)
        .with("at", [20., 10., 0.]);
    assert!(session.call("box", &params).ok);
    let query = Params::new().with("against", "vis:top");
    let legacy = session.call("likeness", &query);
    assert!(legacy.ok, "{}", legacy.text);
    assert_eq!(legacy.text, "like vis:top: IoU 1; area model 800, drawing 800\n");
    assert_eq!(
        legacy.data,
        Value::map([
            ("iou", 1.0.into()),
            ("model_area", 800.0.into()),
            ("drawing_area", 800.0.into()),
            ("worst", Value::List(vec![]))
        ])
    );
    let detailed = session.call("likeness", &query.clone().with("report", "stations").with("tolerance", 1.));
    assert!(detailed.ok, "{}", detailed.text);
    assert_eq!(entry(&detailed.data, "report_version"), &Value::from("krly3d.likeness.stations/1"));
    assert_eq!(entry(&detailed.data, "matches"), &Value::Bool(true));
    let provenance = entry(&detailed.data, "provenance");
    assert_eq!(entry(provenance, "nodes"), &Value::List(vec!["part@1".into()]));
    assert_eq!(entry(provenance, "reference"), &reference);
    assert_eq!(entry(provenance, "mesh_achieved_error"), &Value::from("unknown"));
    let Value::List(worst) = entry(&detailed.data, "worst") else { panic!() };
    for axis in ["u", "v"] {
        assert!(worst.iter().any(|s| entry(s, "along") == &Value::from(axis)));
    }
    // Failed report options do not edit the document or alter legacy responses.
    for bad in [
        query.clone().with("tolerance", 1.),
        query.clone().with("report", "stations").with("tolerance", 0.1),
        query.clone().with("report", "typo"),
        query.clone().with("report", "stations").with("precision", "contour"),
    ] {
        let reply = session.call("likeness", &bad);
        assert!(!reply.ok, "{}", reply.text);
    }
    assert_eq!(session.call("likeness", &query), legacy);
}

#[test]
fn swapped_world_axes_and_reflected_local_axes_remain_valid() {
    let cx = builder().context();
    let mut req = request(&cx, [0., 0.]);
    req.comparison.drawing.axes = [1, 0];
    for p in &mut req.comparison.drawing.polygons[0] {
        p.swap(0, 1);
    }
    req.options.frame = DrawingFrame { origin: [0., 0.], u: [-1., 0.], v: [0., 1.] };
    let found = compare(&cx, &req);
    assert_eq!(found.matches, Some(true));
    close(found.resemblance.iou, 1.);
}

#[test]
fn mesh_failures_retain_face_and_threshold_instead_of_becoming_empty_silhouettes() {
    use krly3d_core::Error;
    use krly3d_kernel::mesh::Mesh;
    use krly3d_kernel::ports::interpret::{MeshRequest, ToMesh};
    use krly3d_kernel::ports::likeness::report::StationLikeness;
    struct Refuse(bool);
    impl Plugin<ToMesh> for Refuse {
        fn id(&self) -> Id {
            Id::new("brep")
        }
        fn call(&self, _: &Context, request: &MeshRequest) -> Attempt<Arc<Mesh>> {
            let reason = format!("face `top`: mesh budget at tolerance {}", request.tolerance);
            if self.0 {
                Attempt::Failed(Error::geometry(reason))
            } else {
                Attempt::Declined(reason)
            }
        }
    }
    let real = builder().context();
    let req = request(&real, [0., 0.]);
    for failed in [false, true] {
        let mut b = KernelBuilder::new();
        b.plugin::<ToMesh>(Refuse(failed));
        b.plugin::<StationLikeness>(krly3d_view::likeness::Raster);
        let error = station_likeness(&b.context(), "raster", &req).into_result().unwrap_err();
        assert!(error.message.contains("face `top`: mesh budget at tolerance 1"), "{error}");
    }
    assert_eq!(compare(&real, &req).matches, Some(true));
}

#[test]
fn invalid_typed_drawings_and_unresolved_raster_contours_fail_explicitly() {
    let cx = builder().context();
    let req = request(&cx, [0., 0.]);
    for axes in [[0, 0], [0, 3], [usize::MAX, 1]] {
        let mut bad = req.clone();
        bad.comparison.drawing.axes = axes;
        assert!(station_likeness(&cx, "raster", &bad)
            .into_result()
            .unwrap_err()
            .message
            .contains("distinct world axes"));
    }
    for unit in [0., -1., f64::NAN, f64::INFINITY] {
        let mut bad = req.clone();
        bad.comparison.drawing.unit = unit;
        assert!(station_likeness(&cx, "raster", &bad)
            .into_result()
            .unwrap_err()
            .message
            .contains("positive unit"));
    }
    let mut bad = req.clone();
    bad.comparison.drawing.polygons[0][0][0] = f64::NAN;
    assert!(station_likeness(&cx, "raster", &bad)
        .into_result()
        .unwrap_err()
        .message
        .contains("finite points"));
    let mut bad = req;
    bad.comparison.shapes.clear();
    bad.comparison.drawing.polygons = vec![vec![[0., 0.], [0.1, 0.], [0.1, 0.1], [0., 0.1]]];
    assert!(station_likeness(&cx, "raster", &bad)
        .into_result()
        .unwrap_err()
        .message
        .contains("no drawing extent"));
}
