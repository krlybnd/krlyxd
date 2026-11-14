use super::{Circle, Line, PairRule, Section};
use crate::brep::SurfaceGeometry;
use crate::math::{add, cross, dot, norm, scale, sub, Point3};

const PARALLEL: f64 = 1e-12;

fn unit(v: Point3) -> Point3 {
    scale(v, 1.0 / norm(v))
}

/// Two circles in a common plane, centres `d` apart: where they meet, as
/// (distance along the centre line, half chord). The same quadratic serves
/// sphere–sphere and parallel cylinders.
enum Lens {
    Apart,
    Same,
    Touch(f64),
    Cross(f64, f64),
}

fn lens(d: f64, r1: f64, r2: f64, tol: f64) -> Lens {
    if d <= tol {
        return if (r1 - r2).abs() <= tol { Lens::Same } else { Lens::Apart };
    }
    if d > r1 + r2 + tol || d < (r1 - r2).abs() - tol {
        return Lens::Apart;
    }
    let x = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
    let h2 = r1 * r1 - x * x;
    if h2 <= tol * tol.max(r1 * 1e-12) {
        Lens::Touch(x)
    } else {
        Lens::Cross(x, h2.sqrt())
    }
}

/// n₁×n₂ is the line's direction; the point is where the line through o₁
/// along (d×n₁) meets plane 2.
pub struct PlanePlane;
impl PairRule for PlanePlane {
    fn name(&self) -> &'static str {
        "plane-plane"
    }
    fn section(&self, a: &SurfaceGeometry, b: &SurfaceGeometry, tol: f64) -> Option<Section> {
        let (SurfaceGeometry::Plane { frame: f }, SurfaceGeometry::Plane { frame: g }) = (a, b) else { return None };
        let d = cross(f.z, g.z);
        if norm(d) <= PARALLEL {
            let apart = dot(sub(g.origin, f.origin), f.z).abs();
            return Some(if apart <= tol { Section::Coincident } else { Section::Empty });
        }
        let v = cross(d, f.z);
        let t = dot(sub(g.origin, f.origin), g.z) / dot(v, g.z);
        Some(Section::Lines(vec![Line { point: add(f.origin, scale(v, t)), direction: unit(d) }]))
    }
}

/// The centre's signed height h over the plane decides: circle of radius √(r²−h²).
pub struct PlaneSphere;
impl PairRule for PlaneSphere {
    fn name(&self) -> &'static str {
        "plane-sphere"
    }
    fn section(&self, a: &SurfaceGeometry, b: &SurfaceGeometry, tol: f64) -> Option<Section> {
        let (SurfaceGeometry::Plane { frame: f }, SurfaceGeometry::Sphere { frame: g, radius }) = (a, b) else { return None };
        let h = dot(sub(g.origin, f.origin), f.z);
        let foot = sub(g.origin, scale(f.z, h));
        Some(if h.abs() > radius + tol {
            Section::Empty
        } else if (h.abs() - radius).abs() <= tol {
            Section::Point(foot)
        } else {
            Section::Circles(vec![Circle { center: foot, normal: f.z, radius: (radius * radius - h * h).sqrt() }])
        })
    }
}

/// With c = |n·a|: c = 1 a circle, 0 < c < 1 an ellipse (semi-axes r/c and r),
/// c = 0 up to two rulings at distance √(r²−h²) from the axis' foot.
pub struct PlaneCylinder;
impl PairRule for PlaneCylinder {
    fn name(&self) -> &'static str {
        "plane-cylinder"
    }
    fn section(&self, a: &SurfaceGeometry, b: &SurfaceGeometry, tol: f64) -> Option<Section> {
        let (SurfaceGeometry::Plane { frame: f }, SurfaceGeometry::Cylinder { frame: g, radius }) = (a, b) else { return None };
        let (n, axis, r) = (f.z, g.z, *radius);
        let c = dot(n, axis);
        if c.abs() <= PARALLEL {
            let h = dot(sub(g.origin, f.origin), n);
            if h.abs() > r + tol {
                return Some(Section::Empty);
            }
            let foot = sub(g.origin, scale(n, h));
            let across = unit(cross(n, axis));
            let half = (r * r - h * h).max(0.0).sqrt();
            let lines = if half <= tol {
                vec![Line { point: foot, direction: axis }]
            } else {
                vec![
                    Line { point: add(foot, scale(across, half)), direction: axis },
                    Line { point: sub(foot, scale(across, half)), direction: axis },
                ]
            };
            return Some(Section::Lines(lines));
        }
        let t = dot(sub(f.origin, g.origin), n) / c;
        let center = add(g.origin, scale(axis, t));
        if (c.abs() - 1.0).abs() <= PARALLEL {
            return Some(Section::Circles(vec![Circle { center, normal: n, radius: r }]));
        }
        Some(Section::Ellipse {
            center,
            normal: n,
            major: unit(sub(axis, scale(n, c))),
            semi_major: r / c.abs(),
            semi_minor: r,
        })
    }
}

