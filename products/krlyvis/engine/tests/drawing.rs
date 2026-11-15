#![allow(clippy::unwrap_used)]

use krly3d_core::schema::{Params, Value as CoreValue};
use krly3d_core::Id;
use krlyvis_api::{encode_result, DrawingCall, DrawingImage, DrawingInput, DrawingResult, PROTOCOL};
use krlyvis_engine::{read_drawing, read_shared, SharedDrawing};
use krlyvis_kernel::ports::make::{make, MakeRequest};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn pnm(width: usize, height: usize, dark: impl Fn(usize, usize) -> bool) -> Vec<u8> {
    let mut bytes = format!("P5\n{width} {height}\n255\n").into_bytes();
    for y in 0..height {
        for x in 0..width {
            bytes.push(if dark(x, y) { 40 } else { 230 });
        }
    }
    bytes
}

fn fixture() -> Vec<u8> {
    pnm(24, 20, |x, y| {
        ((3..19).contains(&x) && (4..16).contains(&y) && !((7..11).contains(&x) && (8..12).contains(&y)))
            || ((21..23).contains(&x) && (2..4).contains(&y))
    })
}

fn call(bytes: Vec<u8>, pixel_unit: f64, seed: Option<[f64; 2]>) -> DrawingCall {
    DrawingCall {
        protocol: PROTOCOL.into(),
        request_id: "drawing-engine".into(),
        input: DrawingInput {
            image: DrawingImage { format: "pnm".into(), data: bytes },
            pixel_unit,
            simplify: 0.0,
            seed,
            source: "caller:unverified-not-a-hash".into(),
        },
    }
}

#[test]
fn holes_nonzero_origin_scaling_and_replayable_pixel_coordinates_survive_encoding() {
    let result = read_drawing(call(fixture(), 2.5, None)).unwrap();
    assert_eq!(result.evidence["node"], "top@1");
    assert_eq!(result.evidence["kind"], "placed");
    assert_eq!(result.evidence["axes"], json!(["x", "y"]));
    assert_eq!(result.evidence["looking"], json!([0, 0, -1]));
    assert_eq!(result.evidence["unit"], 2.5);
    assert_eq!(result.evidence["material_area"], 1100);
    assert_eq!(
        result.evidence["pixels"],
        json!([[[3, 4], [19, 4], [19, 16], [3, 16]], [[7, 8], [7, 12], [11, 12], [11, 8]],])
    );
    assert_eq!(
        result.evidence["polygons"],
        json!([
            [[7.5, 10], [47.5, 10], [47.5, 40], [7.5, 40]],
            [[17.5, 20], [17.5, 30], [27.5, 30], [27.5, 20]],
        ])
    );
    assert_eq!(result.evidence["evidence"]["kind"], "raster");
    assert_eq!(result.evidence["evidence"]["uncertainty"], "unknown");
    let wire: Value = serde_json::from_str(&encode_result(&result).unwrap()).unwrap();
    assert_eq!(wire["request_id"], "drawing-engine");
    assert!(wire["quality"]["confidence"].is_null());
    assert!(wire["quality"]["positional_uncertainty"].is_null());
    assert!(wire.get("model").is_none());
    assert!(wire.get("session").is_none());
    assert!(result.timing.total_ms >= result.timing.read_ms + result.timing.outline_ms);
}

#[test]
fn shared_native_geometry_metadata_and_adapter_match_the_wire_path() {
    for (scale, seed) in [
        (1.0, None),
        (2.5, None),
        (1.0 / 3.0, None),
        (1e9, Some([4.25, 5.75])),
        (0.125, Some([21.5, 2.5])),
        (f64::MIN_POSITIVE, None),
        (f64::from_bits(1), None),
    ] {
        let input = call(fixture(), scale, seed);
        let shared = read_shared(input.clone()).unwrap();
        assert!(matches!(shared.datum().as_ref(), krlyvis_kernel::Datum::Placed { .. }));
        assert_eq!(shared.datum().measure("area"), Some(shared.material_area()));

        let result = shared.to_result("runtime:adapter").unwrap();
        let polygons: Vec<Vec<[f64; 2]>> =
            serde_json::from_value(result.evidence["polygons"].clone()).unwrap();
        assert_eq!(shared.polygons(), polygons);
        assert_eq!(shared.region().polygons().unwrap(), polygons);
        assert_eq!(shared.region().length_unit(), krlyvis_api::metadata().length_unit);
        assert_eq!(result.evidence["material_area"].as_f64(), Some(shared.material_area()));
        assert_eq!(&result.provenance, shared.provenance());
        assert_eq!(&result.quality, shared.quality());
        assert_eq!(&result.timing, shared.timing());
        assert_eq!(result.request_id, "runtime:adapter");
        encode_result(&result).unwrap();

        let delegated = read_drawing(input).unwrap();
        assert_eq!(result.evidence, delegated.evidence);
        assert_eq!(result.provenance, delegated.provenance);
        assert_eq!(result.quality, delegated.quality);
        assert_eq!(delegated.request_id, "drawing-engine");
        assert!(shared.timing().total_ms >= shared.timing().read_ms + shared.timing().outline_ms);
    }
}

