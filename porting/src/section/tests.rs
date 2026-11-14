use super::{PairRule, Rules, Section};
use crate::brep::{Frame3, SurfaceGeometry};
use crate::math::{cross, dot, norm, sub, Point3};

/// Signed distance of a point from the surface: the residual every point of a
/// section must make vanish on both surfaces.
fn residual(s: &SurfaceGeometry, p: Point3) -> f64 {
    match s {
        SurfaceGeometry::Plane { frame } => dot(sub(p, frame.origin), frame.z),
        SurfaceGeometry::Sphere { frame, radius } => norm(sub(p, frame.origin)) - radius,
        SurfaceGeometry::Cylinder { frame, radius } => norm(cross(sub(p, frame.origin), frame.z)) - radius,
        _ => f64::NAN,
    }
}

fn frame(origin: Point3, z: Point3) -> Frame3 {
    let z = { let n = norm(z); [z[0] / n, z[1] / n, z[2] / n] };
    let (x, y) = super::basis(z);
    Frame3 { origin, x, y, z }
}
fn plane(o: Point3, n: Point3) -> SurfaceGeometry { SurfaceGeometry::Plane { frame: frame(o, n) } }
fn sphere(c: Point3, r: f64) -> SurfaceGeometry { SurfaceGeometry::Sphere { frame: frame(c, [0.0, 0.0, 1.0]), radius: r } }
fn cylinder(o: Point3, axis: Point3, r: f64) -> SurfaceGeometry { SurfaceGeometry::Cylinder { frame: frame(o, axis), radius: r } }

/// The section lies on both surfaces to 1e-11, in either argument order.
fn on_both(a: &SurfaceGeometry, b: &SurfaceGeometry) -> Section {
    let rules = Rules::standard();
    let found = rules.section(a, b, 1e-9);
    let points = found.points(64);
    for p in &points {
        assert!(residual(a, *p).abs() < 1e-11 && residual(b, *p).abs() < 1e-11, "{found:?} at {p:?}: {} {}", residual(a, *p), residual(b, *p));
    }
    let swapped = rules.section(b, a, 1e-9).points(64);
    assert_eq!(points.len(), swapped.len(), "argument order changed the section");
    found
}

#[test]
fn planes_meet_in_a_line_or_not_at_all() {
    let a = plane([1.0, 2.0, 3.0], [0.3, -0.5, 1.0]);
    assert!(matches!(on_both(&a, &plane([-2.0, 0.5, 1.0], [1.0, 0.2, -0.4])), Section::Lines(l) if l.len() == 1));
    assert_eq!(on_both(&a, &plane([0.0, 0.0, 9.0], [0.3, -0.5, 1.0])), Section::Empty);
    assert_eq!(on_both(&a, &plane([1.0, 2.0, 3.0], [-0.3, 0.5, -1.0])), Section::Coincident);
}

#[test]
fn a_plane_cuts_a_sphere_in_a_circle_of_the_pythagorean_radius() {
    let s = sphere([1.0, -1.0, 2.0], 5.0);
    let Section::Circles(c) = on_both(&plane([1.0, -1.0, 5.0], [0.0, 0.0, 1.0]), &s) else { panic!("circle expected") };
    assert!((c[0].radius - 4.0).abs() < 1e-12);
    assert!(matches!(on_both(&plane([0.0, 0.0, 7.0], [0.0, 0.0, 1.0]), &s), Section::Point(_)));
    assert_eq!(on_both(&plane([0.0, 0.0, 7.5], [0.0, 0.0, 1.0]), &s), Section::Empty);
    assert!(matches!(on_both(&plane([0.4, 0.1, 1.0], [1.0, 2.0, -0.7]), &s), Section::Circles(_)));
}

