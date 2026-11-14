//! Corinthian column on krly3d, second take: equations and symmetry instead of brute force. Proportions after Vitruvius, lower shaft diameter D = 1.
//! Exact (analytic) parts: plinth, Attic base, fluted shaft, astragal, bell, abacus, fleurons.
//! Approximated (triangle polyhedra): acanthus leaves, caulicoli, volutes, helices.

use krly3d::perception;
use krly3d::primitives::{ProfileSegment, SegmentArc};
use krly3d::kernel::probe::{BoxTree, Exhaustive, Probe};
use krly3d::kernel::Kernel;
use krly3d::perception::KeyLayer;
use krly3d::world_graph::{
    CopyOptions, StepOptions, Transform, CreateOptions, CreatingOperation, EditScope, ModifyingOperation, Plane, Primitive, WorldGraph,
};
use serde_json::json;
use std::f64::consts::{PI, TAU};

type P3 = [f64; 3];

fn at(p: P3) -> Option<Plane> {
    Some(Plane { origin: Some(p), normal: Some([0.0, 1.0, 0.0]), x_direction: Some([1.0, 0.0, 0.0]) })
}
/// The profile plane is the vertical XY plane (profile y = world Y), so revolving
/// about the profile's y axis gives a column standing on the ground.
fn upright() -> Option<Plane> {
    Some(Plane { origin: Some([0.0, 0.0, 0.0]), normal: Some([0.0, 0.0, 1.0]), x_direction: Some([1.0, 0.0, 0.0]) })
}
fn opts(id: &str, plane: Option<Plane>) -> CreateOptions {
    CreateOptions { node_id: Some(id.into()), plane, ..Default::default() }
}
fn line(to: [f64; 2]) -> ProfileSegment {
    ProfileSegment { to, arc: None }
}
/// Arc from `from` to `to` with radius rho; `bulge_out` picks the side the arc bows to (+x side of travel).
fn arc(from: [f64; 2], to: [f64; 2], rho: f64, bulge_left: bool) -> ProfileSegment {
    let m = [(from[0] + to[0]) / 2.0, (from[1] + to[1]) / 2.0];
    let d = [to[0] - from[0], to[1] - from[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    let h = (rho * rho - len * len / 4.0).max(0.0).sqrt();
    let left = [-d[1] / len, d[0] / len];
    // centre on the side opposite to the bulge
    let s = if bulge_left { -1.0 } else { 1.0 };
    let c = [m[0] + s * h * left[0], m[1] + s * h * left[1]];
    let a = [from[0] - c[0], from[1] - c[1]];
    let b = [to[0] - c[0], to[1] - c[1]];
    let ccw = a[0] * b[1] - a[1] * b[0] > 0.0;
    ProfileSegment { to, arc: Some(SegmentArc { center: c, counter_clockwise: ccw }) }
}
fn half_round(from: [f64; 2], to: [f64; 2], outward: bool) -> ProfileSegment {
    let c = [(from[0] + to[0]) / 2.0, (from[1] + to[1]) / 2.0];
    // going up the right-hand side: bulging out (+x) is counter-clockwise
    ProfileSegment { to, arc: Some(SegmentArc { center: c, counter_clockwise: outward }) }
}

// ---------- small vector kit ----------
fn add(a: P3, b: P3) -> P3 { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
fn sub(a: P3, b: P3) -> P3 { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn mul(a: P3, s: f64) -> P3 { [a[0] * s, a[1] * s, a[2] * s] }
fn dot(a: P3, b: P3) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross(a: P3, b: P3) -> P3 { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
fn unit(a: P3) -> P3 { let n = dot(a, a).sqrt(); mul(a, 1.0 / n) }
fn radial(phi: f64) -> P3 { [phi.cos(), 0.0, phi.sin()] }
fn tangential(phi: f64) -> P3 { [-phi.sin(), 0.0, phi.cos()] }
const UP: P3 = [0.0, 1.0, 0.0];

/// Bell (kalathos) radius at height y.
fn bell_r(y: f64) -> f64 {
    let t = ((y - 8.75) / 1.1).clamp(0.0, 1.0);
    0.46 + 0.14 * t * t
}

struct Mesh { points: Vec<P3>, faces: Vec<Vec<u32>> }

/// A thickened, lobed, cupped acanthus leaf as a closed triangle polyhedron.
fn leaf(phi: f64, y0: f64, height: f64, width: f64, lift: f64, nu: usize, nv: usize) -> Mesh {
    let (er, et) = (radial(phi), tangential(phi));
    let surf = |u: f64, v: f64| -> P3 {
        let lobes = 1.0 + 0.28 * (5.0 * PI * v).sin().max(0.0) + 0.1 * (11.0 * PI * v).sin().max(0.0);
        let hw = width * (0.32 + 0.68 * (PI * v.powf(0.85)).sin().max(0.0)) * lobes * (1.0 - 0.85 * v.powi(6)) + 0.012;
        let droop = (v - 0.68).max(0.0);
        let y = y0 + height * v - 1.4 * height * droop * droop;
        let serr = 0.010 * (7.0 * PI * u.abs() + 9.0 * PI * v).sin() * u * u;
        let r = bell_r(y) + lift + 0.02 + 0.30 * v.powf(2.6) * height
            + 0.045 * u * u            // cupped
            - 0.012 * (-(u / 0.12) * (u / 0.12)).exp() + 0.007 * (-(u / 0.04) * (u / 0.04)).exp() // midrib
            + 0.004 * (5.0 * PI * u).cos() * v // veins
            + serr;
        add(add(mul(er, r), mul(et, hw * u)), [0.0, y, 0.0])
    };
    thicken(nu, nv, surf, mul(er, -0.014))
}

/// Grid surface -> closed solid by an offset copy plus rims. Faces wound outward.
fn thicken(nu: usize, nv: usize, surf: impl Fn(f64, f64) -> P3, offset: P3) -> Mesh {
    let mut points = Vec::new();
    for layer in 0..2 {
        for j in 0..=nv {
            for i in 0..=nu {
                let p = surf(-1.0 + 2.0 * i as f64 / nu as f64, j as f64 / nv as f64);
                points.push(if layer == 0 { p } else { add(p, offset) });
            }
        }
    }
    let idx = |l: usize, i: usize, j: usize| (l * (nu + 1) * (nv + 1) + j * (nu + 1) + i) as u32;
    let mut faces = Vec::new();
    for j in 0..nv {
        for i in 0..nu {
            let (a, b, c, d) = (idx(0, i, j), idx(0, i + 1, j), idx(0, i + 1, j + 1), idx(0, i, j + 1));
            faces.push(vec![a, c, b]); faces.push(vec![a, d, c]);
            let (a, b, c, d) = (idx(1, i, j), idx(1, i + 1, j), idx(1, i + 1, j + 1), idx(1, i, j + 1));
            faces.push(vec![a, b, c]); faces.push(vec![a, c, d]);
        }
    }
    // boundary loop in (i,j) order
    let mut ring = Vec::new();
    for i in 0..nu { ring.push((i, 0)); }
    for j in 0..nv { ring.push((nu, j)); }
    for i in (1..=nu).rev() { ring.push((i, nv)); }
    for j in (1..=nv).rev() { ring.push((0, j)); }
    for k in 0..ring.len() {
        let (i0, j0) = ring[k];
        let (i1, j1) = ring[(k + 1) % ring.len()];
        let (a, b, c, d) = (idx(0, i0, j0), idx(0, i1, j1), idx(1, i1, j1), idx(1, i0, j0));
        faces.push(vec![a, b, c]); faces.push(vec![a, c, d]);
    }
    Mesh { points, faces }
}

/// Tube of varying radius along a polyline (parallel-transport frames), capped.
fn tube(path: &[P3], radius: impl Fn(f64) -> f64, seg: usize) -> Mesh {
    let n = path.len();
    let tangent = |k: usize| unit(sub(path[(k + 1).min(n - 1)], path[k.saturating_sub(1)]));
    let t0 = tangent(0);
    let helper = if t0[1].abs() < 0.9 { UP } else { [1.0, 0.0, 0.0] };
    let mut nrm = unit(cross(t0, helper));
    let mut points = Vec::new();
    for k in 0..n {
        let t = tangent(k);
        nrm = unit(sub(nrm, mul(t, dot(nrm, t))));
        let bin = cross(t, nrm);
        let r = radius(k as f64 / (n - 1) as f64);
        for s in 0..seg {
            let a = TAU * s as f64 / seg as f64;
            points.push(add(path[k], add(mul(nrm, r * a.cos()), mul(bin, r * a.sin()))));
        }
    }
    let id = |k: usize, s: usize| (k * seg + s % seg) as u32;
    let mut faces = Vec::new();
    for k in 0..n - 1 {
        for s in 0..seg {
            faces.push(vec![id(k, s), id(k, s + 1), id(k + 1, s + 1)]);
            faces.push(vec![id(k, s), id(k + 1, s + 1), id(k + 1, s)]);
        }
    }
    faces.push((0..seg).rev().map(|s| id(0, s)).collect());
    faces.push((0..seg).map(|s| id(n - 1, s)).collect());
    Mesh { points, faces }
}

fn bezier(a: P3, b: P3, c: P3, d: P3, t: f64) -> P3 {
    let u = 1.0 - t;
    add(add(mul(a, u * u * u), mul(b, 3.0 * u * u * t)), add(mul(c, 3.0 * u * t * t), mul(d, t * t * t)))
}

/// Stalk rising from the bell, ending in a logarithmic spiral in the face plane.
fn scroll(phi_face: f64, side: f64, centre_t: f64, centre_y: f64, r0: f64, turns: f64, start_t: f64, start_y: f64) -> Vec<P3> {
    let (n, t) = (radial(phi_face), mul(tangential(phi_face), side));
    let out = 0.60;
    let c = add(add(mul(n, out), mul(t, centre_t)), [0.0, centre_y, 0.0]);
    let decay = (0.18f64).ln() / (turns * TAU);
    let spiral = |a: f64| -> P3 {
        let r = r0 * (decay * a).exp();
        add(c, add(mul(t, r * a.sin()), mul(UP, r * a.cos())))
    };
    let a_pt = add(add(mul(n, bell_r(start_y) + 0.05), mul(t, start_t)), [0.0, start_y, 0.0]);
    let s0 = spiral(0.0);
    let mut path: Vec<P3> = (0..16)
        .map(|k| bezier(a_pt, add(a_pt, [0.0, 0.25, 0.0]), sub(s0, mul(t, 0.18)), s0, k as f64 / 16.0))
        .collect();
    let steps = (turns * 36.0) as usize;
    path.extend((0..=steps).map(|k| spiral(turns * TAU * k as f64 / steps as f64)));
    path
}

fn poly(w: &mut WorldGraph, id: &str, mesh: Mesh, parts: &mut Vec<String>) {
    let try_make = |w: &mut WorldGraph, faces: Vec<Vec<u32>>| {
        w.create_primitive(Primitive::Polyhedron { points: mesh.points.clone(), faces }, opts(id, None))
    };
    let ok = match try_make(w, mesh.faces.clone()) {
        Ok(_) => true,
        Err(_) => try_make(w, mesh.faces.iter().map(|f| f.iter().rev().copied().collect()).collect()).is_ok(),
    };
    if ok { parts.push(id.into()); } else { eprintln!("  ! {id} rejected"); }
}


/// Cross-section of the fluted shaft as ONE closed 2D profile.
/// Flute k is a circle of radius rf centred at distance d on the ray φk; it meets the
/// shaft circle (radius R) at the angles φk ± α with  cos α = (R² + d² − rf²) / (2·R·d).
fn fluted_profile(r: f64, d: f64, rf: f64, n: usize) -> Primitive {
    let alpha = ((r * r + d * d - rf * rf) / (2.0 * r * d)).acos();
    let on = |a: f64| [r * a.cos(), r * a.sin()];
    let start = on(-alpha);
    let mut segments = Vec::with_capacity(2 * n);
    for k in 0..n {
        let phi = TAU * k as f64 / n as f64;
        let (a, b) = (on(phi - alpha), on(phi + alpha));
        let c = [d * phi.cos(), d * phi.sin()];
        let (u, v) = ([a[0] - c[0], a[1] - c[1]], [b[0] - c[0], b[1] - c[1]]);
        // the part of the flute circle inside the shaft is the short arc
        segments.push(ProfileSegment { to: b, arc: Some(SegmentArc { center: c, counter_clockwise: u[0] * v[1] - u[1] * v[0] > 0.0 }) });
        let next = if k + 1 == n { start } else { on(TAU * (k + 1) as f64 / n as f64 - alpha) };
        segments.push(ProfileSegment { to: next, arc: Some(SegmentArc { center: [0.0, 0.0], counter_clockwise: true }) });
    }
    Primitive::Profile { start, segments, closed: true }
}

/// One definition, many placements: rotate copies about the column axis.
fn ring(w: &mut WorldGraph, source: &str, name: &str, count: usize, step_deg: f64, parts: &mut Vec<String>) {
    for k in 1..count {
        let id = format!("{name}-{k}");
        match w.instance(source, CopyOptions { node_id: Some(id.clone()), ..Default::default() }) {
            Ok(_) => {
                w.transform(&id, Transform::Rotate { axis: [0.0, 1.0, 0.0], degrees: step_deg * k as f64, pivot: Some([0.0, 0.0, 0.0]) }).unwrap();
                parts.push(id);
            }
            Err(e) => eprintln!("  ! instance {id}: {e:?}"),
        }
    }
}

fn main() {
    // the kernel is assembled by a builder; probe and layers are injected
    let mut kernel = Kernel::builder().probe(BoxTree::new()).layer(KeyLayer::default()).build().unwrap();
    let mut parts: Vec<String> = Vec::new();
    let t = std::time::Instant::now();
    let w = kernel.world_mut();

    // ---- plinth + Attic base (exact) ----
    w.create_primitive(Primitive::Cuboid { width: 1.42, height: 0.16, depth: 1.42 }, opts("plinth", at([0.0, 0.0, 0.0]))).unwrap();
    parts.push("plinth".into());
    let base = Primitive::Profile {
        start: [0.0, 0.16],
        segments: vec![
            line([0.66, 0.16]),
            half_round([0.66, 0.16], [0.66, 0.28], true),
            line([0.60, 0.28]), line([0.60, 0.295]),
            half_round([0.60, 0.295], [0.60, 0.385], false),
            line([0.60, 0.40]), line([0.575, 0.40]),
            half_round([0.575, 0.40], [0.575, 0.475], true),
            line([0.515, 0.475]), line([0.515, 0.50]),
            line([0.0, 0.50]), line([0.0, 0.16]),
        ],
        closed: true,
    };
    w.create_primitive(base, opts("base-profile", upright())).unwrap();
    w.create_operation(CreatingOperation::Revolve { profile: "base-profile".into(), axis: None }, opts("base", None)).unwrap();
    w.dispose("base-profile").ok();
    parts.push("base".into());

    // ---- shaft: the 24 flutes are ONE extruded profile, not 24 booleans ----
    let (y0, y1, r) = (0.50, 8.75, 0.5);
    let (rf, rc, fy0, fy1) = (0.046, 0.506, 0.95, 8.30);
    let t_shaft = std::time::Instant::now();
    w.create_primitive(Primitive::Cylinder { radius: r, height: fy0 - y0 }, opts("shaft-foot", at([0.0, y0, 0.0]))).unwrap();
    w.create_primitive(fluted_profile(r, rc, rf, 24), opts("flute-profile", at([0.0, fy0, 0.0]))).unwrap();
    w.create_operation(CreatingOperation::Extrude { profile: "flute-profile".into(), holes: vec![], distance: fy1 - fy0 }, opts("shaft-flutes", None)).unwrap();
    w.dispose("flute-profile").ok();
    w.create_primitive(Primitive::Cylinder { radius: r, height: y1 - fy1 }, opts("shaft-neck", at([0.0, fy1, 0.0]))).unwrap();
    parts.extend(["shaft-foot".into(), "shaft-flutes".into(), "shaft-neck".into()]);
    println!("fluted shaft: {:?}", t_shaft.elapsed());
    w.create_primitive(Primitive::Torus { major_radius: 0.47, minor_radius: 0.045 }, opts("astragal", at([0.0, 8.60, 0.0]))).unwrap();
    w.create_primitive(Primitive::Torus { major_radius: 0.50, minor_radius: 0.022 }, opts("apophyge", at([0.0, 0.50, 0.0]))).unwrap();
    parts.extend(["astragal".into(), "apophyge".into()]);

    // ---- bell ----
    let bell = Primitive::Profile {
        start: [0.0, 8.75],
        segments: vec![
            line([bell_r(8.75), 8.75]),
            arc([bell_r(8.75), 8.75], [0.60, 9.85], 2.2, true),
            line([0.0, 9.85]), line([0.0, 8.75]),
        ],
        closed: true,
    };
    w.create_primitive(bell, opts("bell-profile", upright())).unwrap();
    w.create_operation(CreatingOperation::Revolve { profile: "bell-profile".into(), axis: None }, opts("bell", None)).unwrap();
    w.dispose("bell-profile").ok();
    parts.push("bell".into());

    // ---- abacus ----
    let (ay, ah, aw) = (9.85, 0.19, 1.64);
    w.create_primitive(Primitive::Cuboid { width: aw, height: ah, depth: aw }, opts("abacus", at([0.0, ay, 0.0]))).unwrap();
    for e in ["front|left", "front|right", "back|left", "back|right"] {
        if let Err(err) = w.chamfer_named("abacus", &[e], 0.09, EditScope::Node) { println!("! chamfer {e}: {err:?}"); }
    }
    let rho = 1.5;
    let mut cut = Vec::new();
    for k in 0..4 {
        let phi = PI / 2.0 * k as f64;
        let d = aw / 2.0 - 0.13 + rho;
        let id = format!("abacus-cut-{k}");
        w.create_primitive(Primitive::Cylinder { radius: rho, height: ah + 0.1 }, opts(&id, at([d * phi.cos(), ay - 0.05, d * phi.sin()]))).unwrap();
        cut.push(id);
    }
    if let Err(e) = w.operate("abacus", ModifyingOperation::Subtract, &cut, EditScope::Node) { println!("! abacus cut: {e:?}"); }
    for id in &cut { w.dispose(id).ok(); }
    parts.push("abacus".into());
    println!("exact parts done: {:?}", t.elapsed());

    // ---- ornament: one definition each, the rest are instances (8-, 4-fold symmetry) ----
    let p = add(mul(radial(0.0), aw / 2.0 - 0.10), [0.0, ay + ah / 2.0, 0.0]);
    w.create_primitive(Primitive::Sphere { radius: 0.075 }, opts("fleuron-0", at(p))).unwrap();
    parts.push("fleuron-0".into());
    ring(w, "fleuron-0", "fleuron", 4, 90.0, &mut parts);
    poly(w, "leaf-low-0", leaf(0.0, 8.76, 0.40, 0.17, 0.035, 14, 18), &mut parts);
    ring(w, "leaf-low-0", "leaf-low", 8, 45.0, &mut parts);
    poly(w, "leaf-high-0", leaf(PI / 8.0, 8.78, 0.74, 0.16, 0.0, 14, 22), &mut parts);
    ring(w, "leaf-high-0", "leaf-high", 8, 45.0, &mut parts);
    for (s, side) in [(0, 1.0), (1, -1.0)] {
        let v = scroll(0.0, side, 0.56, 9.70, 0.13, 1.75, 0.20, 9.25);
        poly(w, &format!("volute-{s}-0"), tube(&v, |q| 0.030 - 0.018 * q, 8), &mut parts);
        ring(w, &format!("volute-{s}-0"), &format!("volute-{s}"), 4, 90.0, &mut parts);
        let h = scroll(0.0, side, 0.19, 9.70, 0.07, 1.6, 0.07, 9.35);
        poly(w, &format!("helix-{s}-0"), tube(&h, |q| 0.020 - 0.012 * q, 6), &mut parts);
        ring(w, &format!("helix-{s}-0"), &format!("helix-{s}"), 4, 90.0, &mut parts);
    }
    let n = radial(0.0);
    let path: Vec<P3> = (0..=12).map(|k| {
        let q = k as f64 / 12.0;
        let y = 9.40 + 0.48 * q;
        add(mul(n, bell_r(y) + 0.04 + 0.08 * q * q), [0.0, y, 0.0])
    }).collect();
    poly(w, "stalk-0", tube(&path, |q| 0.018 - 0.006 * q, 8), &mut parts);
    ring(w, "stalk-0", "stalk", 4, 90.0, &mut parts);
    let build_time = t.elapsed();
    println!("MODEL BUILT: {} bodies, {} shapes, {:?}", parts.len(), w.shape_count(), build_time);

    // ---- proofs ----
    let lens = {
        let (rr, r2, d) = (r, rf, rc);
        r2 * r2 * ((d * d + r2 * r2 - rr * rr) / (2.0 * d * r2)).acos() + rr * rr * ((d * d + rr * rr - r2 * r2) / (2.0 * d * rr)).acos()
            - 0.5 * ((-d + r2 + rr) * (d + r2 - rr) * (d - r2 + rr) * (d + r2 + rr)).sqrt()
    };
    let expected = PI * r * r * (y1 - y0) - 24.0 * lens * (fy1 - fy0);
    let measured: f64 = ["shaft-foot", "shaft-flutes", "shaft-neck"].iter().map(|id| w.measure(id, None).unwrap().volume).sum();
    println!("shaft volume: kernel {measured:.9}  closed form {expected:.9}  diff {:.2e}", (measured - expected).abs());
    let flutes = w.placed_brep("shaft-flutes").unwrap();
    println!("shaft-flutes faces: {}", flutes.topology.faces.len());
    for id in ["base", "shaft-flutes", "bell", "abacus", "leaf-low-3", "volute-1-2"] {
        let b = w.bounds(id).unwrap().unwrap();
        println!("bounds {id}: x {:.3}..{:.3}  y {:.3}..{:.3}  z {:.3}..{:.3}", b[0], b[3], b[1], b[4], b[2], b[5]);
    }
    println!("ray across shaft at y=4: {}", perception::sentence(&kernel.ray("shaft-flutes", [2.0, 4.0, 0.0], [-1.0, 0.0, 0.0]).unwrap(), false));

    // probe strategies on a dense body (one leaf): same answers, different cost
    let leaf_brep = kernel.world().placed_brep("leaf-high-0").unwrap();
    let rays: Vec<(P3, P3)> = (0..120).map(|k| {
        let a = TAU * k as f64 / 120.0;
        ([3.0 * (a * 3.0).cos(), 8.8 + 0.7 * (k % 20) as f64 / 20.0, 3.0 * (a * 3.0).sin()], [-(a * 3.0).cos(), 0.02, -(a * 3.0).sin()])
    }).collect();
    let run = |p: &dyn Probe| {
        let t = std::time::Instant::now();
        let hits: usize = rays.iter().map(|(o, d)| p.ray(&leaf_brep, *o, *d).unwrap().len()).sum();
        (hits, t.elapsed())
    };
    let tree = BoxTree::new();
    let _ = tree.ray(&leaf_brep, rays[0].0, rays[0].1); // build the tree once
    let (h1, t1) = run(&Exhaustive);
    let (h2, t2) = run(&tree);
    println!("probe on a leaf ({} faces), 120 rays: exhaustive {} hits in {:?}; box-tree {} hits in {:?}  -> x{:.1}",
        leaf_brep.topology.faces.len(), h1, t1, h2, t2, t1.as_secs_f64() / t2.as_secs_f64());
    kernel.refresh("abacus").unwrap();
    let found = kernel.layer::<KeyLayer>().unwrap().index("abacus").unwrap().find(&perception::Query::parse("cylinder r:1.5"), 8);
    println!("find `cylinder r:1.5` on the abacus: {} faces", found.len());

    // ---- display mesh: tessellate each SHAPE once, place it per node ----
    let w = kernel.world();
    let mut cache: std::collections::BTreeMap<String, (Vec<f32>, Vec<u32>, [f64; 3])> = Default::default();
    let mut out = Vec::new();
    for id in &parts {
        let node = w.node(id).unwrap();
        let Some(shape) = node.shape.clone() else { continue };
        if !cache.contains_key(&shape) {
            match w.buffers(&shape, 0.004, 1_500_000) {
                Ok(b) => { cache.insert(shape.clone(), (b.positions.clone(), b.indices.clone(), b.origin)); }
                Err(e) => { eprintln!("tess {id}: {e:?}"); continue }
            }
        }
        let (positions, indices, origin) = &cache[&shape];
        let m = w.world_matrix(id).unwrap();
        let mut pos = Vec::with_capacity(positions.len());
        for c in positions.chunks(3) {
            let p = [c[0] as f64 + origin[0], c[1] as f64 + origin[1], c[2] as f64 + origin[2]];
            for r in 0..3 { pos.push((m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r]) as f32); }
        }
        let kind = if id.starts_with("leaf") || id.starts_with("volute") || id.starts_with("helix") || id.starts_with("stalk") { "approx" } else { "exact" };
        out.push((id.clone(), kind, pos, indices.clone()));
    }
    let tris: usize = out.iter().map(|o| o.3.len() / 3).sum();
    println!("triangles for display: {tris} (tessellated {} shapes); total {:?}", cache.len(), t.elapsed());
    let mut bin = Vec::new();
    let mut meta = Vec::new();
    for (id, kind, pos, idx) in &out {
        let (po, io) = (bin.len(), bin.len() + pos.len() * 4);
        for v in pos { bin.extend_from_slice(&v.to_le_bytes()); }
        for v in idx { bin.extend_from_slice(&v.to_le_bytes()); }
        meta.push(json!({"id": id, "kind": kind, "pos": [po, pos.len()], "idx": [io, idx.len()]}));
    }
    std::fs::write("/home/claude/column.bin", &bin).unwrap();
    std::fs::write("/home/claude/column.json", serde_json::to_string(&meta).unwrap()).unwrap();
    let options = StepOptions { unit: "metre".into(), up_axis: "Y".into(), name: "korinthoszi-oszlop".into(), timestamp: "2026-11-13T12:00:00".into(), ..StepOptions::default() };
    let t_step = std::time::Instant::now();
    match w.export_step(&parts, &options) {
        Ok((text, report)) => {
            std::fs::write("/home/claude/korinthoszi-oszlop.step", &text).unwrap();
            println!("STEP: {:.1} MB, {} products, {} faces, in {:?}", text.len() as f64 / 1e6, report.products, report.faces, t_step.elapsed());
        }
        Err(e) => println!("! STEP export failed: {e:?}"),
    }
}