#[test]
fn shared_clones_retain_arc_identity_and_owned_adapters_cannot_change_the_snapshot() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SharedDrawing>();

    let shared = read_shared(call(fixture(), 2.5, None)).unwrap();
    let cloned = shared.clone();
    assert!(Arc::ptr_eq(shared.datum(), cloned.datum()));
    assert!(Arc::ptr_eq(shared.region(), cloned.region()));
    assert!(Arc::ptr_eq(shared.datum().shape().unwrap(), cloned.datum().shape().unwrap()));
    let polygons = shared.polygons();
    let mut changed_polygons = cloned.polygons();
    changed_polygons[0][0] = [-1.0, -1.0];
    assert_eq!(shared.polygons(), polygons);

    let mut changed_native = Arc::clone(cloned.datum());
    if let krlyvis_kernel::Datum::Placed { shape, frame } = Arc::make_mut(&mut changed_native) {
        frame.scale = [10.0, 10.0];
        Arc::make_mut(shape).polygons[0][0] = krlyvis_kernel::Pixel::new(-1.0, -1.0);
    }
    assert!(!Arc::ptr_eq(shared.datum(), &changed_native));
    assert_eq!(shared.polygons(), polygons);
    assert_eq!(shared.material_area(), 1100.0);

    let first = shared.to_result("first").unwrap();
    let mut changed_result = cloned.to_result("changed").unwrap();
    changed_result.evidence["polygons"][0][0][0] = json!(-1);
    changed_result.provenance.source = "another claim".into();
    changed_result.quality.confidence = Some(1.0);
    changed_result.timing.total_ms = -1.0;
    let second = cloned.to_result("").unwrap();
    assert_eq!(second.request_id, "");
    assert_eq!(first.evidence, second.evidence);
    assert_eq!(first.provenance, second.provenance);
    assert_eq!(first.quality, second.quality);
    assert_eq!(first.timing, second.timing);
    assert!(Arc::ptr_eq(shared.datum(), cloned.datum()));
    drop(shared);
    assert_eq!(cloned.polygons(), polygons);
    assert_eq!(cloned.material_area(), 1100.0);
}

#[test]
fn shared_and_wire_paths_reject_the_same_invalid_native_calls() {
    let mut wrong_version = call(fixture(), 1.0, None);
    wrong_version.protocol = "krlyvis.drawing/2".into();
    let mut wrong_format = call(fixture(), 1.0, None);
    wrong_format.input.image.format = "gif".into();
    let mut blank_source = call(fixture(), 1.0, None);
    blank_source.input.source = " \t\n".into();
    for input in [
        wrong_version,
        wrong_format,
        blank_source,
        call(vec![], 1.0, None),
        call(fixture(), f64::NAN, None),
        call(fixture(), 0.0, None),
        call(fixture(), 1.0, Some([f64::INFINITY, 1.0])),
        call(fixture(), 1.0, Some([8.0, 9.0])),
    ] {
        let shared = read_shared(input.clone()).unwrap_err();
        let wire = read_drawing(input).unwrap_err();
        assert_eq!(shared, wire);
    }
}

#[test]
fn default_largest_fractional_seed_and_background_hole_and_outside_seed_rejection() {
    let largest = read_drawing(call(fixture(), 1.0, None)).unwrap();
    assert_eq!(largest.evidence["material_area"], 176);
    let seeded = read_drawing(call(fixture(), 1.0, Some([21.75, 2.25]))).unwrap();
    assert_eq!(seeded.evidence["material_area"], 4);
    assert_eq!(seeded.evidence["pixels"], json!([[[21, 2], [23, 2], [23, 4], [21, 4]]]));
    for seed in [[0.0, 0.0], [8.0, 9.0], [24.0, 2.0], [2.0, 20.0], [1e300, 1.0]] {
        let error = read_drawing(call(fixture(), 1.0, Some(seed))).unwrap_err();
        assert_eq!(error.code.as_str(), "input");
        assert!(error.message.contains("seed"));
    }
}

