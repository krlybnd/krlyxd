#![allow(clippy::unwrap_used)]

use krly3d_reconstruction::{Change, EventKind, Host, Settings, EVENT_CAPACITY, MAX_IN_FLIGHT};
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
        request_id: "fixture".into(),
        input: DrawingInput {
            image: DrawingImage { format: "pnm".into(), data },
            pixel_unit: 0.5,
            simplify: 0.0,
            seed: None,
            source: "ring".into(),
        },
    }
}

fn settings(height: f64, tolerance: f64) -> Settings {
    Settings { height, tolerance }
}

#[test]
fn native_route_preserves_holes_and_snapshot_identity() {
    let mut host = Host::default();
    let job = host.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap();
    assert_eq!(host.in_flight(), 1);
    let completed = std::thread::spawn(move || job.run()).join().unwrap().unwrap();
    assert_eq!(host.in_flight(), 1, "unpublished result retains admission slot");
    let result = host.publish(completed).unwrap();
    assert_eq!(result.evidence().material_area(), 32.0);
    assert_eq!(result.mesh().volume, 96.0);
    assert_eq!(result.mesh().open, 0);
    assert_eq!(result.mesh().provenance.source, result.model().snapshot().identity().source);
    assert_eq!(result.evidence().polygons().len(), 2);
    // Evidence stays in its source frame; model geometry is centered in CAD XY.
    let frame = result.model().frame();
    assert_eq!(frame.source_bounds, [[1.0, 1.0], [7.0, 7.0]]);
    assert_eq!(frame.source_center, [4.0, 4.0]);
    let placed: Vec<Vec<_>> = result
        .evidence()
        .polygons()
        .iter()
        .map(|polygon| polygon.iter().map(|point| frame.model_point(*point)).collect())
        .collect();
    assert_eq!(result.model().region().polygons().unwrap(), placed);
    assert_eq!(host.in_flight(), 0);
    assert!(result.model().to_k3d().unwrap().contains("extrude"));
    let events = host.events_after(0).unwrap();
    assert_eq!(
        events.iter().map(|event| (event.sequence, event.revision, event.kind)).collect::<Vec<_>>(),
        [(1, 1, EventKind::Invalidated(Change::Drawing)), (2, 1, EventKind::Committed),]
    );
}

#[test]
fn requested_outline_tolerance_survives_native_reconstruction_and_revisions() {
    let mut call = drawing();
    call.input.simplify = 1.0;
    let mut host = Host::default();
    let completed = host.replace_drawing(call, settings(3.0, 0.05)).unwrap().run().unwrap();
    let result = host.publish(completed).unwrap();
    assert_eq!(result.evidence().provenance().simplify, 1.0);
    assert_eq!(result.evidence().polygons().len(), 2);
    assert_eq!(result.mesh().volume, 96.0);
    assert_eq!(result.mesh().open, 0);
    assert_eq!(result.evidence().to_result("positive-opt-in").unwrap().provenance.simplify, 1.0);
    let revised = host.revise(settings(4.0, 0.05)).unwrap().run().unwrap();
    let revised = host.publish(revised).unwrap();
    assert_eq!(revised.evidence().provenance().simplify, 1.0);
    assert!(Arc::ptr_eq(result.evidence(), revised.evidence()));
}

#[test]
fn edits_reuse_only_unaffected_artifacts_and_readers_keep_old_snapshots() {
    let mut host = Host::default();
    let first = host.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap().run().unwrap();
    let first = host.publish(first).unwrap();
    let changed = host.revise(settings(4.0, 0.05)).unwrap();
    assert!(host.current().is_none(), "old geometry cannot masquerade as current");
    let second = host.publish(changed.run().unwrap()).unwrap();
    assert!(Arc::ptr_eq(first.evidence(), second.evidence()));
    assert!(!Arc::ptr_eq(first.model(), second.model()));
    assert!(Arc::ptr_eq(first.model().region(), second.model().region()));
    assert_eq!(first.mesh().volume, 96.0);
    assert!((second.mesh().volume - 128.0).abs() <= 4.0 * f64::EPSILON * 128.0);
    let third = host.revise(settings(4.0, 0.1)).unwrap().run().unwrap();
    let third = host.publish(third).unwrap();
    assert!(Arc::ptr_eq(second.model(), third.model()));
    assert!(Arc::ptr_eq(second.evidence(), third.evidence()));
    assert!(!Arc::ptr_eq(second.mesh(), third.mesh()));
    assert_eq!(second.mesh().provenance.source, third.mesh().provenance.source);
    let replace = host.replace_drawing(drawing(), settings(4.0, 0.1)).unwrap().run().unwrap();
    let replace = host.publish(replace).unwrap();
    assert!(!Arc::ptr_eq(replace.evidence(), third.evidence()));
    assert!(!Arc::ptr_eq(replace.model().region(), third.model().region()));
    drop(host);
    assert_eq!(first.mesh().volume, 96.0);
    assert_eq!(first.evidence().material_area(), 32.0);
}

