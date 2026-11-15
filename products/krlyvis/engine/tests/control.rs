//! Cooperative stops are native and never become state on a reusable image owner.
#![allow(clippy::unwrap_used)]

use krly3d_core::{CallControl, Clock, FakeClock};
use krlyvis_api::{DrawingCall, DrawingImage, DrawingInput, PROTOCOL};
use krlyvis_engine::{read_drawing, read_shared, read_shared_controlled, DrawingSource};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn call(seed: Option<[f64; 2]>) -> DrawingCall {
    let mut bytes = b"P5\n80 64\n255\n".to_vec();
    for y in 0..64 {
        for x in 0..80 {
            let material = ((3..45).contains(&x)
                && (4..56).contains(&y)
                && !((12..24).contains(&x) && (16..32).contains(&y)))
                || ((60..74).contains(&x) && (8..28).contains(&y));
            bytes.push(if material { 40 } else { 230 });
        }
    }
    DrawingCall {
        protocol: PROTOCOL.into(),
        request_id: "controlled".into(),
        input: DrawingInput {
            image: DrawingImage { format: "pnm".into(), data: bytes },
            pixel_unit: 2.5,
            simplify: 0.0,
            seed,
            source: "caller:unverified-scan".into(),
        },
    }
}

#[test]
fn pre_cancelled_and_expired_calls_fail_before_validation_or_selection() {
    let source = DrawingSource::read(call(None)).unwrap();
    let cancelled = Arc::new(CallControl::new());
    cancelled.cancel();
    let expired = Arc::new(CallControl::with_deadline(Arc::new(FakeClock::default()), Duration::ZERO));
    for (control, code) in [(cancelled, "cancelled"), (expired, "deadline_exceeded")] {
        let mut malformed = call(None);
        malformed.protocol = "invalid".into();
        malformed.input.image.data.clear();
        assert_eq!(
            DrawingSource::read_controlled(malformed.clone(), Arc::clone(&control))
                .err()
                .unwrap()
                .code
                .as_str(),
            code
        );
        assert_eq!(read_shared_controlled(malformed, Arc::clone(&control)).unwrap_err().code.as_str(), code);
        assert_eq!(source.select_controlled(Some([f64::NAN, 0.0]), control).unwrap_err().code.as_str(), code);
        assert_eq!(source.select(None).unwrap().material_area(), 12450.0);
    }
}

#[test]
fn controls_are_not_retained_and_legacy_calls_remain_independent() {
    let control = Arc::new(CallControl::new());
    let weak = Arc::downgrade(&control);
    let source = DrawingSource::read_controlled(call(None), Arc::clone(&control)).unwrap();
    let first = source.select_controlled(None, Arc::clone(&control)).unwrap();
    let snapshot = read_shared_controlled(call(None), Arc::clone(&control)).unwrap();
    assert_eq!(Arc::strong_count(&control), 1);
    control.cancel();
    let second = source.select(Some([61.5, 9.5])).unwrap();
    assert_eq!(second.material_area(), 1750.0);
    assert_eq!(source.select(None).unwrap().polygons(), first.polygons());
    assert_eq!(read_shared(call(None)).unwrap().polygons(), first.polygons());
    assert_eq!(first.polygons(), snapshot.polygons());
    drop(control);
    assert!(weak.upgrade().is_none());
    drop(source);
    assert_eq!(first.material_area(), 12450.0);
    snapshot.to_result("still-usable").unwrap();
}

#[test]
fn controlled_outputs_keep_legacy_geometry_hash_provenance_and_quality() {
    for seed in [None, Some([4.5, 5.5]), Some([61.5, 9.5])] {
        let controlled = read_shared_controlled(call(seed), Arc::new(CallControl::new())).unwrap();
        let legacy = read_drawing(call(seed)).unwrap();
        let result = controlled.to_result("controlled").unwrap();
        assert_eq!(result.evidence, legacy.evidence);
        assert_eq!(result.provenance, legacy.provenance);
        assert_eq!(result.quality, legacy.quality);
        assert_eq!(result.protocol, legacy.protocol);
        assert_eq!(result.request_id, legacy.request_id);
        result.timing.validate().unwrap();
    }
}

struct HashDeadline(AtomicUsize);

impl Clock for HashDeadline {
    fn now(&self) -> Duration {
        // Admission and validation pass, then the second hash chunk expires.
        if self.0.fetch_add(1, Ordering::Relaxed) >= 3 {
            Duration::from_secs(1)
        } else {
            Duration::ZERO
        }
    }
}

#[test]
fn hashing_stops_between_chunks_before_attempting_decode() {
    let mut input = call(None);
    // Valid native input, deliberately malformed codec bytes, spanning hash chunks.
    input.input.image.data = vec![b'x'; 3 * 64 * 1024];
    let clock = Arc::new(HashDeadline(AtomicUsize::new(0)));
    let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
    let error = DrawingSource::read_controlled(input, control).err().unwrap();
    assert_eq!(error.code.as_str(), "deadline_exceeded");
    assert_eq!(clock.0.load(Ordering::Relaxed), 4);
}