#[test]
fn hashes_actual_supplied_image_bytes_and_keeps_source_claim_separate() {
    let bytes = fixture();
    let result = read_drawing(call(bytes.clone(), 1.0, None)).unwrap();
    assert_eq!(result.provenance.image_sha256, format!("{:x}", Sha256::digest(&bytes)));
    assert_eq!(result.provenance.source, "caller:unverified-not-a-hash");
    assert_eq!(result.provenance.image_size, [24, 20]);
    assert_eq!(result.provenance.method, "trace-holes");
    assert_eq!(result.provenance.simplify, 0.0);
    // Same image samples with a different legal header have different byte hashes.
    let mut alternate = b"P5\n# byte provenance differs\n24 20\n255\n".to_vec();
    alternate.extend_from_slice(&bytes[bytes.len() - 24 * 20..]);
    let other = read_drawing(call(alternate, 1.0, None)).unwrap();
    assert_eq!(result.evidence, other.evidence);
    assert_ne!(result.provenance.image_sha256, other.provenance.image_sha256);
    let again = read_drawing(call(bytes, 1.0, None)).unwrap();
    assert_eq!(result.evidence, again.evidence);
    assert_eq!(result.provenance, again.provenance);
}

#[test]
fn rejects_vertex_explosion_without_simplifying_and_accepts_exactly_256_vertices() {
    for (steps, succeeds) in [(63, true), (64, false)] {
        let bytes = pnm(2 * steps + 6, 10, |x, y| {
            (2..2 * steps + 4).contains(&x) && (4..7).contains(&y)
                || ((3..2 * steps + 3).contains(&x) && x % 2 == 1 && y == 3)
        });
        let result = read_drawing(call(bytes, 1.0, None));
        if succeeds {
            let result = result.unwrap();
            assert_eq!(result.evidence["polygons"][0].as_array().unwrap().len(), 256);
        } else {
            assert_eq!(result.unwrap_err().code.as_str(), "resource_limit");
        }
    }
}

#[test]
fn positive_pixel_tolerance_preserves_holes_and_provenance_without_raising_vertex_limit() {
    let bytes = pnm(134, 24, |x, y| {
        (((2..132).contains(&x) && (6..20).contains(&y)) || ((3..131).contains(&x) && x % 2 == 1 && y == 5))
            && !((40..54).contains(&x) && (10..16).contains(&y))
    });
    assert_eq!(read_drawing(call(bytes.clone(), 0.25, None)).unwrap_err().code.as_str(), "resource_limit");
    let mut input = call(bytes.clone(), 0.25, None);
    input.input.simplify = 1.0;
    let result = read_drawing(input.clone()).unwrap();
    assert_eq!(result.provenance.simplify, 1.0);
    assert_eq!(result.provenance.image_sha256, format!("{:x}", Sha256::digest(&bytes)));
    assert_eq!(result.evidence["polygons"].as_array().unwrap().len(), 2);
    let vertices: usize =
        result.evidence["polygons"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().len()).sum();
    assert!(vertices <= krlyvis_api::MAX_VERTICES);
    assert_eq!(result.quality.confidence, None);
    assert_eq!(result.quality.positional_uncertainty, None);
    encode_result(&result).unwrap();
    input.input.pixel_unit = 2.5;
    let scaled = read_drawing(input).unwrap();
    assert_eq!(
        result.evidence["pixels"], scaled.evidence["pixels"],
        "tolerance is in pixels, not model units"
    );
    for tolerance in [f64::NAN, f64::INFINITY, -1.0, krlyvis_api::MAX_SIMPLIFY_PIXELS + 1.0] {
        let mut invalid = result.clone();
        invalid.provenance.simplify = tolerance;
        assert!(encode_result(&invalid).is_err());
    }
}

#[test]
fn typed_boundary_invalid_images_uniform_images_and_ambiguous_topology_fail() {
    for bytes in [vec![], b"not an image".to_vec(), pnm(10, 10, |_, _| false)] {
        assert!(read_drawing(call(bytes, 1.0, None)).is_err());
    }
    for value in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1.0e10] {
        assert!(read_drawing(call(fixture(), value, None)).is_err());
    }
    assert!(read_drawing(call(fixture(), 1.0, Some([f64::NAN, 1.0]))).is_err());
    let bytes = pnm(10, 10, |x, y| {
        (2..8).contains(&x) && (2..8).contains(&y) && !((x == 4 && y == 4) || (x == 5 && y == 5))
    });
    assert_eq!(read_drawing(call(bytes, 1.0, None)).unwrap_err().code.as_str(), "geometry");
}

#[test]
fn image_provider_resource_limits_are_preserved() {
    let oversized = b"P5 268435457 1 255\n".to_vec();
    let error = read_drawing(call(oversized, 1.0, None)).unwrap_err();
    assert_eq!(error.code.as_str(), "format");
    assert!(error.message.contains("not a size"));
    for format in ["png", "jpeg"] {
        let mut input = call(fixture(), 1.0, None);
        input.input.image.format = format.into();
        assert_eq!(read_drawing(input).unwrap_err().code.as_str(), "format");
    }
}

