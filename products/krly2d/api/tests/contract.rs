#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use krly2d_api::{
    metadata, Path, Point2, Region, Segment, API_VERSION, MAX_ABS_ANGLE, MAX_HOLES, MAX_SEGMENTS,
};
use krlyxd::{ApiVersion, Coordinates, LengthUnit};

fn triangle(origin: Point2) -> Vec<Point2> {
    vec![origin, [origin[0] + 4.0, origin[1]], [origin[0], origin[1] + 3.0]]
}

fn line_path(count: usize, closed: bool) -> Path {
    Path::new([5.0, -2.0], vec![Segment::Line { to: [6.0, -1.0] }; count], closed).unwrap()
}

fn arc() -> Segment {
    Segment::EllipticArc {
        radii: [2.5, 4.25],
        x_axis_rotation: -0.75,
        large_arc: true,
        sweep: false,
        to: [19.0, -6.0],
    }
}

#[test]
fn shared_metadata_identifies_the_native_contract_and_cargo_package() {
    let value = metadata();
    value.validate().unwrap();
    assert_eq!(value.package.name, "krly2d-api");
    assert_eq!(value.package.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(API_VERSION, ApiVersion { major: 1, minor: 0 });
    assert_eq!(value.api, API_VERSION);
    assert_eq!(value.protocol, API_VERSION);
    assert_eq!(value.engine_revision, "krly2d.native/1");
    assert_eq!(value.coordinates, Coordinates::RIGHT_HANDED_Z_UP);
    assert_eq!(value.length_unit, LengthUnit::Model);
}

#[test]
fn polygons_preserve_units_order_nonzero_origins_and_vertex_bits() {
    let polygons = vec![
        vec![[10.0, -20.0], [40.0, -20.0], [40.0, 15.0], [10.0, -20.0]],
        triangle([22.0, -5.0]),
        vec![[15.0, -0.0], [15.0, 4.0], [19.0, 0.0]],
    ];
    for unit in [LengthUnit::Model, LengthUnit::Metre] {
        let region = Region::from_polygons(polygons.clone(), unit).unwrap();
        assert_eq!(region.length_unit(), unit);
        assert_eq!(region.outer().start(), polygons[0][0]);
        assert!(region.outer().closed());
        assert_eq!(region.outer().segments().len(), polygons[0].len() - 1);
        assert_eq!(region.holes().len(), 2);
        assert_eq!(region.holes()[0].start(), polygons[1][0]);
        assert_eq!(region.holes()[1].start(), polygons[2][0]);
        let result = region.polygons().unwrap();
        assert_eq!(result, polygons);
        for (actual, supplied) in result.iter().flatten().flatten().zip(polygons.iter().flatten().flatten()) {
            assert_eq!(actual.to_bits(), supplied.to_bits());
        }
    }
}

#[test]
fn large_subnormal_and_underflowed_polygon_coordinates_remain_bit_exact() {
    let tiny = f64::from_bits(1);
    let polygons = vec![
        vec![[47.5e9, -47.5e9], [f64::MAX, -f64::MAX], [tiny, -tiny]],
        vec![[tiny, -0.0], [tiny * 0.25, 0.0], [tiny, -tiny]],
    ];
    for unit in [LengthUnit::Model, LengthUnit::Metre] {
        let region = Region::from_polygons(polygons.clone(), unit).unwrap();
        let result = region.polygons().unwrap();
        assert_eq!(result.iter().map(Vec::len).collect::<Vec<_>>(), vec![3, 3]);
        for (actual, supplied) in result.iter().flatten().flatten().zip(polygons.iter().flatten().flatten()) {
            assert_eq!(actual.to_bits(), supplied.to_bits());
        }
        assert_eq!(region.length_unit(), unit);
    }
}

#[test]
fn finite_curve_coordinates_and_controls_have_no_magnitude_or_degeneracy_threshold() {
    for point in [[f64::MAX, -f64::MAX], [47.5e9, -47.5e9], [f64::from_bits(1), -0.0], [0.0, 0.0]] {
        let segments = vec![
            Segment::Line { to: point },
            Segment::QuadraticBezier { control: point, to: point },
            Segment::CubicBezier { control1: point, control2: point, to: point },
            Segment::EllipticArc {
                radii: [f64::from_bits(1), f64::MAX],
                x_axis_rotation: 0.0,
                large_arc: false,
                sweep: true,
                to: point,
            },
        ];
        let path = Path::new(point, segments.clone(), true).unwrap();
        assert_eq!(path.start(), point);
        assert_eq!(path.segments(), segments);
        assert!(Region::new(path, vec![], LengthUnit::Model).is_ok());
    }
}

#[test]
fn open_paths_and_implicit_closure_retain_exact_endpoint_connectivity() {
    let start = [12.0, -7.0];
    let segments = vec![Segment::Line { to: [16.0, -7.0] }, Segment::Line { to: [12.0, -4.0] }];
    let open = Path::new(start, segments.clone(), false).unwrap();
    assert_eq!(open.start(), start);
    assert_eq!(open.segments(), segments);
    assert!(!open.closed());
    assert!(Region::new(open, vec![], LengthUnit::Model).is_err());
    let closed = Path::new(start, segments, true).unwrap();
    let region = Region::new(closed, vec![], LengthUnit::Metre).unwrap();
    assert_eq!(region.polygons().unwrap(), vec![vec![start, [16.0, -7.0], [12.0, -4.0]]]);
    assert!(Region::new(line_path(2, true), vec![line_path(2, false)], LengthUnit::Model).is_err());
    let empty = Path::new(start, vec![], false).unwrap();
    assert!(empty.segments().is_empty());
    assert_eq!(empty.start(), start);
}

#[test]
fn every_curve_is_retained_and_polygon_conversion_is_explicitly_unsupported() {
    let curves = [
        Segment::QuadraticBezier { control: [15.0, -2.0], to: [18.0, -7.0] },
        Segment::CubicBezier { control1: [15.0, -2.0], control2: [21.0, -9.0], to: [23.0, -7.0] },
        arc(),
    ];
    for curve in curves {
        let path = Path::new([12.0, -7.0], vec![curve], true).unwrap();
        assert_eq!(path.segments(), &[curve]);
        assert_eq!(path.segments()[0].end(), curve.end());
        let region = Region::new(path.clone(), vec![], LengthUnit::Metre).unwrap();
        assert_eq!(region.outer(), &path);
        assert_eq!(region.polygons().unwrap_err().code.as_str(), "unsupported");
        let region = Region::new(line_path(2, true), vec![path], LengthUnit::Model).unwrap();
        assert_eq!(region.holes()[0].segments(), &[curve]);
        assert_eq!(region.polygons().unwrap_err().code.as_str(), "unsupported");
    }
    let path = Path::new([12.0, -7.0], curves.to_vec(), false).unwrap();
    assert_eq!(path.segments(), &curves);
}

#[test]
fn no_topology_repair_deduplication_or_winding_normalization_is_claimed() {
    // Self-intersection, repetition, collinearity and holes outside the outer
    // loop are intentionally left to existing geometry providers.
    let polygons = vec![
        vec![[1.0, 1.0], [3.0, 3.0], [1.0, 3.0], [3.0, 1.0]],
        vec![[20.0, 20.0]; 3],
        vec![[40.0, 40.0], [41.0, 41.0], [42.0, 42.0]],
    ];
    assert_eq!(
        Region::from_polygons(polygons.clone(), LengthUnit::Model).unwrap().polygons().unwrap(),
        polygons
    );
}

#[test]
fn missing_or_short_linear_loops_are_rejected() {
    assert!(Region::from_polygons(vec![], LengthUnit::Model).is_err());
    for polygon in [vec![], vec![[0.0, 0.0]], vec![[0.0, 0.0], [1.0, 1.0]]] {
        assert!(Region::from_polygons(vec![polygon.clone()], LengthUnit::Model).is_err());
        assert!(Region::from_polygons(vec![triangle([5.0, 6.0]), polygon], LengthUnit::Model).is_err());
    }
    for count in 0..2 {
        assert!(Path::new([0.0, 0.0], vec![Segment::Line { to: [1.0, 1.0] }; count], true).is_err());
    }
}

#[test]
fn all_coordinate_roles_reject_nonfinite_values() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for invalid in [[bad, 0.0], [0.0, bad]] {
            assert!(Path::new(invalid, vec![], false).is_err());
            let segments = [
                Segment::Line { to: invalid },
                Segment::QuadraticBezier { control: invalid, to: [1.0, 1.0] },
                Segment::QuadraticBezier { control: [1.0, 1.0], to: invalid },
                Segment::CubicBezier { control1: invalid, control2: [2.0, 2.0], to: [1.0, 1.0] },
                Segment::CubicBezier { control1: [2.0, 2.0], control2: invalid, to: [1.0, 1.0] },
                Segment::CubicBezier { control1: [2.0, 2.0], control2: [1.0, 2.0], to: invalid },
                Segment::EllipticArc {
                    radii: [1.0, 2.0],
                    x_axis_rotation: 0.0,
                    large_arc: false,
                    sweep: true,
                    to: invalid,
                },
            ];
            for segment in segments {
                assert!(Path::new([0.0, 0.0], vec![segment], false).is_err());
            }
            for index in 0..3 {
                let mut polygon = triangle([5.0, 6.0]);
                polygon[index] = invalid;
                assert!(Region::from_polygons(vec![polygon], LengthUnit::Model).is_err());
            }
        }
    }
    assert!(Path::new([f64::MAX, -f64::MAX], vec![], false).is_ok());
}

