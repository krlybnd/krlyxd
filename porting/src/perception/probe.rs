//! Probes: exact, on-demand questions to the geometry.
//! A ray turns 3D into a 1D sentence: what the ray meets, in order.

use crate::brep::{BrepEnvelope, GeometryError, Surface, SurfaceGeometry};
use crate::math::{add, dot, norm, scale, sub, Interval, Point3};
use crate::query::face_contains_uv;
use crate::query::ray_roots::support_roots;

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub t: f64,
    pub face: String,
    /// true: the ray enters material here; false: it leaves.
    pub entering: bool,
    pub point: Point3,
}

pub fn unit(v: Point3) -> Option<Point3> {
    let n = norm(v);
    (n > 1e-12).then(|| scale(v, 1.0 / n))
}

/// Crossings of one face with the ray (appended to `hits`, unsorted).
pub(crate) fn hit_face(
    brep: &BrepEnvelope,
    face: &crate::brep::Face,
    origin: Point3,
    dir: Point3,
    domain: Interval,
    hits: &mut Vec<Hit>,
) -> Result<(), GeometryError> {
    let tol = brep.accuracy.intersection;
    let surface = brep.geometry.surface(face.surface)?;
    let Some(roots) = support_roots(surface, origin, dir, domain, tol)? else {
        return Ok(()); // ray lies in the surface: grazing, not a crossing
    };
    for t in roots {
        if t < -tol {
            continue;
        }
        let p = add(origin, scale(dir, t));
        let Ok(uv) = surface.project(p, None) else { continue };
        if norm(sub(surface.point_at(uv)?, p)) > tol {
            continue;
        }
        // None = on the trim boundary (a seam or an edge): still a crossing
        if face_contains_uv(brep, face, uv)? == Some(false) {
            continue;
        }
        let n = scale(surface.normal_at(uv)?, face.sense.multiplier());
        let d = dot(n, dir);
        if d.abs() <= 1e-12 {
            continue;
        }
        hits.push(Hit { t, face: face.key.clone(), entering: d < 0.0, point: p });
    }
    Ok(())
}

/// All crossings of the ray `origin + t·dir (t ≥ 0)` with the body, sorted by t.
pub fn ray(brep: &BrepEnvelope, origin: Point3, dir: Point3) -> Result<Vec<Hit>, GeometryError> {
    use crate::kernel::probe::{Exhaustive, Probe};
    Exhaustive.ray(brep, origin, dir)
}

/// The ray as a sentence: `air 0.5 > [top] solid 2 > [bottom] air`.
pub fn sentence(hits: &[Hit], start_inside: bool) -> String {
    let mut out = String::new();
    let mut inside = start_inside;
    let mut last = 0.0;
    for h in hits {
        out.push_str(&format!(
            "{} {} > [{}] ",
            if inside { "solid" } else { "air" },
            num(h.t - last),
            h.face
        ));
        inside = h.entering;
        last = h.t;
    }
    out.push_str(if inside { "solid" } else { "air" });
    out
}

/// A point on the face and its outward normal there.
pub fn face_anchor(brep: &BrepEnvelope, face: u32) -> Result<(Point3, Point3), GeometryError> {
    let f = brep
        .topology
        .faces
        .get(face as usize)
        .ok_or_else(|| GeometryError::InvalidTopology("face is missing".into()))?;
    let surface = brep.geometry.surface(f.surface)?;
    let mut sum = [0.0; 3];
    let mut count = 0.0;
    for half in &brep.topology.halfedges {
        if half.face == Some(face) {
            let p = brep.topology.vertices[half.from as usize].position;
            sum = add(sum, p);
            count += 1.0;
        }
    }
    let guess = if count > 0.0 { scale(sum, 1.0 / count) } else { surface.frame().origin };
    let uv = surface.project(guess, None)?;
    let mut uv_try = uv;
    // the vertex average can fall outside a non-convex face: nudge toward a vertex
    if face_contains_uv(brep, f, uv)? != Some(true) {
        for half in brep.topology.halfedges.iter().filter(|h| h.face == Some(face)) {
            let v = brep.topology.vertices[half.from as usize].position;
            let p = add(scale(v, 0.9), scale(guess, 0.1));
            if let Ok(c) = surface.project(p, None) {
                if face_contains_uv(brep, f, c)? == Some(true) {
                    uv_try = c;
                    break;
                }
            }
        }
    }
    let point = surface.point_at(uv_try)?;
    let normal = scale(surface.normal_at(uv_try)?, f.sense.multiplier());
    Ok((point, normal))
}