#[test]
fn reverse_completion_cannot_publish_stale_results() {
    let mut host = Host::default();
    let old = host.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap();
    let latest = host.revise(settings(5.0, 0.05)).unwrap();
    let current = host.publish(latest.run().unwrap()).unwrap();
    let error = host.publish(old.run().unwrap()).err().unwrap();
    assert_eq!(error.code.as_str(), "stale_revision");
    assert!(Arc::ptr_eq(host.current().unwrap(), &current));
    assert!((current.mesh().volume - 160.0).abs() <= 4.0 * f64::EPSILON * 160.0);
    assert_eq!(host.events_after(0).unwrap().len(), 3);
    assert_eq!(host.in_flight(), 0);
}

#[test]
fn delayed_proposal_cannot_change_newer_user_state() {
    let mut host = Host::default();
    drop(host.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap());
    let base = host.revision();
    let latest = host.revise(settings(5.0, 0.05)).unwrap();
    let before = host.events_after(0).unwrap();
    assert_eq!(
        host.revise_if_current(base, settings(42.0, 0.05)).err().unwrap().code.as_str(),
        "stale_revision"
    );
    assert_eq!(host.events_after(0).unwrap(), before);
    let latest = host.publish(latest.run().unwrap()).unwrap();
    assert!((latest.mesh().volume - 160.0).abs() <= 4.0 * f64::EPSILON * 160.0);
    assert!(host.revise_if_current(host.revision(), settings(6.0, 0.05)).is_ok());
}

#[test]
fn another_host_cannot_accept_a_matching_revision_number() {
    let mut owner = Host::default();
    let mut foreign = Host::default();
    let completed = owner.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap().run().unwrap();
    let _pending = foreign.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap();
    assert_eq!(foreign.publish(completed).err().unwrap().code.as_str(), "foreign_host");
    assert!(foreign.current().is_none());
    assert_eq!(owner.in_flight(), 0);
}

#[test]
fn invalid_commands_and_capacity_rejection_leave_state_and_events_untouched() {
    let mut host = Host::default();
    let current = host.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap().run().unwrap();
    let current = host.publish(current).unwrap();
    let before = host.events_after(0).unwrap();
    assert!(host.revise(settings(f64::NAN, 0.05)).is_err());
    let mut invalid = drawing();
    invalid.input.image.data.clear();
    assert!(host.replace_drawing(invalid, settings(3.0, 0.05)).is_err());
    assert_eq!(host.revision(), 1);
    assert_eq!(host.in_flight(), 0);
    assert_eq!(host.events_after(0).unwrap(), before);
    assert!(Arc::ptr_eq(host.current().unwrap(), &current));
    let mut jobs = Vec::new();
    for i in 0..MAX_IN_FLIGHT {
        jobs.push(host.revise(settings(4.0 + i as f64, 0.05)).unwrap());
    }
    let revision = host.revision();
    assert_eq!(host.revise(settings(42.0, 0.05)).err().unwrap().code.as_str(), "resource_limit");
    assert_eq!(host.revision(), revision);
    drop(jobs);
    assert_eq!(host.in_flight(), 0);
    assert!(host.revise(settings(42.0, 0.05)).is_ok());
}

#[test]
fn failed_work_releases_slot_without_publishing_or_reviving_old_result() {
    let mut host = Host::default();
    let good = host.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap().run().unwrap();
    let _good = host.publish(good).unwrap();
    // Positive tolerance passes structural validation but is below native accuracy.
    let job = host.revise(settings(3.0, f64::MIN_POSITIVE)).unwrap();
    assert!(job.run().is_err());
    assert!(host.current().is_none());
    assert_eq!(host.in_flight(), 0);
    assert_eq!(
        host.events_after(0).unwrap().last().unwrap().kind,
        EventKind::Invalidated(Change::MeshTolerance)
    );
}