#[test]
fn arc_radii_are_positive_finite_and_rotations_bounded_without_normalization() {
    for bad in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for radii in [[bad, 1.0], [1.0, bad]] {
            let segment = Segment::EllipticArc {
                radii,
                x_axis_rotation: 0.0,
                large_arc: false,
                sweep: true,
                to: [1.0, 1.0],
            };
            assert!(Path::new([0.0, 0.0], vec![segment], false).is_err());
        }
    }
    for angle in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, MAX_ABS_ANGLE + 1.0, -MAX_ABS_ANGLE - 1.0] {
        let segment = Segment::EllipticArc {
            radii: [1.0, 2.0],
            x_axis_rotation: angle,
            large_arc: false,
            sweep: false,
            to: [1.0, 1.0],
        };
        assert!(Path::new([0.0, 0.0], vec![segment], false).is_err());
    }
    for angle in [-MAX_ABS_ANGLE, -0.0, MAX_ABS_ANGLE] {
        let segment = Segment::EllipticArc {
            radii: [f64::from_bits(1), f64::MAX],
            x_axis_rotation: angle,
            large_arc: false,
            sweep: true,
            to: [0.0, 0.0],
        };
        let path = Path::new([0.0, 0.0], vec![segment], true).unwrap();
        assert_eq!(path.segments(), &[segment]);
        if let Segment::EllipticArc { x_axis_rotation, .. } = path.segments()[0] {
            assert_eq!(x_axis_rotation.to_bits(), angle.to_bits());
        }
    }
}

