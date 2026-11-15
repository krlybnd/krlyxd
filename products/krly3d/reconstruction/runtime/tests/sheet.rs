#![allow(clippy::unwrap_used)]
use krly3d_reconstruction::sheet::{PartSpec, PartStage, PartStatus, Sheet};
use krlyvis_api::{DrawingCall, DrawingImage, DrawingInput, PROTOCOL};
use krlyxd::{CallControl, FakeClock};
use std::sync::Arc;
use std::time::Duration;

fn drawing() -> DrawingCall {
    let mut data = b"P5 40 24 255\n".to_vec();
    for y in 0..24 {
        for x in 0..40 {
            let left = (2..14).contains(&x) && (2..18).contains(&y);
            let hole = (6..10).contains(&x) && (6..12).contains(&y);
            let right = (22..36).contains(&x) && (4..20).contains(&y);
            data.push(if left && !hole || right { 0 } else { 255 });
        }
    }
    DrawingCall {
        protocol: PROTOCOL.into(),
        request_id: "sheet".into(),
        input: DrawingInput {
            image: DrawingImage { format: "pnm".into(), data },
            pixel_unit: 0.25,
            simplify: 0.0,
            seed: None,
            source: "sheet-fixture".into(),
        },
    }
}
fn part(id: &str, seed: [f64; 2], height: f64) -> PartSpec {
    PartSpec { id: id.into(), seed, height }
}
fn parts() -> Vec<PartSpec> {
    vec![part("Ring", [3., 3.], 2.), part("Plate", [23., 5.], 5.)]
}
fn sheet(parts: Vec<PartSpec>, control: Arc<CallControl>) -> Sheet {
    Sheet::new(drawing(), parts, 0.05, control).unwrap()
}

#[test]
fn selected_parts_keep_holes_world_placement_and_pinned_review_sources() {
    let mut sheet = sheet(parts(), Arc::new(CallControl::new()));
    let mut events = Vec::new();
    while sheet
        .advance(|id, stage| {
            let kind = match stage {
                PartStage::Evidence(_) => "evidence",
                PartStage::Coarse(_) => "coarse",
                PartStage::Editable(_) => "editable",
                PartStage::Preview(_) => "preview",
                PartStage::Validation(_) => "validation",
            };
            events.push((id.to_string(), kind));
            Ok(())
        })
        .unwrap()
    {}
    assert!(sheet.is_complete());
    for (index, record) in sheet.records().iter().enumerate() {
        assert_eq!(record.status(), PartStatus::Completed);
        let result = record.result().unwrap();
        assert!((result.mesh().volume - [21.0, 70.0][index]).abs() < 1e-8);
        assert_eq!(result.mesh().open, 0);
        assert_eq!(result.evidence().polygons().len(), [2, 1][index]);
        assert_eq!(result.evidence().provenance().image_sha256, sheet.provenance().image_sha256);
        let source = result.model().snapshot().identity();
        assert_eq!(&result.review().preview().unwrap().provenance.source, source);
        assert_eq!(&result.review().likeness().unwrap().provenance.source, source);
        assert_eq!(result.review().likeness().unwrap().matches, Some(true));
        let xs: Vec<_> = result.evidence().polygons().into_iter().flatten().map(|p| p[0]).collect();
        assert_eq!(xs.iter().copied().fold(f64::INFINITY, f64::min), [0.5, 5.5][index]);
    }
    let expected: Vec<_> = ["Ring", "Plate"]
        .into_iter()
        .flat_map(|id| {
            ["evidence", "coarse", "editable", "preview", "validation"].map(|kind| (id.to_string(), kind))
        })
        .collect();
    assert_eq!(events, expected);
}

#[test]
fn duplicate_selection_is_rejected_before_second_geometry_and_retains_first() {
    let mut specs = parts();
    specs[1].seed = [4., 4.];
    let mut sheet = sheet(specs, Arc::new(CallControl::new()));
    sheet.advance(|_, _| Ok(())).unwrap();
    let first = Arc::clone(sheet.records()[0].result().unwrap());
    let mut called = 0;
    assert_eq!(
        sheet
            .advance(|_, _| {
                called += 1;
                Ok(())
            })
            .err()
            .unwrap()
            .code
            .as_str(),
        "duplicate_component"
    );
    assert_eq!(called, 0);
    assert!(Arc::ptr_eq(&first, sheet.records()[0].result().unwrap()));
    assert_eq!(sheet.records()[1].status(), PartStatus::Failed);
    assert!(sheet.records()[1].result().is_none());
    assert!(!sheet.is_complete());
    assert!(sheet
        .advance(|_, _| {
            called += 1;
            Ok(())
        })
        .is_err());
    assert_eq!(called, 0);
}

