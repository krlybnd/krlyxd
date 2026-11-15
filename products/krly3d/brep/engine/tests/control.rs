#![allow(clippy::unwrap_used)]

use krly2d_api::Region;
use krly3d_brep::planar::{PlanarCall, PlanarInput, PROTOCOL};
use krly3d_brep_engine::{
    build_planar, build_planar_retained_controlled, build_region_retained, build_region_retained_controlled,
    BrepEngine,
};
use krly3d_kernel::model::Statement;
use krly3d_kernel::ports::interpret::{body_of, mesh_of};
use krly3d_kernel::ports::measure::measure;
use krly3d_kernel::schema::Params;
use krly3d_kernel::topo::check;
use krlyxd::{CallControl, FakeClock, LengthUnit};
use std::sync::Arc;
use std::time::Duration;

fn call() -> PlanarCall {
    PlanarCall {
        protocol: PROTOCOL.into(),
        request_id: "controlled".into(),
        input: PlanarInput {
            polygons: vec![
                vec![[10., -4.], [16., -4.], [16., 0.], [10., 0.]],
                vec![[11., -3.], [11., -1.], [13., -1.], [13., -3.]],
            ],
            height: 3.,
            length_unit: "model".into(),
            source: "vector:plate".into(),
        },
    }
}

#[test]
fn controlled_region_and_planar_paths_preserve_native_wire_geometry_and_quality() {
    for reverse in [false, true] {
        let mut call = call();
        if reverse {
            for polygon in &mut call.input.polygons {
                polygon.reverse();
            }
        }
        let input = Arc::new(Region::from_polygons(call.input.polygons.clone(), LengthUnit::Model).unwrap());
        let region = build_region_retained_controlled(
            Arc::clone(&input),
            call.input.height,
            call.request_id.clone(),
            call.input.source.clone(),
            Arc::new(CallControl::new()),
        )
        .unwrap();
        let planar = build_planar_retained_controlled(call.clone(), Arc::new(CallControl::new())).unwrap();
        let legacy = build_region_retained(
            Arc::clone(&input),
            call.input.height,
            call.request_id.clone(),
            call.input.source.clone(),
        )
        .unwrap();
        let wire = build_planar(call).unwrap();
        if !reverse {
            // Independent native-document entry point: bypass all planar bridge
            // builders to catch changes to the emitted recipe or construction.
            let native = BrepEngine::from_k3d(
                "k3d 1\nlayer geo\noutline_profile = profile path=\"M 10,-4 L 16,-4 L 16,0 L 10,0 Z M 11,-3 L 11,-1 L 13,-1 L 13,-3 Z\" plane=xy\npart = extrude outline_profile h=3\n",
            ).unwrap();
            assert_eq!(native.to_k3d().unwrap(), wire.model);
            assert_eq!(native.snapshot("part@1").unwrap().identity().build_key, wire.provenance.build_key);
        }
        assert!(Arc::ptr_eq(&region.region, &input));
        for retained in [region, planar, legacy] {
            assert_eq!(retained.engine.to_k3d().unwrap(), wire.model);
            assert_eq!(retained.snapshot.identity().build_key, wire.provenance.build_key);
            assert_eq!(retained.snapshot.identity().node, wire.node);
            assert_eq!(retained.request_id, wire.request_id);
            assert_eq!(retained.source, wire.source);
            assert_eq!(retained.quality, wire.quality);
            let cx = retained.snapshot.legacy_context();
            let body = body_of(cx, retained.snapshot.legacy_shape()).into_result().unwrap();
            assert!(Arc::ptr_eq(&body, &retained.engine.kernel().body("part@1").unwrap()));
            check(&body, cx.accuracy()).unwrap();
            let volume = measure(cx, "volume", &body).into_result().unwrap().value;
            assert!((volume - 60.).abs() < 1e-8);
        }
        let reloaded = BrepEngine::from_k3d(&wire.model).unwrap().snapshot("part@1").unwrap();
        let mesh = mesh_of(reloaded.legacy_context(), reloaded.legacy_shape(), 0.01).into_result().unwrap();
        assert!(mesh.open_sides().is_empty());
        assert!((mesh.volume() - 60.).abs() < 1e-8);
    }
}

#[test]
fn later_cancellation_and_deadline_leave_retained_owner_and_cold_snapshot_reusable() {
    for (planar_path, cancelled) in [(false, false), (false, true), (true, false), (true, true)] {
        let clock = Arc::new(FakeClock::default());
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
        let call = call();
        let mut retained = if planar_path {
            build_planar_retained_controlled(call, Arc::clone(&control)).unwrap()
        } else {
            let region = Arc::new(Region::from_polygons(call.input.polygons, LengthUnit::Model).unwrap());
            build_region_retained_controlled(
                region,
                call.input.height,
                call.request_id,
                call.input.source,
                Arc::clone(&control),
            )
            .unwrap()
        };
        assert_eq!(Arc::strong_count(&control), 1, "retained services must release the computation token");
        let snapshot = retained.snapshot.clone();
        let original = retained.engine.to_k3d().unwrap();
        if cancelled {
            control.cancel();
        } else {
            clock.advance(Duration::from_secs(1));
        }
        assert!(control.check().is_err());
        assert!(retained.engine.kernel().context().check_control().is_ok());
        assert!(snapshot.legacy_context().check_control().is_ok());
        assert_eq!(retained.engine.to_k3d().unwrap(), original);
        retained
            .engine
            .apply(
                Statement::new("part", "extrude", Params::new().with("h", 5.), &["outline_profile"]).unwrap(),
            )
            .unwrap();
        let edited = retained.engine.snapshot("part").unwrap();
        let edited_body = body_of(edited.legacy_context(), edited.legacy_shape()).into_result().unwrap();
        let volume = measure(edited.legacy_context(), "volume", &edited_body).into_result().unwrap().value;
        assert!((volume - 100.).abs() < 1e-8);
        assert_ne!(retained.engine.to_k3d().unwrap(), original);
        assert!(retained.engine.owns(&snapshot));
        drop(retained);
        // No mesh conversion was warmed while the caller's token was live.
        std::thread::spawn(move || {
            let cx = snapshot.legacy_context();
            let body = body_of(cx, snapshot.legacy_shape()).into_result().unwrap();
            check(&body, cx.accuracy()).unwrap();
            assert!((measure(cx, "volume", &body).into_result().unwrap().value - 60.).abs() < 1e-8);
            let mesh = mesh_of(cx, snapshot.legacy_shape(), 0.01).into_result().unwrap();
            assert!(mesh.open_sides().is_empty());
            assert!((mesh.volume() - 60.).abs() < 1e-8);
        })
        .join()
        .unwrap();
    }
}
