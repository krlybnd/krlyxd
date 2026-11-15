#![allow(clippy::unwrap_used)]

use krly3d_reconstruction::{Host, Settings};
use krlyvis_api::{DrawingCall, DrawingImage, DrawingInput, PROTOCOL};
use krlyxd::{CallControl, Diagnostic, FakeClock};
use std::sync::Arc;
use std::time::Duration;

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
        request_id: "review-fixture".into(),
        input: DrawingInput {
            image: DrawingImage { format: "pnm".into(), data },
            pixel_unit: 0.5,
            simplify: 0.0,
            seed: None,
            source: "review-ring".into(),
        },
    }
}

fn settings() -> Settings {
    Settings { height: 3.0, tolerance: 0.05 }
}

fn assert_diagnostic<T>(result: Result<T, Diagnostic>, expected_code: &str) {
    assert_eq!(result.err().unwrap().code.as_str(), expected_code);
}

#[test]
fn render_then_compare_retains_optional_arc_results_from_the_model_snapshot() {
    let mut host = Host::default();
    let control = Arc::new(CallControl::new());
    let job = host.replace_drawing_controlled(drawing(), settings(), Arc::clone(&control)).unwrap();
    let completed = job.run().unwrap();

    assert!(completed.result().review().preview().is_none());
    assert!(completed.result().review().likeness().is_none());

    let rendered = completed.render().unwrap();
    assert!(rendered.result().review().preview().is_some());
    assert!(rendered.result().review().likeness().is_none());
    let preview = Arc::clone(rendered.result().review().preview().unwrap());

    let compared = rendered.compare().unwrap();
    let provisional = compared.result();
    let preview_after_compare = Arc::clone(provisional.review().preview().unwrap());
    let likeness = Arc::clone(provisional.review().likeness().unwrap());
    let model_source = provisional.model().snapshot().identity();
    assert_eq!(&preview.provenance.source, model_source);
    assert_eq!(&likeness.provenance.source, model_source);
    assert_eq!(likeness.matches, Some(true));
    assert!(Arc::ptr_eq(&preview, &preview_after_compare));
    assert!(host.current().is_none(), "review remains provisional until publish");

    let published = host.publish(compared).unwrap();
    assert!(Arc::ptr_eq(published.review().preview().unwrap(), &preview));
    assert!(Arc::ptr_eq(published.review().likeness().unwrap(), &likeness));
    assert!(Arc::ptr_eq(host.current().unwrap(), &published));
    assert_eq!(host.in_flight(), 0);
}

#[test]
fn controlled_job_run_observes_cancellation() {
    let mut host = Host::default();
    let control = Arc::new(CallControl::new());
    let job = host.replace_drawing_controlled(drawing(), settings(), Arc::clone(&control)).unwrap();
    control.cancel();

    assert_diagnostic(job.run(), "cancelled");
    assert!(host.current().is_none());
    assert_eq!(host.in_flight(), 0);
}

#[test]
fn cancellation_before_render_stops_a_completed_controlled_job() {
    let mut host = Host::default();
    let control = Arc::new(CallControl::new());
    let completed =
        host.replace_drawing_controlled(drawing(), settings(), Arc::clone(&control)).unwrap().run().unwrap();
    control.cancel();

    assert_diagnostic(completed.render(), "cancelled");
    assert!(host.current().is_none());
    assert_eq!(host.in_flight(), 0);
}

#[test]
fn cancellation_before_compare_stops_the_same_controlled_review() {
    let mut host = Host::default();
    let control = Arc::new(CallControl::new());
    let completed =
        host.replace_drawing_controlled(drawing(), settings(), Arc::clone(&control)).unwrap().run().unwrap();
    let rendered = completed.render().unwrap();
    control.cancel();

    assert_diagnostic(rendered.compare(), "cancelled");
    assert!(host.current().is_none());
    assert_eq!(host.in_flight(), 0);
}

#[test]
fn cancellation_after_completion_rejects_final_publish() {
    let mut host = Host::default();
    let control = Arc::new(CallControl::new());
    let completed =
        host.replace_drawing_controlled(drawing(), settings(), Arc::clone(&control)).unwrap().run().unwrap();
    control.cancel();

    assert_diagnostic(host.publish(completed), "cancelled");
    assert!(host.current().is_none());
    assert_eq!(host.in_flight(), 0);
}

#[test]
fn compare_before_render_is_rejected_as_a_stage_error() {
    let mut host = Host::default();
    let completed = host.replace_drawing(drawing(), settings()).unwrap().run().unwrap();

    assert_diagnostic(completed.compare(), "stage");
    assert!(host.current().is_none());
    assert_eq!(host.in_flight(), 0);
}

#[test]
fn half_review_cannot_publish_but_core_only_results_remain_compatible() {
    let mut half_review_host = Host::default();
    let completed = half_review_host.replace_drawing(drawing(), settings()).unwrap().run().unwrap();
    let rendered = completed.render().unwrap();
    assert!(rendered.result().review().preview().is_some());
    assert!(rendered.result().review().likeness().is_none());

    assert_diagnostic(half_review_host.publish(rendered), "stage");
    assert!(half_review_host.current().is_none());
    assert_eq!(half_review_host.in_flight(), 0);

    let mut core_host = Host::default();
    let core_only = core_host.replace_drawing(drawing(), settings()).unwrap().run().unwrap();
    assert!(core_only.result().review().preview().is_none());
    assert!(core_only.result().review().likeness().is_none());
    let published = core_host.publish(core_only).unwrap();
    assert!(Arc::ptr_eq(core_host.current().unwrap(), &published));
    assert_eq!(core_host.in_flight(), 0);
}

#[test]
fn fake_clock_deadline_at_final_publish_rejects_reviewed_result() {
    let mut host = Host::default();
    let clock = Arc::new(FakeClock::default());
    let deadline = Duration::from_secs(5);
    let control = Arc::new(CallControl::with_deadline(clock.clone(), deadline));
    let completed =
        host.replace_drawing_controlled(drawing(), settings(), Arc::clone(&control)).unwrap().run().unwrap();
    let reviewed = completed.render().unwrap().compare().unwrap();
    clock.advance(deadline);

    assert_diagnostic(host.publish(reviewed), "deadline_exceeded");
    assert!(host.current().is_none());
    assert_eq!(host.in_flight(), 0);
}
