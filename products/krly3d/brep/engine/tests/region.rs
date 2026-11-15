//! The shared 2D contract is a real native input, independent of raster evidence.
#![allow(clippy::unwrap_used)]

use krly2d_api::{Path, Region, Segment};
use krly3d_brep::planar::{PlanarCall, PlanarInput, PROTOCOL};
use krly3d_brep_engine::{build_planar, build_region_retained};
use krly3d_kernel::ports::interpret::mesh_of;
use krlyxd::LengthUnit;
use std::sync::Arc;

fn loops() -> Vec<Vec<[f64; 2]>> {
    vec![
        vec![[10., -4.], [16., -4.], [16., 0.], [10., 0.]],
        vec![[11., -3.], [11., -1.], [13., -1.], [13., -3.]],
    ]
}

#[test]
fn native_profile_and_legacy_wire_produce_identical_editable_geometry() {
    let region = Arc::new(Region::from_polygons(loops(), LengthUnit::Model).unwrap());
    let native =
        build_region_retained(Arc::clone(&region), 3., "native".into(), "vector:plate".into()).unwrap();
    assert!(Arc::ptr_eq(&region, &native.region));
    let wire = build_planar(PlanarCall {
        protocol: PROTOCOL.into(),
        request_id: "native".into(),
        input: PlanarInput {
            polygons: loops(),
            height: 3.,
            length_unit: "model".into(),
            source: "vector:plate".into(),
        },
    })
    .unwrap();
    assert_eq!(native.engine.to_k3d().unwrap(), wire.model);
    assert_eq!(native.quality, wire.quality);
    assert_eq!(native.snapshot.identity().build_key, wire.provenance.build_key);
    let snapshot = native.snapshot.clone();
    drop(native);
    drop(region);
    let mesh = mesh_of(snapshot.legacy_context(), snapshot.legacy_shape(), 0.01).into_result().unwrap();
    assert_eq!(mesh.volume(), 60.);
    assert!(mesh.open_sides().is_empty());
}

#[test]
fn structural_region_acceptance_cannot_bypass_brep_topology_or_units() {
    let crossing = vec![vec![[0., 0.], [4., 4.], [0., 4.], [4., 0.]]];
    let region = Arc::new(Region::from_polygons(crossing, LengthUnit::Model).unwrap());
    assert!(build_region_retained(region, 2., "crossing".into(), "supplied".into()).is_err());

    let metres = Arc::new(Region::from_polygons(loops(), LengthUnit::Metre).unwrap());
    let error = build_region_retained(metres, 2., "metres".into(), "supplied".into()).err().unwrap();
    assert_eq!(error.code.as_str(), "unsupported");
    let region = Arc::new(Region::from_polygons(loops(), LengthUnit::Model).unwrap());
    for height in [0., -1., f64::NAN, f64::INFINITY, 1e10] {
        assert!(build_region_retained(Arc::clone(&region), height, "bad".into(), "supplied".into()).is_err());
    }
    assert!(build_region_retained(region, 2., String::new(), "supplied".into()).is_err());
}

#[test]
fn curved_boundaries_are_rejected_instead_of_extruding_their_chords() {
    let path = Path::new(
        [0., 0.],
        vec![Segment::QuadraticBezier { control: [3., 5.], to: [6., 0.] }, Segment::Line { to: [3., -2.] }],
        true,
    )
    .unwrap();
    let region = Arc::new(Region::new(path, vec![], LengthUnit::Model).unwrap());
    let error =
        build_region_retained(Arc::clone(&region), 2., "curve".into(), "vector".into()).err().unwrap();
    assert_eq!(error.code.as_str(), "unsupported");
    assert!(matches!(region.outer().segments()[0], Segment::QuadraticBezier { .. }));
}