#[test]
fn a_plane_cuts_a_cylinder_in_a_circle_an_ellipse_or_rulings() {
    let c = cylinder([1.0, 1.0, 0.0], [0.0, 0.0, 1.0], 2.0);
    assert!(matches!(on_both(&plane([0.0, 0.0, 3.0], [0.0, 0.0, 1.0]), &c), Section::Circles(_)));
    let Section::Ellipse { semi_major, semi_minor, .. } = on_both(&plane([0.0, 0.0, 3.0], [1.0, 0.0, 1.0]), &c) else { panic!("ellipse expected") };
    assert!((semi_minor - 2.0).abs() < 1e-12 && (semi_major - 2.0 * 2f64.sqrt()).abs() < 1e-12);
    assert!(matches!(on_both(&plane([2.0, 0.0, 0.0], [1.0, 0.0, 0.0]), &c), Section::Lines(l) if l.len() == 2));
    assert!(matches!(on_both(&plane([3.0, 0.0, 0.0], [1.0, 0.0, 0.0]), &c), Section::Lines(l) if l.len() == 1));
    assert_eq!(on_both(&plane([3.5, 0.0, 0.0], [1.0, 0.0, 0.0]), &c), Section::Empty);
    // tilted cylinder, arbitrary plane
    let tilted = cylinder([0.3, -2.0, 1.0], [0.2, 1.0, -0.5], 1.3);
    assert!(matches!(on_both(&plane([0.0, 0.5, 0.2], [0.6, 0.1, 1.0]), &tilted), Section::Ellipse { .. }));
}

#[test]
fn spheres_and_parallel_cylinders_share_the_lens_formula() {
    let Section::Circles(c) = on_both(&sphere([0.0, 0.0, 0.0], 5.0), &sphere([6.0, 0.0, 0.0], 5.0)) else { panic!("circle expected") };
    assert!((c[0].radius - 4.0).abs() < 1e-12 && (c[0].center[0] - 3.0).abs() < 1e-12);
    assert!(matches!(on_both(&sphere([0.0, 0.0, 0.0], 2.0), &sphere([5.0, 0.0, 0.0], 3.0)), Section::Point(_)));
    assert_eq!(on_both(&sphere([0.0, 0.0, 0.0], 2.0), &sphere([9.0, 0.0, 0.0], 3.0)), Section::Empty);
    let a = cylinder([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.5);
    // the column's flute: radius 0.046 at distance 0.506 from the shaft axis
    let Section::Lines(l) = on_both(&a, &cylinder([0.506, 3.0, 0.0], [0.0, 1.0, 0.0], 0.046)) else { panic!("two rulings expected") };
    assert_eq!(l.len(), 2);
    let cos_alpha = (0.25 + 0.506 * 0.506 - 0.046 * 0.046) / (2.0 * 0.5 * 0.506);
    assert!((l[0].point[0] - 0.5 * cos_alpha).abs() < 1e-12, "the flute corner agrees with the profile formula");
}

#[test]
fn a_sphere_on_the_axis_cuts_a_cylinder_in_two_circles() {
    let c = cylinder([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 3.0);
    assert!(matches!(on_both(&sphere([1.0, 4.0, 0.0], 5.0), &c), Section::Circles(k) if k.len() == 2));
    assert!(matches!(on_both(&sphere([1.0, 4.0, 0.0], 3.0), &c), Section::Circles(k) if k.len() == 1));
    assert_eq!(on_both(&sphere([1.0, 4.0, 0.0], 2.0), &c), Section::Empty);
}

#[test]
fn uncovered_pairs_say_so_and_the_registry_is_open() {
    let off_axis = Rules::standard().section(&sphere([0.7, 0.0, 0.0], 2.0), &cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0), 1e-9);
    assert_eq!(off_axis, Section::NoClosedForm);
    struct Everything;
    impl PairRule for Everything {
        fn name(&self) -> &'static str { "test rule" }
        fn section(&self, _: &SurfaceGeometry, _: &SurfaceGeometry, _: f64) -> Option<Section> { Some(Section::Empty) }
    }
    let extended = Rules::standard().with(Everything);
    assert_eq!(extended.names().len(), 7);
    assert_eq!(extended.section(&sphere([0.7, 0.0, 0.0], 2.0), &cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0), 1e-9), Section::Empty);
    assert_eq!(Rules::empty().section(&plane([0.0; 3], [0.0, 0.0, 1.0]), &plane([0.0; 3], [1.0, 0.0, 0.0]), 1e-9), Section::NoClosedForm);
}