#[test]
fn event_history_is_bounded_and_lag_is_explicit() {
    let mut host = Host::default();
    drop(host.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap());
    for _ in 0..EVENT_CAPACITY {
        drop(host.revise(settings(3.0, 0.05)).unwrap());
    }
    assert_eq!(host.events_after(0).unwrap_err().code.as_str(), "event_gap");
    assert_eq!(host.events_after(1).unwrap().len(), EVENT_CAPACITY);
    assert!(host.events_after(u64::MAX).is_err());
    assert!(host.events_after(EVENT_CAPACITY as u64 + 1).unwrap().is_empty());
}

#[test]
fn stopped_edit_and_proposal_keep_the_published_revision_and_capacity() {
    use krlyxd::{CallControl, FakeClock};
    use std::time::Duration;
    let mut host = Host::default();
    let first = host.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap().run().unwrap();
    let first = host.publish(first).unwrap();
    let before = host.events_after(0).unwrap();
    let cancelled = Arc::new(CallControl::new());
    cancelled.cancel();
    let expired = Arc::new(CallControl::with_deadline(Arc::new(FakeClock::default()), Duration::ZERO));
    for (control, code) in [(cancelled, "cancelled"), (expired, "deadline_exceeded")] {
        assert_eq!(
            host.revise_controlled(settings(9.0, 0.1), control.clone()).err().unwrap().code.as_str(),
            code
        );
        assert_eq!(
            host.revise_if_current_controlled(first.revision(), settings(9.0, 0.1), control)
                .err()
                .unwrap()
                .code
                .as_str(),
            code
        );
        assert!(Arc::ptr_eq(host.current().unwrap(), &first));
        assert_eq!(host.revision(), first.revision());
        assert_eq!(host.events_after(0).unwrap(), before);
        assert_eq!(host.in_flight(), 0);
    }
}

#[test]
fn edit_deadline_at_admission_cannot_invalidate_current_state() {
    use krlyxd::{CallControl, Clock};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    struct AdmissionClock(AtomicUsize);
    impl Clock for AdmissionClock {
        fn now(&self) -> Duration {
            Duration::from_secs(self.0.fetch_add(1, Ordering::SeqCst) as u64)
        }
    }
    let mut host = Host::default();
    let first = host.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap().run().unwrap();
    let first = host.publish(first).unwrap();
    let before = host.events_after(0).unwrap();
    let control = Arc::new(CallControl::with_deadline(
        Arc::new(AdmissionClock(AtomicUsize::new(0))),
        Duration::from_secs(1),
    ));
    assert_eq!(
        host.revise_controlled(settings(6.0, 0.05), control).err().unwrap().code.as_str(),
        "deadline_exceeded"
    );
    assert!(Arc::ptr_eq(host.current().unwrap(), &first));
    assert_eq!(host.events_after(0).unwrap(), before);
    assert_eq!(host.in_flight(), 0);
}

#[test]
fn controlled_revision_retains_reuse_and_stops_review_publication() {
    use krlyxd::CallControl;
    let mut host = Host::default();
    let first = host.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap().run().unwrap();
    let first = host.publish(first).unwrap();
    let control = Arc::new(CallControl::new());
    let revised =
        host.revise_if_current_controlled(first.revision(), settings(3.0, 0.1), control.clone()).unwrap();
    let revised = revised.run().unwrap().render().unwrap().compare().unwrap();
    assert!(Arc::ptr_eq(first.model(), revised.result().model()));
    assert!(Arc::ptr_eq(first.evidence(), revised.result().evidence()));
    control.cancel();
    assert_eq!(host.publish(revised).err().unwrap().code.as_str(), "cancelled");
    assert!(host.current().is_none());
    assert_eq!(host.in_flight(), 0);
    let retry = host.revise(settings(3.0, 0.1)).unwrap().run().unwrap();
    let retry = host.publish(retry).unwrap();
    assert!(Arc::ptr_eq(first.model(), retry.model()));
}

#[test]
fn controlled_stale_proposal_cannot_touch_newer_work_even_with_cancelled_token() {
    use krlyxd::CallControl;
    let mut host = Host::default();
    let pending = host.replace_drawing(drawing(), settings(3.0, 0.05)).unwrap();
    let old = pending.revision();
    let latest = host.revise(settings(4.0, 0.05)).unwrap();
    let before = host.events_after(0).unwrap();
    let control = Arc::new(CallControl::new());
    control.cancel();
    assert_eq!(
        host.revise_if_current_controlled(old, settings(8.0, 0.1), control).err().unwrap().code.as_str(),
        "stale_revision"
    );
    assert_eq!(host.revision(), latest.revision());
    assert_eq!(host.events_after(0).unwrap(), before);
    assert_eq!(host.in_flight(), 2);
    drop(pending);
    host.publish(latest.run().unwrap()).unwrap();
}