/// Short, exact description of the face's carrying surface.
pub fn surface_text(s: &SurfaceGeometry) -> String {
    match s {
        SurfaceGeometry::Plane { frame } => format!("plane n={}", axis(frame.z)),
        SurfaceGeometry::Cylinder { frame, radius } => format!(
            "cylinder r={} axis={} through {}",
            num(*radius),
            axis(frame.z),
            pt(frame.origin)
        ),
        SurfaceGeometry::Sphere { frame, radius } => {
            format!("sphere r={} center {}", num(*radius), pt(frame.origin))
        }
        SurfaceGeometry::Cone { frame, semi_angle } => format!(
            "cone half-angle={}° axis={} apex {}",
            num(semi_angle.to_degrees()),
            axis(frame.z),
            pt(frame.origin)
        ),
        SurfaceGeometry::Torus { frame, major_radius, minor_radius } => format!(
            "torus R={} r={} axis={}",
            num(*major_radius),
            num(*minor_radius),
            axis(frame.z)
        ),
    }
}

pub fn kind(s: &SurfaceGeometry) -> &'static str {
    match s {
        SurfaceGeometry::Plane { .. } => "plane",
        SurfaceGeometry::Cylinder { .. } => "cylinder",
        SurfaceGeometry::Sphere { .. } => "sphere",
        SurfaceGeometry::Cone { .. } => "cone",
        SurfaceGeometry::Torus { .. } => "torus",
    }
}

/// `+z`, `-x`, or the rounded vector if not axis-aligned.
pub fn axis(v: Point3) -> String {
    let names = ["x", "y", "z"];
    for i in 0..3 {
        if (v[i].abs() - 1.0).abs() <= 1e-9 {
            return format!("{}{}", if v[i] > 0.0 { "+" } else { "-" }, names[i]);
        }
    }
    pt(v)
}

pub fn pt(p: Point3) -> String {
    format!("({},{},{})", num(p[0]), num(p[1]), num(p[2]))
}

pub fn num(x: f64) -> String {
    let r = (x * 1e4).round() / 1e4;
    let s = format!("{r}");
    if s == "-0" { "0".into() } else { s }
}

/// Angle between the outward normals across an edge: convex (outside corner)
/// or concave (inside corner), with the dihedral angle in degrees.
pub fn edge_character(n_a: Point3, n_b: Point3, toward_b: Point3) -> (bool, f64) {
    let angle = dot(n_a, n_b).clamp(-1.0, 1.0).acos().to_degrees();
    if angle <= 1e-6 {
        return (true, 180.0); // tangent (smooth) transition
    }
    // b's material lies on the side of toward_b; convex if n_a points away from b's face
    let convex = dot(n_a, toward_b) <= 1e-12;
    (convex, 180.0 - angle)
}

/// Outward normal of a face at (the projection of) a given point.
pub fn normal_near(brep: &BrepEnvelope, face: u32, p: Point3) -> Result<Point3, GeometryError> {
    let f = brep
        .topology
        .faces
        .get(face as usize)
        .ok_or_else(|| GeometryError::InvalidTopology("face is missing".into()))?;
    let surface = brep.geometry.surface(f.surface)?;
    let uv = surface.project(p, None)?;
    Ok(scale(surface.normal_at(uv)?, f.sense.multiplier()))
}
