#![allow(clippy::unwrap_used)]

use krlyvis_api::{
    encode_result, DrawingCall, DrawingImage, DrawingInput, DrawingQuality, ENGINE_REVISION, PROTOCOL,
};
use krlyvis_engine::{read_drawing, read_shared, DrawingSource, SharedDrawing};
use sha2::{Digest, Sha256};

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
    pnm(40, 28, |x, y| {
        ((3..19).contains(&x) && (4..20).contains(&y) && !((7..11).contains(&x) && (8..12).contains(&y)))
            || ((25..37).contains(&x)
                && (9..23).contains(&y)
                && !((29..33).contains(&x) && (13..19).contains(&y)))
    })
}

fn call(bytes: Vec<u8>, pixel_unit: f64, seed: Option<[f64; 2]>) -> DrawingCall {
    DrawingCall {
        protocol: PROTOCOL.into(),
        request_id: "original-call".into(),
        input: DrawingInput {
            image: DrawingImage { format: "pnm".into(), data: bytes },
            pixel_unit,
            simplify: 0.0,
            seed,
            source: "caller:sheet-scan".into(),
        },
    }
}

fn pixels(drawing: &SharedDrawing) -> Vec<Vec<[f64; 2]>> {
    drawing
        .datum()
        .shape()
        .unwrap()
        .polygons
        .iter()
        .map(|polygon| polygon.iter().map(|pixel| [pixel.x, pixel.y]).collect())
        .collect()
}

#[test]
fn selects_separated_components_with_global_offsets_holes_and_shared_provenance() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DrawingSource>();

    let bytes = fixture();
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let source = DrawingSource::read(call(bytes, 2.5, Some([25.75, 10.25]))).unwrap();
    let provenance = source.provenance().clone();
    assert_eq!(provenance.image_sha256, digest);
    assert_eq!(provenance.source, "caller:sheet-scan");
    assert_eq!(provenance.image_format, "pnm");
    assert_eq!(provenance.image_size, [40, 28]);
    assert_eq!(provenance.pixel_unit, 2.5);
    assert_eq!(provenance.seed, None);
    assert_eq!(provenance.method, "trace-holes");
    assert_eq!(provenance.simplify, 0.0);
    assert_eq!(provenance.engine_revision, ENGINE_REVISION);
    provenance.validate().unwrap();

    let largest = source.select(None).unwrap();
    let first = source.select(Some([4.75, 5.25])).unwrap();
    let second = source.select(Some([25.75, 10.25])).unwrap();
    assert_eq!(pixels(&largest), pixels(&first));
    assert_eq!(
        pixels(&first),
        vec![
            vec![[3.0, 4.0], [19.0, 4.0], [19.0, 20.0], [3.0, 20.0]],
            vec![[7.0, 8.0], [7.0, 12.0], [11.0, 12.0], [11.0, 8.0]],
        ]
    );
    assert_eq!(
        pixels(&second),
        vec![
            vec![[25.0, 9.0], [37.0, 9.0], [37.0, 23.0], [25.0, 23.0]],
            vec![[29.0, 13.0], [29.0, 19.0], [33.0, 19.0], [33.0, 13.0]],
        ]
    );
    assert_eq!(first.material_area(), 1500.0);
    assert_eq!(second.material_area(), 900.0);
    assert_eq!(
        second.polygons(),
        vec![
            vec![[62.5, 22.5], [92.5, 22.5], [92.5, 57.5], [62.5, 57.5]],
            vec![[72.5, 32.5], [72.5, 47.5], [82.5, 47.5], [82.5, 32.5]],
        ]
    );
    for (drawing, seed) in [(&largest, None), (&first, Some([4.75, 5.25])), (&second, Some([25.75, 10.25]))] {
        let mut expected = provenance.clone();
        expected.seed = seed;
        assert_eq!(drawing.provenance(), &expected);
        assert_eq!(drawing.quality(), &DrawingQuality::default());
        assert_eq!(drawing.timing().read_ms, largest.timing().read_ms);
        drawing.timing().validate().unwrap();
    }
    assert_eq!(source.provenance(), &provenance);
    drop(source);
    assert_eq!(first.material_area(), 1500.0);
    assert_eq!(second.material_area(), 900.0);
}

