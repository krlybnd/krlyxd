//! A flat outline with holes cut into triangles, then refined until the surface stays within
//! the tolerance of them.
//!
//! Holes are joined to the outline by bridges (the method of Eberly, "Triangulation by ear
//! clipping"), the polygon is cut ear by ear, sides are flipped towards a Delaunay
//! triangulation (Lawson), and a triangle the surface bulges away from has its longest free
//! side halved, in rounds, until none does. Sides of the boundary, seams and poles are never
//! flipped or halved, so that the neighbouring faces keep sharing their points.

use super::layout::Layout;
use super::{Abort, Work};
use krly3d_core::math::{Point3, Uv};
use krly3d_kernel::ports::surface::Surface;
use std::collections::{BTreeMap, BTreeSet};

/// Rounds of refinement at most, and triangles at most: past them the face is too fine
/// for the tolerance asked and is refused rather than allowed to eat the memory.
#[derive(Clone, Copy)]
pub(super) struct Quality {
    pub refinement_rounds: usize,
    pub triangles: usize,
    pub flip_rounds: usize,
}

impl Default for Quality {
    fn default() -> Self {
        Self { refinement_rounds: 40, triangles: 400_000, flip_rounds: 200 }
    }
}

fn cross(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn signed_area(work: &mut Work<'_>, flat: &[[f64; 2]], polygon: &[usize]) -> Result<f64, Abort> {
    let mut sum = -0.0;
    for i in 0..polygon.len() {
        work.step()?;
        let (a, b) = (flat[polygon[i]], flat[polygon[(i + 1) % polygon.len()]]);
        sum += a[0] * b[1] - b[0] * a[1];
    }
    Ok(sum * 0.5)
}

/// Triangles of the layout's points, all counter-clockwise in the plane, refined to the
/// tolerance; the layout gains the points the refinement adds.
pub(super) fn triangles(
    work: &mut Work<'_>,
    layout: &mut Layout,
    surface: &dyn Surface,
    points: &mut Vec<Point3>,
    tolerance: f64,
) -> Result<Vec<[usize; 3]>, Abort> {
    work.check()?;
    let mut outer = layout.outer.clone();
    if signed_area(work, &layout.flat, &outer)? < 0.0 {
        outer.reverse();
    }
    let mut holes = layout.holes.clone();
    for hole in &mut holes {
        work.step()?;
        if signed_area(work, &layout.flat, hole)? > 0.0 {
            hole.reverse();
        }
    }
    let polygon = bridged(work, &layout.flat, outer, holes)?;
    let mut found = clipped(work, &layout.flat, polygon)?;
    flip(work, layout, &mut found)?;
    refine(work, layout, surface, points, tolerance, &mut found)?;
    Ok(found)
}

/// Holes joined to the outline, rightmost first: from the rightmost point of a hole to a
/// point of the outline it can see.
fn bridged(
    work: &mut Work<'_>,
    flat: &[[f64; 2]],
    mut outer: Vec<usize>,
    holes: Vec<Vec<usize>>,
) -> Result<Vec<usize>, Abort> {
    // The old stable sort repeatedly scanned entire holes in its comparator. Order
    // by the same total-order float key and original index, checking every point.
    let mut ordered = BTreeMap::new();
    for (index, hole) in holes.into_iter().enumerate() {
        work.step()?;
        let mut right = f64::NEG_INFINITY;
        for i in &hole {
            work.step()?;
            right = right.max(flat[*i][0]);
        }
        let bits = right.to_bits();
        let key = if bits >> 63 == 1 { !bits } else { bits | (1 << 63) };
        ordered.insert((std::cmp::Reverse(key), index), hole);
    }
    for (_, hole) in ordered {
        work.step()?;
        let mut rightmost: Option<usize> = None;
        for i in 0..hole.len() {
            work.step()?;
            rightmost = Some(match rightmost {
                Some(m) if flat[hole[i]][0].total_cmp(&flat[hole[m]][0]).is_lt() => m,
                _ => i,
            });
        }
        let Some(m) = rightmost else { continue };
        let point = flat[hole[m]];
        // The nearest side of the outline that a ray to the right meets, and its end to the right.
        let mut best: Option<(f64, usize)> = None;
        for i in 0..outer.len() {
            work.step()?;
            let (a, b) = (flat[outer[i]], flat[outer[(i + 1) % outer.len()]]);
            if (a[1] - point[1]) * (b[1] - point[1]) > 0.0 || a[1] == b[1] {
                continue;
            }
            let x = a[0] + (point[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if x >= point[0] && best.is_none_or(|(known, _)| x < known) {
                let end = if a[0] > b[0] { i } else { (i + 1) % outer.len() };
                best = Some((x, end));
            }
        }
        let mut target = best.map_or(0, |(_, end)| end);
        // A point of the outline inside the triangle of the bridge would block the view;
        // the one at the smallest angle from the ray is visible.
        if let Some((x, _)) = best {
            let (hit, end) = ([x, point[1]], flat[outer[target]]);
            let mut angle = f64::INFINITY;
            for (index, candidate) in outer.iter().enumerate() {
                work.step()?;
                let at = flat[*candidate];
                let inside = cross(point, hit, at) >= 0.0
                    && cross(hit, end, at) >= 0.0
                    && cross(end, point, at) >= 0.0;
                if inside && index != target {
                    let here = (at[1] - point[1]).abs().atan2(at[0] - point[0]);
                    if here < angle {
                        angle = here;
                        target = index;
                    }
                }
            }
        }
        let mut joined = Vec::with_capacity(outer.len() + hole.len() + 2);
        for id in outer[..=target].iter().chain(hole[m..].iter()).chain(hole[..=m].iter()) {
            work.step()?;
            joined.push(*id);
        }
        joined.push(outer[target]);
        for id in &outer[target + 1..] {
            work.step()?;
            joined.push(*id);
        }
        outer = joined;
    }
    Ok(outer)
}

/// Cuts ears off a counter-clockwise polygon until three points are left.
fn clipped(
    work: &mut Work<'_>,
    flat: &[[f64; 2]],
    mut polygon: Vec<usize>,
) -> Result<Vec<[usize; 3]>, Abort> {
    let mut found = Vec::with_capacity(polygon.len());
    while polygon.len() > 3 {
        work.step()?;
        let count = polygon.len();
        let mut ear = None;
        for i in 0..count {
            work.step()?;
            let (a, b, c) = (polygon[(i + count - 1) % count], polygon[i], polygon[(i + 1) % count]);
            if cross(flat[a], flat[b], flat[c]) <= 0.0 {
                continue;
            }
            let mut blocked = false;
            for other in &polygon {
                work.step()?;
                let at = flat[*other];
                let shared = [a, b, c].iter().any(|corner| flat[*corner] == at);
                if !shared
                    && cross(flat[a], flat[b], at) >= 0.0
                    && cross(flat[b], flat[c], at) >= 0.0
                    && cross(flat[c], flat[a], at) >= 0.0
                {
                    blocked = true;
                    break;
                }
            }
            if !blocked {
                ear = Some(i);
                break;
            }
        }
        // Rounding can leave no clean ear; the most convex corner is cut then.
        let i = match ear {
            Some(i) => i,
            None => {
                let turn = |i: usize| {
                    cross(
                        flat[polygon[(i + count - 1) % count]],
                        flat[polygon[i]],
                        flat[polygon[(i + 1) % count]],
                    )
                };
                let mut best = 0;
                for i in 0..count {
                    work.step()?;
                    if !turn(i).total_cmp(&turn(best)).is_lt() {
                        best = i;
                    }
                }
                best
            }
        };
        found.push([polygon[(i + count - 1) % count], polygon[i], polygon[(i + 1) % count]]);
        polygon.remove(i);
    }
    if polygon.len() == 3 {
        found.push([polygon[0], polygon[1], polygon[2]]);
    }
    Ok(found)
}

/// For every free side, the two triangles beside it with the corner of each opposite it.
/// With `cuttable`, only the sides between two different points in space: a side between
/// two layouts of one point (along a pole, across a seam) has no middle to cut at.
type Sides = BTreeMap<(usize, usize), Vec<(usize, usize)>>;

fn sides(
    work: &mut Work<'_>,
    triangles: &[[usize; 3]],
    layout: &Layout,
    cuttable: bool,
) -> Result<Sides, Abort> {
    let mut map = Sides::new();
    for (index, triangle) in triangles.iter().enumerate() {
        work.step()?;
        for k in 0..3 {
            let (a, b) = (triangle[k], triangle[(k + 1) % 3]);
            let key = (a.min(b), a.max(b));
            if !layout.fixed.contains(&key) && (!cuttable || layout.ids[a] != layout.ids[b]) {
                map.entry(key).or_default().push((index, triangle[(k + 2) % 3]));
            }
        }
    }
    Ok(map)
}

/// Lawson flips: a side whose opposite corners see it under more than a straight angle
/// is replaced by the other diagonal of its quadrilateral, while that is convex. Each round
/// flips every such side whose two triangles no other flip of the round has touched.
fn flip(work: &mut Work<'_>, layout: &Layout, triangles: &mut [[usize; 3]]) -> Result<(), Abort> {
    let flat = &layout.flat;
    for _ in 0..work.quality.flip_rounds {
        work.check()?;
        let mut touched = BTreeSet::new();
        for ((a, b), beside) in sides(work, triangles, layout, false)? {
            work.step()?;
            let [(first, c), (second, d)] = beside[..] else { continue };
            if touched.contains(&first)
                || touched.contains(&second)
                || [a, b, c, d].iter().collect::<BTreeSet<_>>().len() < 4
            {
                continue;
            }
            let angle = |at: usize| {
                let (u, v) = (
                    [flat[a][0] - flat[at][0], flat[a][1] - flat[at][1]],
                    [flat[b][0] - flat[at][0], flat[b][1] - flat[at][1]],
                );
                (u[0] * v[1] - u[1] * v[0]).abs().atan2(u[0] * v[0] + u[1] * v[1])
            };
            let convex = cross(flat[c], flat[d], flat[a]) * cross(flat[c], flat[d], flat[b]) < 0.0
                && cross(flat[a], flat[b], flat[c]) * cross(flat[a], flat[b], flat[d]) < 0.0;
            if convex && angle(c) + angle(d) > std::f64::consts::PI * (1.0 + 1e-9) {
                // Keep each triangle counter-clockwise in the plane.
                let ccw = |x: usize, y: usize, z: usize| {
                    if cross(flat[x], flat[y], flat[z]) > 0.0 {
                        [x, y, z]
                    } else {
                        [x, z, y]
                    }
                };
                triangles[first] = ccw(c, d, a);
                triangles[second] = ccw(d, c, b);
                touched.insert(first);
                touched.insert(second);
            }
        }
        if touched.is_empty() {
            return Ok(());
        }
    }
    Ok(())
}

/// Halves the longest free side of every triangle the surface leaves by more than the
/// tolerance, in rounds, flipping after each.
fn refine(
    work: &mut Work<'_>,
    layout: &mut Layout,
    surface: &dyn Surface,
    points: &mut Vec<Point3>,
    tolerance: f64,
    triangles: &mut Vec<[usize; 3]>,
) -> Result<(), Abort> {
    let mut cut_at: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    let same = 1e-3 * tolerance;
    for _ in 0..work.quality.refinement_rounds {
        work.check()?;
        let mut rough = Vec::new();
        for (t, triangle) in triangles.iter().enumerate() {
            work.step()?;
            if off(layout, surface, points, *triangle, tolerance) {
                rough.push(t);
            }
        }
        if rough.is_empty() {
            return Ok(());
        }
        if triangles.len() > work.quality.triangles {
            let most = work.quality.triangles;
            return Err(Abort::Budget(format!(
                "the face needs more than {most} triangles for a tolerance of {tolerance}"
            )));
        }
        let beside = sides(work, triangles, layout, true)?;
        let mut touched = BTreeSet::new();
        let mut halved = 0;
        for triangle in rough {
            work.step()?;
            if touched.contains(&triangle) {
                continue;
            }
            let [a, b, c] = triangles[triangle];
            let length = |x: usize, y: usize| (points[layout.ids[x]] - points[layout.ids[y]]).norm();
            let mut candidates = [(a, b), (b, c), (c, a)];
            candidates.sort_by(|p, q| length(q.0, q.1).total_cmp(&length(p.0, p.1)));
            let Some(&(x, y)) =
                candidates.iter().find(|(x, y)| beside.contains_key(&((*x).min(*y), (*x).max(*y))))
            else {
                continue;
            };
            let key = (x.min(y), x.max(y));
            let around = &beside[&key];
            let mut already_touched = false;
            for (t, _) in around {
                work.step()?;
                if touched.contains(t) {
                    already_touched = true;
                    break;
                }
            }
            if already_touched {
                continue;
            }
            let middle = Uv::from(0.5 * (layout.uv[x].coords + layout.uv[y].coords));
            let pair = (layout.ids[x].min(layout.ids[y]), layout.ids[x].max(layout.ids[y]));
            let at = surface.point(middle);
            // Copies across a seam meet at the same point in space; two sides from one pole
            // to one point do not, and get a point each.
            let made = cut_at.entry(pair).or_default();
            let mut known = None;
            for id in made.iter() {
                work.step()?;
                if (points[*id] - at).norm() <= same {
                    known = Some(*id);
                    break;
                }
            }
            let new_id = match known {
                Some(id) => id,
                None => {
                    points.push(at);
                    made.push(points.len() - 1);
                    points.len() - 1
                }
            };
            layout.uv.push(middle);
            layout.flat.push([middle.x * layout.scale[0], middle.y * layout.scale[1]]);
            layout.ids.push(new_id);
            let m = layout.ids.len() - 1;
            for (t, _) in around {
                work.step()?;
                let corners = triangles[*t];
                touched.insert(*t);
                // The triangle becomes two, each keeping its turning sense.
                let split = |keep: usize| corners.map(|corner| if corner == keep { m } else { corner });
                triangles[*t] = split(y);
                triangles.push(split(x));
                touched.insert(triangles.len() - 1);
            }
            halved += 1;
        }
        if halved == 0 {
            return Ok(());
        }
        flip(work, layout, triangles)?;
    }
    Ok(())
}

/// Whether the surface strays from the triangle by more than the tolerance at the middles
/// of its free sides or at its centre.
fn off(
    layout: &Layout,
    surface: &dyn Surface,
    points: &[Point3],
    triangle: [usize; 3],
    tolerance: f64,
) -> bool {
    let at = |i: usize| points[layout.ids[i]];
    let [a, b, c] = triangle;
    if layout.ids[a] == layout.ids[b] || layout.ids[b] == layout.ids[c] || layout.ids[c] == layout.ids[a] {
        return false;
    }
    let centre_uv = Uv::from((layout.uv[a].coords + layout.uv[b].coords + layout.uv[c].coords) / 3.0);
    let centre = Point3::from((at(a).coords + at(b).coords + at(c).coords) / 3.0);
    if (surface.point(centre_uv) - centre).norm() > tolerance {
        return true;
    }
    [(a, b), (b, c), (c, a)].iter().any(|(x, y)| {
        if layout.fixed.contains(&((*x).min(*y), (*x).max(*y))) {
            return false;
        }
        let uv = Uv::from(0.5 * (layout.uv[*x].coords + layout.uv[*y].coords));
        (surface.point(uv) - Point3::from(0.5 * (at(*x).coords + at(*y).coords))).norm() > tolerance
    })
}