#[test]
fn total_segments_are_bounded_across_paths_and_polygon_conversions() {
    assert!(Path::new([0.0, 0.0], vec![Segment::Line { to: [1.0, 1.0] }; MAX_SEGMENTS + 1], false).is_err());
    assert!(Region::new(line_path(MAX_SEGMENTS, true), vec![], LengthUnit::Model).is_ok());
    assert!(
        Region::new(line_path(MAX_SEGMENTS - 2, true), vec![line_path(2, true)], LengthUnit::Model).is_ok()
    );
    let error = Region::new(line_path(MAX_SEGMENTS - 1, true), vec![line_path(2, true)], LengthUnit::Model)
        .unwrap_err();
    assert_eq!(error.code.as_str(), "resource_limit");
    let polygons = vec![vec![[0.0, 0.0]; MAX_SEGMENTS - 1], triangle([5.0, 6.0])];
    let region = Region::from_polygons(polygons.clone(), LengthUnit::Model).unwrap();
    assert_eq!(region.polygons().unwrap(), polygons);
    let error =
        Region::from_polygons(vec![vec![[0.0, 0.0]; MAX_SEGMENTS], triangle([5.0, 6.0])], LengthUnit::Model)
            .unwrap_err();
    assert_eq!(error.code.as_str(), "resource_limit");
    assert!(Region::from_polygons(vec![vec![[0.0, 0.0]; MAX_SEGMENTS + 2]], LengthUnit::Model).is_err());
}

#[test]
fn hole_counts_are_bounded_before_converting_polygon_contents() {
    let holes = vec![line_path(2, true); MAX_HOLES];
    let region = Region::new(line_path(2, true), holes, LengthUnit::Model).unwrap();
    assert_eq!(region.holes().len(), MAX_HOLES);
    assert!(
        Region::new(line_path(2, true), vec![line_path(2, true); MAX_HOLES + 1], LengthUnit::Model).is_err()
    );
    let region = Region::from_polygons(vec![triangle([5.0, 6.0]); MAX_HOLES + 1], LengthUnit::Model).unwrap();
    assert_eq!(region.holes().len(), MAX_HOLES);
    let error = Region::from_polygons(vec![vec![]; MAX_HOLES + 2], LengthUnit::Model).unwrap_err();
    assert_eq!(error.code.as_str(), "resource_limit");
}

#[test]
fn arc_shared_regions_own_their_data_and_expose_only_independent_mutable_copies() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Region>();
    let shared = {
        let polygons = vec![triangle([31.0, -17.0]), triangle([32.0, -16.0])];
        Arc::new(Region::from_polygons(polygons, LengthUnit::Metre).unwrap())
    };
    let weak = Arc::downgrade(&shared);
    let retained = Arc::clone(&shared);
    drop(shared);
    assert!(weak.upgrade().is_some());
    let returned = std::thread::spawn(move || {
        let mut polygons = retained.polygons().unwrap();
        polygons[0][0] = [999.0, 999.0];
        polygons.reverse();
        let mut segments = retained.outer().segments().to_vec();
        segments[0] = Segment::Line { to: [-999.0, -999.0] };
        assert_eq!(retained.outer().start(), [31.0, -17.0]);
        assert_eq!(retained.outer().segments()[0].end(), [35.0, -17.0]);
        assert_eq!(retained.holes()[0].start(), [32.0, -16.0]);
        assert_eq!(retained.length_unit(), LengthUnit::Metre);
        retained
    })
    .join()
    .unwrap();
    assert_eq!(returned.polygons().unwrap()[0], triangle([31.0, -17.0]));
    drop(returned);
    assert!(weak.upgrade().is_none());
}