#[test]
fn reusable_native_and_legacy_one_shot_outputs_match_for_each_seed_and_scale() {
    for unit in [1.0, 2.5, 1.0 / 3.0, 1e9, f64::MIN_POSITIVE, f64::from_bits(1)] {
        let source = DrawingSource::read(call(fixture(), unit, None)).unwrap();
        for seed in [None, Some([4.75, 5.25]), Some([25.75, 10.25])] {
            let drawing = source.select(seed).unwrap();
            let input = call(fixture(), unit, seed);
            let shared = read_shared(input.clone()).unwrap();
            assert_eq!(drawing.polygons(), shared.polygons());
            assert_eq!(pixels(&drawing), pixels(&shared));
            assert_eq!(drawing.material_area(), shared.material_area());
            let result = drawing.to_result("original-call").unwrap();
            let legacy = read_drawing(input).unwrap();
            assert_eq!(result.evidence, legacy.evidence);
            assert_eq!(result.provenance, legacy.provenance);
            assert_eq!(result.quality, legacy.quality);
            assert_eq!(result.request_id, legacy.request_id);
            result.timing.validate().unwrap();
            legacy.timing.validate().unwrap();
            encode_result(&result).unwrap();
        }
    }
}

#[test]
fn seed_errors_match_one_shot_and_do_not_poison_the_source_or_existing_drawings() {
    let source = DrawingSource::read(call(fixture(), 1.0, None)).unwrap();
    let before = source.select(None).unwrap();
    let before_pixels = pixels(&before);
    for seed in [
        [f64::NAN, 1.0],
        [1.0, f64::INFINITY],
        [f64::NEG_INFINITY, 1.0],
        [-0.25, 5.0],
        [0.0, 0.0],
        [8.5, 9.5],
        [30.5, 15.5],
        [40.0, 10.0],
        [4.0, 28.0],
        [1e300, 1.0],
    ] {
        let error = source.select(Some(seed)).unwrap_err();
        assert_eq!(error.code.as_str(), "input");
        assert!(error.message.contains("seed"));
        assert_eq!(error, read_shared(call(fixture(), 1.0, Some(seed))).unwrap_err());
        let recovered = source.select(Some([25.75, 10.25])).unwrap();
        assert_eq!(recovered.material_area(), 144.0);
        assert_eq!(recovered.timing().read_ms, before.timing().read_ms);
        assert_eq!(pixels(&before), before_pixels);
        assert_eq!(source.provenance().seed, None);
    }
}

#[test]
fn failed_topology_selection_leaves_other_components_selectable() {
    let bytes = pnm(32, 20, |x, y| {
        ((2..16).contains(&x) && (2..16).contains(&y) && !((x == 6 && y == 6) || (x == 7 && y == 7)))
            || ((22..29).contains(&x) && (5..12).contains(&y))
    });
    let source = DrawingSource::read(call(bytes, 1.0, None)).unwrap();
    let error = source.select(Some([3.5, 3.5])).unwrap_err();
    assert_eq!(error.code.as_str(), "geometry");
    let other = source.select(Some([23.5, 6.5])).unwrap();
    assert_eq!(other.material_area(), 49.0);
    assert_eq!(pixels(&other), vec![vec![[22.0, 5.0], [29.0, 5.0], [29.0, 12.0], [22.0, 12.0]]]);
}

#[test]
fn read_validates_the_original_call_without_selecting_its_seed() {
    let valid = call(fixture(), 1.0, Some([0.0, 0.0]));
    let source = DrawingSource::read(valid.clone()).unwrap();
    assert_eq!(source.select(None).unwrap().material_area(), 240.0);
    assert_eq!(read_shared(valid.clone()).unwrap_err().code.as_str(), "input");

    let mut invalid_calls = Vec::new();
    let mut protocol = valid.clone();
    protocol.protocol = "wrong".into();
    invalid_calls.push(protocol);
    let mut format = valid.clone();
    format.input.image.format = "gif".into();
    invalid_calls.push(format);
    let mut source_claim = valid.clone();
    source_claim.input.source = " \t\n".into();
    invalid_calls.push(source_claim);
    for unit in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e10] {
        invalid_calls.push(call(fixture(), unit, None));
    }
    invalid_calls.push(call(fixture(), 1.0, Some([f64::NAN, 2.0])));
    invalid_calls.push(call(fixture(), 1.0, Some([-1.0, 2.0])));
    for invalid in invalid_calls {
        let expected = invalid.validate().unwrap_err();
        assert_eq!(DrawingSource::read(invalid).err().unwrap(), expected);
    }
    let malformed = call(b"not an image".to_vec(), 1.0, None);
    let expected = read_shared(malformed.clone()).unwrap_err();
    assert_eq!(expected.code.as_str(), "format");
    assert_eq!(DrawingSource::read(malformed).err().unwrap(), expected);
}