#[test]
fn seed_failure_preserves_completed_parts_and_leaves_later_parts_pending() {
    let specs = vec![parts()[0].clone(), part("Bad", [100., 100.], 3.), parts()[1].clone()];
    let mut sheet = sheet(specs, Arc::new(CallControl::new()));
    sheet.advance(|_, _| Ok(())).unwrap();
    assert!(sheet.advance(|_, _| Ok(())).is_err());
    assert_eq!(
        sheet.records().iter().map(|r| r.status()).collect::<Vec<_>>(),
        [PartStatus::Completed, PartStatus::Failed, PartStatus::Pending]
    );
    let retained = Arc::clone(sheet.records()[0].result().unwrap());
    drop(sheet);
    assert_eq!(retained.mesh().volume, 21.);
    assert!(retained.model().to_k3d().unwrap().contains("extrude"));
}

#[test]
fn cancellation_in_a_callback_retains_first_part_and_stops_remaining_stages() {
    let control = Arc::new(CallControl::new());
    let mut specs = parts();
    specs.push(part("Later", [4., 4.], 3.));
    let mut sheet = sheet(specs, control.clone());
    sheet.advance(|_, _| Ok(())).unwrap();
    let mut stages = 0;
    let error = sheet
        .advance(|_, stage| {
            stages += 1;
            if matches!(stage, PartStage::Coarse(_)) {
                control.cancel();
            }
            Ok(())
        })
        .err()
        .unwrap();
    assert_eq!(error.code.as_str(), "cancelled");
    assert_eq!(stages, 2);
    assert_eq!(
        sheet.records().iter().map(|r| r.status()).collect::<Vec<_>>(),
        [PartStatus::Completed, PartStatus::Cancelled, PartStatus::Pending]
    );
}

#[test]
fn one_deadline_is_shared_across_parts_not_restarted_for_each_selection() {
    let clock = Arc::new(FakeClock::default());
    let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(10)));
    let mut sheet = sheet(parts(), control);
    sheet.advance(|_, _| Ok(())).unwrap();
    clock.advance(Duration::from_secs(10));
    let mut stages = 0;
    assert_eq!(
        sheet
            .advance(|_, _| {
                stages += 1;
                Ok(())
            })
            .err()
            .unwrap()
            .code
            .as_str(),
        "deadline_exceeded"
    );
    assert_eq!(stages, 0);
    assert_eq!(sheet.records()[0].status(), PartStatus::Completed);
    assert_eq!(sheet.records()[1].status(), PartStatus::Failed);
}

#[test]
fn observer_failure_cannot_publish_a_partial_part() {
    let mut sheet = sheet(parts(), Arc::new(CallControl::new()));
    let result = sheet.advance(|_, stage| {
        if matches!(stage, PartStage::Validation(_)) {
            Err(krlyxd::Diagnostic { code: krlyxd::Id::new("io"), message: "observer failed".into() })
        } else {
            Ok(())
        }
    });
    assert_eq!(result.err().unwrap().code.as_str(), "io");
    assert!(sheet.records()[0].result().is_none());
    assert_eq!(sheet.records()[0].status(), PartStatus::Failed);
    assert_eq!(sheet.records()[1].status(), PartStatus::Pending);
}

#[test]
fn whole_selection_validation_precedes_execution() {
    let mut invalid = vec![vec![], vec![parts()[0].clone(); 17], vec![parts()[0].clone(); 2]];
    for id in ["", "../a", "1part", "bad/name", "é", &"a".repeat(65)] {
        invalid.push(vec![part(id, [3., 3.], 2.)]);
    }
    invalid.push(vec![part("Bad", [f64::NAN, 3.], 2.)]);
    invalid.push(vec![part("Bad", [3., 3.], 0.)]);
    for specs in invalid {
        assert!(Sheet::new(drawing(), specs, 0.05, Arc::new(CallControl::new())).is_err());
    }
    let control = Arc::new(CallControl::new());
    control.cancel();
    assert_eq!(Sheet::new(drawing(), parts(), 0.05, control).err().unwrap().code.as_str(), "cancelled");
}
