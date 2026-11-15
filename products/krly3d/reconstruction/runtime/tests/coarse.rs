#![allow(clippy::unwrap_used)]

use krly3d_reconstruction::{Change, ConstructionFrame, EventKind, Host, Settings, MAX_IN_FLIGHT};
use krlyvis_api::{DrawingCall, DrawingImage, DrawingInput, PROTOCOL};
use std::sync::Arc;

fn drawing() -> DrawingCall {
    let mut data = b"P5 16 16 255\n".to_vec();
    for y in 0..16 {
        for x in 0..16 {
            let outer = (2..14).contains(&x) && (2..14).contains(&y);
            let hole = (6..10).contains(&x) && (6..10).contains(&y);
            data.push(if outer && !hole { 0 } else { 255 });
        }
    }
    DrawingCall {
        protocol: PROTOCOL.into(),
        request_id: "coarse-fixture".into(),
        input: DrawingInput {
            image: DrawingImage { format: "pnm".into(), data },
            pixel_unit: 0.5,
            simplify: 0.0,
            seed: None,
            source: "ring".into(),
        },
    }
}

fn settings(height: f64) -> Settings {
    Settings { height, tolerance: 0.05 }
}

fn mesh_bounds(mesh: &krly3d_mesh::MeshResult) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for point in &mesh.mesh.as_ref().unwrap().data.points {
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    (min, max)
}

fn assert_evidence_bounds(
    evidence: &krlyvis_engine::SharedDrawing,
    frame: ConstructionFrame,
    min: [f64; 3],
    max: [f64; 3],
) {
    let mut evidence_min = [f64::INFINITY; 2];
    let mut evidence_max = [f64::NEG_INFINITY; 2];
    for point in evidence.polygons().iter().flatten() {
        for axis in 0..2 {
            evidence_min[axis] = evidence_min[axis].min(point[axis]);
            evidence_max[axis] = evidence_max[axis].max(point[axis]);
        }
    }
    assert_eq!(frame.source_bounds, [evidence_min, evidence_max]);
    assert_eq!(frame.source_center, [4.0, 4.0]);
    assert_eq!([min[0], min[1]], frame.model_point(evidence_min), "coarse uses the final model frame");
    assert_eq!([max[0], max[1]], frame.model_point(evidence_max));
}

#[test]
fn coarse_rectangle_precedes_contour_reconstruction_and_preserves_requested_settings() {
    let mut host = Host::default();
    let requested = Settings { height: 3.0, tolerance: 0.1 };
    let job = host.replace_drawing(drawing(), requested).unwrap();
    let ready = std::thread::spawn(move || job.coarse()).join().unwrap().unwrap();
    assert_eq!(ready.preview().revision(), host.revision());
    assert!(host.current().is_none());
    assert_eq!(host.events_after(0).unwrap().len(), 1, "computation does not publish");
    let coarse = host.publish_coarse(&ready).unwrap();
    assert!(Arc::ptr_eq(&coarse, ready.preview()));
    assert!(Arc::ptr_eq(&coarse, host.coarse_preview().unwrap()));
    assert_eq!(coarse.mesh().volume, 108.0, "bounding prism fills the hole");
    assert_eq!(coarse.mesh().open, 0);
    assert_eq!(coarse.mesh().provenance.input.tolerance.value, requested.tolerance);
    let (min, max) = mesh_bounds(coarse.mesh());
    assert_eq!(max[0] - min[0], 6.0);
    assert_eq!(max[1] - min[1], 6.0);
    assert_eq!((min[2], max[2]), (0.0, requested.height));
    assert!(host.current().is_none());
    assert_eq!(host.in_flight(), 1, "coarse publication keeps the original permit");
    let completed = std::thread::spawn(move || ready.finish()).join().unwrap().unwrap();
    assert_eq!(host.in_flight(), 1);
    let final_result = host.publish(completed).unwrap();
    assert!(host.coarse_preview().is_none());
    assert_eq!(final_result.mesh().volume, 96.0, "finish reconstructs the contour and hole");
    assert_eq!(final_result.settings(), requested);
    assert_eq!((min, max), ([-3.0, -3.0, 0.0], [3.0, 3.0, requested.height]));
    assert_eq!(mesh_bounds(final_result.mesh()), (min, max));
    assert_evidence_bounds(final_result.evidence(), final_result.model().frame(), min, max);
    assert_ne!(coarse.mesh().provenance.source, final_result.mesh().provenance.source);
    assert_eq!(host.in_flight(), 0);
    assert_eq!(
        host.events_after(0)
            .unwrap()
            .iter()
            .map(|event| (event.sequence, event.revision, event.kind, event.settings))
            .collect::<Vec<_>>(),
        [
            (1, 1, EventKind::Invalidated(Change::Drawing), requested),
            (2, 1, EventKind::CoarsePreview, requested),
            (3, 1, EventKind::Committed, requested),
        ]
    );
    // Retained approximations remain explicitly separate after final publication.
    drop(host);
    assert_eq!(coarse.mesh().volume, 108.0);
    assert_eq!(final_result.mesh().volume, 96.0);
}