pub struct SphereSphere;
impl PairRule for SphereSphere {
    fn name(&self) -> &'static str {
        "sphere-sphere"
    }
    fn section(&self, a: &SurfaceGeometry, b: &SurfaceGeometry, tol: f64) -> Option<Section> {
        let (SurfaceGeometry::Sphere { frame: f, radius: r1 }, SurfaceGeometry::Sphere { frame: g, radius: r2 }) = (a, b) else { return None };
        let delta = sub(g.origin, f.origin);
        let d = norm(delta);
        Some(match lens(d, *r1, *r2, tol) {
            Lens::Apart => Section::Empty,
            Lens::Same => Section::Coincident,
            Lens::Touch(x) => Section::Point(add(f.origin, scale(delta, x / d))),
            Lens::Cross(x, h) => {
                Section::Circles(vec![Circle { center: add(f.origin, scale(delta, x / d)), normal: unit(delta), radius: h }])
            }
        })
    }
}

/// Sphere centred on the cylinder's axis: circles at ±√(R²−r²) along the axis.
pub struct SphereCylinderCoaxial;
impl PairRule for SphereCylinderCoaxial {
    fn name(&self) -> &'static str {
        "sphere-cylinder (centre on axis)"
    }
    fn section(&self, a: &SurfaceGeometry, b: &SurfaceGeometry, tol: f64) -> Option<Section> {
        let (SurfaceGeometry::Sphere { frame: f, radius: big }, SurfaceGeometry::Cylinder { frame: g, radius: r }) = (a, b) else { return None };
        if norm(cross(sub(f.origin, g.origin), g.z)) > tol {
            return None;
        }
        Some(if *big < r - tol {
            Section::Empty
        } else if (big - r).abs() <= tol {
            Section::Circles(vec![Circle { center: f.origin, normal: g.z, radius: *r }])
        } else {
            let up = (big * big - r * r).sqrt();
            Section::Circles(vec![
                Circle { center: add(f.origin, scale(g.z, up)), normal: g.z, radius: *r },
                Circle { center: sub(f.origin, scale(g.z, up)), normal: g.z, radius: *r },
            ])
        })
    }
}

/// Parallel axes: the cross-sections are two circles; their common points
/// extend to rulings.
pub struct CylinderCylinderParallel;
impl PairRule for CylinderCylinderParallel {
    fn name(&self) -> &'static str {
        "cylinder-cylinder (parallel axes)"
    }
    fn section(&self, a: &SurfaceGeometry, b: &SurfaceGeometry, tol: f64) -> Option<Section> {
        let (SurfaceGeometry::Cylinder { frame: f, radius: r1 }, SurfaceGeometry::Cylinder { frame: g, radius: r2 }) = (a, b) else { return None };
        if norm(cross(f.z, g.z)) > PARALLEL {
            return None;
        }
        let between = sub(g.origin, f.origin);
        let delta = sub(between, scale(f.z, dot(between, f.z)));
        let d = norm(delta);
        Some(match lens(d, *r1, *r2, tol) {
            Lens::Apart => Section::Empty,
            Lens::Same => Section::Coincident,
            Lens::Touch(x) => Section::Lines(vec![Line { point: add(f.origin, scale(delta, x / d)), direction: f.z }]),
            Lens::Cross(x, h) => {
                let along = scale(delta, 1.0 / d);
                let across = cross(f.z, along);
                let mid = add(f.origin, scale(along, x));
                Section::Lines(vec![
                    Line { point: add(mid, scale(across, h)), direction: f.z },
                    Line { point: sub(mid, scale(across, h)), direction: f.z },
                ])
            }
        })
    }
}