struct ImageFile(PathBuf);

impl ImageFile {
    fn new(bytes: &[u8]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "krlyvis-drawing-parity-{}-{}.pnm",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
}

impl Drop for ImageFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn assert_core_value(actual: &Value, expected: &CoreValue) {
    match expected {
        CoreValue::Bool(value) => assert_eq!(actual.as_bool(), Some(*value)),
        CoreValue::Number(value) => assert_eq!(actual.as_f64(), Some(*value)),
        CoreValue::Text(value) => assert_eq!(actual.as_str(), Some(value.as_str())),
        CoreValue::List(items) => {
            let actual = actual.as_array().unwrap();
            assert_eq!(actual.len(), items.len());
            for (actual, expected) in actual.iter().zip(items) {
                assert_core_value(actual, expected);
            }
        }
        CoreValue::Map(items) => {
            assert_eq!(actual.as_object().unwrap().len(), items.len());
            for (key, expected) in items {
                assert_core_value(&actual[key], expected);
            }
        }
    }
}

#[test]
fn exactly_matches_existing_picture_outline_frame_and_polygons_provider_path() {
    for (scale, seed) in [(1.0, None), (2.5, None), (1e9, Some([4.25, 5.75])), (0.125, Some([21.5, 2.5]))] {
        let bytes = fixture();
        let image = ImageFile::new(&bytes);
        let cx = krlyvis_std::standard().context();
        let picture = make(
            &cx,
            &Id::new("picture"),
            &MakeRequest {
                params: Params::new().with("path", image.0.to_string_lossy().to_string()),
                inputs: vec![],
            },
        )
        .into_result()
        .unwrap();
        let seed_values = seed.map_or_else(Vec::new, |[x, y]| vec![x.into(), y.into()]);
        let outline = make(
            &cx,
            &Id::new("outline"),
            &MakeRequest {
                params: Params::new()
                    .with("region", CoreValue::List(vec![]))
                    .with("seed", CoreValue::List(seed_values))
                    .with("threshold", -1.0)
                    .with("simplify", 0.0)
                    .with("method", "trace-holes"),
                inputs: vec![picture],
            },
        )
        .into_result()
        .unwrap();
        let placed = make(
            &cx,
            &Id::new("frame"),
            &MakeRequest {
                params: Params::new().with("view", "top").with("axes", "").with(
                    "points",
                    CoreValue::List(
                        [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, scale, scale]
                            .into_iter()
                            .map(CoreValue::from)
                            .collect(),
                    ),
                ),
                inputs: vec![outline],
            },
        )
        .into_result()
        .unwrap();
        let mut expected = placed.value();
        if let CoreValue::Map(fields) = &mut expected {
            fields.insert("node".into(), "top@1".into());
        }
        let actual = read_drawing(call(bytes, scale, seed)).unwrap();
        assert_core_value(&actual.evidence, &expected);
        assert_eq!(actual.evidence["material_area"].as_f64(), placed.measure("area"));
    }
}

#[test]
fn result_encoding_rejects_missing_nonfinite_and_forged_metadata() {
    let result = read_drawing(call(fixture(), 1.0, None)).unwrap();
    type Mutation = Box<dyn Fn(&mut DrawingResult)>;
    let cases: Vec<Mutation> = vec![
        Box::new(|r| r.protocol = "wrong".into()),
        Box::new(|r| r.evidence["polygons"][0][0][0] = Value::Null),
        Box::new(|r| r.evidence["unit"] = Value::Null),
        Box::new(|r| r.evidence["material_area"] = json!(-1)),
        Box::new(|r| r.evidence["node"] = json!("global@42")),
        Box::new(|r| r.evidence["extra"] = json!(true)),
        Box::new(|r| r.evidence["evidence"]["threshold"] = Value::Null),
        Box::new(|r| r.provenance.pixel_unit = f64::NAN),
        Box::new(|r| r.provenance.simplify = f64::INFINITY),
        Box::new(|r| r.provenance.seed = Some([f64::NAN, 1.0])),
        Box::new(|r| r.provenance.image_sha256 = "caller-claim".into()),
        Box::new(|r| r.provenance.image_sha256 = "A".repeat(64)),
        Box::new(|r| r.quality.confidence = Some(f64::NAN)),
        Box::new(|r| r.quality.positional_uncertainty = Some(f64::INFINITY)),
        Box::new(|r| r.timing.read_ms = f64::NAN),
        Box::new(|r| r.timing.outline_ms = f64::INFINITY),
        Box::new(|r| r.timing.total_ms = -1.0),
    ];
    for mutate in cases {
        let mut invalid = result.clone();
        mutate(&mut invalid);
        assert!(encode_result(&invalid).is_err());
    }
}