#[test]
fn foreign_and_duplicate_previews_leave_host_and_permit_unchanged() {
    let mut owner = Host::default();
    let mut foreign = Host::default();
    let ready = owner.replace_drawing(drawing(), settings(3.0)).unwrap().coarse().unwrap();
    let pending = foreign.replace_drawing(drawing(), settings(3.0)).unwrap();
    assert_eq!(owner.revision(), foreign.revision());
    let before = foreign.events_after(0).unwrap();
    assert_eq!(foreign.publish_coarse(&ready).err().unwrap().code.as_str(), "foreign_host");
    assert_eq!(foreign.events_after(0).unwrap(), before);
    assert!(foreign.current().is_none());
    assert_eq!(owner.in_flight(), 1);
    let preview = owner.publish_coarse(&ready).unwrap();
    let before = owner.events_after(0).unwrap();
    assert_eq!(owner.publish_coarse(&ready).err().unwrap().code.as_str(), "stale_revision");
    assert_eq!(owner.events_after(0).unwrap(), before);
    assert!(owner.current().is_none());
    assert_eq!(owner.in_flight(), 1);
    drop(ready);
    drop(pending);
    assert_eq!(owner.in_flight(), 0);
    assert_eq!(foreign.in_flight(), 0);
    assert_eq!(preview.mesh().volume, 108.0, "readers do not retain job permits");
}

#[test]
fn stale_coarse_cannot_replace_a_newer_preview_or_final_result() {
    let mut host = Host::default();
    let old = host.replace_drawing(drawing(), settings(3.0)).unwrap().coarse().unwrap();
    let old_preview = host.publish_coarse(&old).unwrap();
    let latest = host.revise(settings(5.0)).unwrap().coarse().unwrap();
    let before = host.events_after(0).unwrap();
    assert_eq!(host.publish_coarse(&old).err().unwrap().code.as_str(), "stale_revision");
    assert_eq!(host.events_after(0).unwrap(), before);
    let current_preview = host.publish_coarse(&latest).unwrap();
    assert_eq!(current_preview.mesh().volume, 180.0);
    let current = host.publish(latest.finish().unwrap()).unwrap();
    let before = host.events_after(0).unwrap();
    assert_eq!(host.publish_coarse(&old).err().unwrap().code.as_str(), "stale_revision");
    assert_eq!(host.events_after(0).unwrap(), before);
    assert!(Arc::ptr_eq(host.current().unwrap(), &current));
    // Triangle-volume accumulation may round one ULP above the analytic prism volume.
    assert!((current.mesh().volume - 160.0).abs() <= 4.0 * f64::EPSILON * 160.0);
    assert_eq!(old_preview.mesh().volume, 108.0);
    drop(old);
    assert_eq!(host.in_flight(), 0);
    let next = host.revise(settings(6.0)).unwrap().coarse().unwrap();
    assert!(host.publish_coarse(&next).is_ok(), "a new revision permits another coarse publication");
}

#[test]
fn ready_handles_keep_admission_bounded_and_drop_or_failure_releases_one_permit() {
    let mut host = Host::default();
    let mut ready = Vec::new();
    ready.push(host.replace_drawing(drawing(), settings(3.0)).unwrap().coarse().unwrap());
    for index in 1..MAX_IN_FLIGHT {
        ready.push(host.revise(settings(3.0 + index as f64)).unwrap().coarse().unwrap());
    }
    assert_eq!(host.in_flight(), MAX_IN_FLIGHT);
    let before = host.events_after(0).unwrap();
    assert_eq!(host.revise(settings(42.0)).err().unwrap().code.as_str(), "resource_limit");
    assert_eq!(host.events_after(0).unwrap(), before);
    let latest = ready.pop().unwrap();
    let retained = host.publish_coarse(&latest).unwrap();
    assert_eq!(host.in_flight(), MAX_IN_FLIGHT);
    drop(latest);
    assert_eq!(host.in_flight(), MAX_IN_FLIGHT - 1);
    let bad = host.revise(Settings { height: 3.0, tolerance: f64::MIN_POSITIVE }).unwrap();
    let before = host.events_after(0).unwrap();
    assert!(bad.coarse().is_err());
    assert_eq!(host.events_after(0).unwrap(), before);
    assert_eq!(host.in_flight(), MAX_IN_FLIGHT - 1);
    drop(ready);
    assert_eq!(host.in_flight(), 0);
    assert!(!retained.mesh().mesh.as_ref().unwrap().data.triangles.is_empty());
}

#[test]
fn coarse_finish_preserves_existing_contour_reuse_for_tolerance_edits() {
    let mut host = Host::default();
    let first = host.replace_drawing(drawing(), settings(3.0)).unwrap().run().unwrap();
    let first = host.publish(first).unwrap();
    let next = host.revise(Settings { height: 3.0, tolerance: 0.1 }).unwrap().coarse().unwrap();
    let preview = host.publish_coarse(&next).unwrap();
    let second = host.publish(next.finish().unwrap()).unwrap();
    assert!(Arc::ptr_eq(first.model(), second.model()));
    assert!(Arc::ptr_eq(first.evidence(), second.evidence()));
    assert_eq!(second.mesh().provenance.input.tolerance.value, 0.1);
    assert_ne!(preview.mesh().provenance.source, second.mesh().provenance.source);
    assert_eq!(host.in_flight(), 0);
}

#[test]
fn coarse_and_finish_can_complete_after_host_drop_without_publishing() {
    let mut host = Host::default();
    let job = host.replace_drawing(drawing(), settings(3.0)).unwrap();
    drop(host);
    let ready = std::thread::spawn(move || job.coarse()).join().unwrap().unwrap();
    let preview = Arc::clone(ready.preview());
    let completed = ready.finish().unwrap();
    drop(completed);
    assert_eq!(preview.revision(), 1);
    assert_eq!(preview.mesh().volume, 108.0);
}
