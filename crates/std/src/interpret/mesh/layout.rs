//! A face laid out flat in its parameters, ready to be cut into triangles.
//!
//! Each boundary loop is projected onto the surface's parameters and unwrapped across the
//! periods, so that it runs without jumps. A loop that comes back shifted by a whole period
//! goes round the surface. Then:
//!
//! * no loop goes round: the largest loop is the outline, the others are holes;
//! * two loops go round the same way in opposite senses (the side of a cylinder, a band of
//!   a torus, a loft between sections): the face is cut open along a seam from the first
//!   loop to the second, and the cut is laid out on both sides with the same points;
//! * one loop goes round and the face reaches a pole of the surface (a cap of a sphere, a
//!   cone up to its apex): the outline is closed along the pole, all one point;
//! * no loop at all (a whole sphere or torus): the full rectangle of parameters, with its
//!   seams and poles.
//!
//! Anything else is declined with the reason.

use krly3d_core::math::{Point3, Uv};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::ports::trim::{place, Place, TrimRequest};
use krly3d_kernel::topo::Body;
use krly3d_kernel::Context;
use std::collections::BTreeSet;
use std::f64::consts::FRAC_PI_8;
use std::sync::Arc;

/// Halving steps a seam may take at most.
const DEEPEST: u32 = 16;

pub(super) struct Layout {
    /// Parameters of each laid-out point, its place in the plane (parameters scaled to
    /// lengths) and the number of the mesh point it stands for. A point on a seam or a pole
    /// is laid out more than once under one number.
    pub uv: Vec<Uv>,
    pub flat: Vec<[f64; 2]>,
    pub ids: Vec<usize>,
    pub outer: Vec<usize>,
    pub holes: Vec<Vec<usize>>,
    /// Sides along the boundary, a seam or a pole: never cut, never flipped.
    pub fixed: BTreeSet<(usize, usize)>,
    pub scale: [f64; 2],
}

impl Layout {
    fn add(&mut self, uv: Uv, id: usize) -> usize {
        self.uv.push(uv);
        self.flat.push([uv.x * self.scale[0], uv.y * self.scale[1]]);
        self.ids.push(id);
        self.ids.len() - 1
    }

    fn fix(&mut self, polygon: &[usize]) {
        for index in 0..polygon.len() {
            let (a, b) = (polygon[index], polygon[(index + 1) % polygon.len()]);
            self.fixed.insert((a.min(b), a.max(b)));
        }
    }
}

/// Lays out face `face` of `body`; `edges` are the edges' points as numbers into `points`,
/// to which the points of seams and poles are added.
pub(super) fn lay_out(
    cx: &Context,
    body: &Arc<Body>,
    face: usize,
    edges: &[Vec<usize>],
    points: &mut Vec<Point3>,
    tolerance: f64,
) -> Result<Layout, String> {
    let data = &body.faces[face];
    let surface = data.surface.as_ref();
    let periods = surface.periods();
    let poles: Vec<(f64, Point3)> =
        surface.poles().into_iter().map(|v| (v, surface.point(Uv::new(0.0, v)))).collect();
    let mut loops = Vec::with_capacity(data.loops.len());
    for uses in &data.loops {
        let mut ids = Vec::new();
        for used in uses {
            let along = &edges[used.edge];
            let ordered: Vec<usize> =
                if used.forward { along.clone() } else { along.iter().rev().copied().collect() };
            ids.extend_from_slice(&ordered[..ordered.len() - 1]);
        }
        let (uv, winding) = unwrapped(surface, &ids, points, &poles, periods, tolerance);
        loops.push(Loop { ids, uv, winding });
    }
    let centre = if loops.is_empty() {
        Uv::new(periods[0].map_or(0.0, |p| 0.5 * p), 0.0)
    } else {
        let all: Vec<Uv> = loops.iter().flat_map(|l| l.uv.iter().copied()).collect();
        Uv::from(all.iter().map(|uv| uv.coords).sum::<nalgebra::Vector2<f64>>() / all.len() as f64)
    };
    let mut layout = Layout {
        uv: Vec::new(),
        flat: Vec::new(),
        ids: Vec::new(),
        outer: Vec::new(),
        holes: Vec::new(),
        fixed: BTreeSet::new(),
        scale: scale(surface, centre),
    };
    let around: Vec<usize> = (0..loops.len()).filter(|index| loops[*index].winding != [0, 0]).collect();
    let direction = match around
        .iter()
        .map(|index| usize::from(loops[*index].winding[0] == 0))
        .collect::<BTreeSet<_>>()
        .len()
    {
        0 => None,
        1 => Some(usize::from(loops[around[0]].winding[0] == 0)),
        _ => return Err("boundary loops go round the surface in both directions".into()),
    };
    match (loops.len(), around.len(), direction) {
        (0, _, _) => whole(&mut layout, surface, periods, &poles, points)?,
        (_, 0, _) => {
            let outer = (0..loops.len())
                .max_by(|a, b| area(&layout, &loops[*a]).total_cmp(&area(&layout, &loops[*b])))
                .unwrap_or(0);
            layout.outer = place_loop(&mut layout, &loops[outer], 0.0, None);
            let reference = loops[outer].uv[0];
            for (_, other) in loops.iter().enumerate().filter(|(index, _)| *index != outer) {
                let hole = place_loop(&mut layout, other, 0.0, Some((reference, periods)));
                layout.holes.push(hole);
            }
        }
        (_, 2, Some(d)) => seam(&mut layout, surface, &loops, &around, d, periods, points, tolerance)?,
        (_, 1, Some(0)) => pole(cx, &mut layout, body, face, &loops, around[0], periods, &poles, points)?,
        _ => {
            return Err(format!(
                "a face with {} boundary loops, {} of them going round, cannot be laid out flat yet",
                loops.len(),
                around.len()
            ))
        }
    }
    let (outer, holes) = (layout.outer.clone(), layout.holes.clone());
    layout.fix(&outer);
    for hole in &holes {
        layout.fix(hole);
    }
    Ok(layout)
}

struct Loop {
    ids: Vec<usize>,
    uv: Vec<Uv>,
    /// How many whole periods the loop goes round in u and in v.
    winding: [i64; 2],
}

/// The parameters of the loop's points without jumps across a period, and how many times
/// it goes round. A point at a pole has no longitude of its own; it takes its neighbour's.
fn unwrapped(
    surface: &dyn Surface,
    ids: &[usize],
    points: &[Point3],
    poles: &[(f64, Point3)],
    periods: [Option<f64>; 2],
    tolerance: f64,
) -> (Vec<Uv>, [i64; 2]) {
    let mut uv: Vec<Uv> = Vec::with_capacity(ids.len());
    for id in ids {
        let point = points[*id];
        let mut here = surface.project(&point);
        if let Some((v, _)) = poles.iter().find(|(_, at)| (at - point).norm() <= tolerance) {
            here = Uv::new(uv.last().map_or(here.x, |before| before.x), *v);
        }
        if let Some(before) = uv.last() {
            for d in 0..2 {
                if let Some(period) = periods[d] {
                    here[d] += ((before[d] - here[d]) / period).round() * period;
                }
            }
        }
        uv.push(here);
    }
    let mut winding = [0, 0];
    if let (Some(first), Some(last)) = (uv.first(), uv.last()) {
        for d in 0..2 {
            if let Some(period) = periods[d] {
                let closing = first[d] + ((last[d] - first[d]) / period).round() * period;
                winding[d] = ((closing - first[d]) / period).round() as i64;
            }
        }
    }
    (uv, winding)
}

/// How long a step in u and in v is on the surface near `at`, so that the flat layout has
/// roughly the surface's proportions.
fn scale(surface: &dyn Surface, at: Uv) -> [f64; 2] {
    const STEP: f64 = 1e-4;
    let speed = |du: f64, dv: f64| {
        let ahead = surface.point(Uv::new(at.x + du, at.y + dv));
        let behind = surface.point(Uv::new(at.x - du, at.y - dv));
        (ahead - behind).norm() / (2.0 * STEP)
    };
    let usable = |value: f64| if value.is_finite() && value > 0.0 { value } else { 1.0 };
    [usable(speed(STEP, 0.0)), usable(speed(0.0, STEP))]
}

fn area(layout: &Layout, l: &Loop) -> f64 {
    let flat: Vec<[f64; 2]> =
        l.uv.iter().map(|uv| [uv.x * layout.scale[0], uv.y * layout.scale[1]]).collect();
    (0..flat.len())
        .map(|i| {
            let (a, b) = (flat[i], flat[(i + 1) % flat.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
}

/// Lays out a loop that does not go round, shifted by whole periods (for a hole: next to
/// `reference`) and by `shift` along u.
fn place_loop(
    layout: &mut Layout,
    l: &Loop,
    shift: f64,
    reference: Option<(Uv, [Option<f64>; 2])>,
) -> Vec<usize> {
    let mut offset = [shift, 0.0];
    if let Some((reference, periods)) = reference {
        for d in 0..2 {
            if let Some(period) = periods[d] {
                offset[d] += ((reference[d] - l.uv[0][d]) / period).round() * period;
            }
        }
    }
    l.uv.iter()
        .zip(&l.ids)
        .map(|(uv, id)| layout.add(Uv::new(uv.x + offset[0], uv.y + offset[1]), *id))
        .collect()
}

/// The points of a straight cut in the parameters from `from` to `to`, its ends left out,
/// close enough together that the surface between them stays within the tolerance.
fn cut_points(surface: &dyn Surface, from: Uv, to: Uv, tolerance: f64, depth: u32, out: &mut Vec<Uv>) {
    let middle = Uv::from(0.5 * (from.coords + to.coords));
    let chord = 0.5 * (surface.point(from).coords + surface.point(to).coords);
    let off = (surface.point(middle).coords - chord).norm();
    if depth < 2 || (depth < DEEPEST && off > tolerance) {
        cut_points(surface, from, middle, tolerance, depth + 1, out);
        out.push(middle);
        cut_points(surface, middle, to, tolerance, depth + 1, out);
    }
}

#[allow(clippy::too_many_arguments)]
fn seam(
    layout: &mut Layout,
    surface: &dyn Surface,
    loops: &[Loop],
    around: &[usize],
    d: usize,
    periods: [Option<f64>; 2],
    points: &mut Vec<Point3>,
    tolerance: f64,
) -> Result<(), String> {
    let (first, second) = (&loops[around[0]], &loops[around[1]]);
    if first.winding[d] != -second.winding[d] {
        return Err("two loops go round the surface the same way".into());
    }
    let Some(period) = periods[d] else {
        return Err("a loop goes round a direction that does not close".into());
    };
    let turn = first.winding[d] as f64 * period;
    let mut end = first.uv[0];
    end[d] += turn;
    // The second loop starts at its point nearest the end of the first along the period.
    let gap = |uv: &Uv| {
        let difference = uv[d] - end[d];
        (difference - (difference / period).round() * period).abs()
    };
    let start =
        (0..second.uv.len()).min_by(|a, b| gap(&second.uv[*a]).total_cmp(&gap(&second.uv[*b]))).unwrap_or(0);
    let mut shift = [0.0, 0.0];
    shift[d] = ((end[d] - second.uv[start][d]) / period).round() * period;
    let mut polygon: Vec<usize> =
        first.uv.iter().zip(&first.ids).map(|(uv, id)| layout.add(*uv, *id)).collect();
    polygon.push(layout.add(end, first.ids[0]));
    let joined = Uv::new(second.uv[start].x + shift[0], second.uv[start].y + shift[1]);
    let mut cut = Vec::new();
    cut_points(surface, end, joined, tolerance, 0, &mut cut);
    let cut_ids: Vec<usize> = cut
        .iter()
        .map(|uv| {
            points.push(surface.point(*uv));
            points.len() - 1
        })
        .collect();
    for (uv, id) in cut.iter().zip(&cut_ids) {
        polygon.push(layout.add(*uv, *id));
    }
    let count = second.uv.len();
    for step in 0..=count {
        let index = (start + step) % count;
        let mut uv = Uv::new(second.uv[index].x + shift[0], second.uv[index].y + shift[1]);
        // Past the end of the list the loop has gone round once.
        if start + step >= count {
            uv[d] += second.winding[d] as f64 * period;
        }
        polygon.push(layout.add(uv, second.ids[index]));
    }
    let back = -turn;
    for (uv, id) in cut.iter().zip(&cut_ids).rev() {
        let mut moved = *uv;
        moved[d] += back;
        polygon.push(layout.add(moved, *id));
    }
    layout.outer = polygon;
    let reference = first.uv[0];
    for (_, other) in loops.iter().enumerate().filter(|(index, _)| !around.contains(index)) {
        let mut middle = reference;
        middle[d] += 0.5 * turn;
        let hole = place_loop(layout, other, 0.0, Some((middle, periods)));
        layout.holes.push(hole);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn pole(
    cx: &Context,
    layout: &mut Layout,
    body: &Arc<Body>,
    face: usize,
    loops: &[Loop],
    around: usize,
    periods: [Option<f64>; 2],
    poles: &[(f64, Point3)],
    points: &mut Vec<Point3>,
) -> Result<(), String> {
    let l = &loops[around];
    let Some(period) = periods[0] else {
        return Err("a loop goes round a direction that does not close".into());
    };
    let reached = poles.iter().find(|(_, at)| {
        matches!(place(cx, &TrimRequest { body: Arc::clone(body), face, point: *at }), krly3d_core::Attempt::Done(found) if found != Place::Outside)
    });
    let Some((v, at)) = reached else {
        return Err("a loop goes round the surface but the face reaches no pole".into());
    };
    let turn = l.winding[0] as f64 * period;
    let mut polygon: Vec<usize> = l.uv.iter().zip(&l.ids).map(|(uv, id)| layout.add(*uv, *id)).collect();
    let mut end = l.uv[0];
    end.x += turn;
    polygon.push(layout.add(end, l.ids[0]));
    points.push(*at);
    let pole_id = points.len() - 1;
    // Along the pole, back to where the loop started, in steps that keep triangles shapely.
    let steps = ((turn.abs() / FRAC_PI_8).ceil() as usize).max(2);
    for step in 0..=steps {
        let u = end.x - turn * step as f64 / steps as f64;
        polygon.push(layout.add(Uv::new(u, *v), pole_id));
    }
    layout.outer = polygon;
    for (_, other) in loops.iter().enumerate().filter(|(index, _)| *index != around) {
        let mut middle = l.uv[0];
        middle.x += 0.5 * turn;
        let hole = place_loop(layout, other, 0.0, Some((middle, periods)));
        layout.holes.push(hole);
    }
    Ok(())
}

/// A face without boundary: the whole rectangle of parameters, its seams laid out on both
/// sides under the same numbers and each pole as one point.
fn whole(
    layout: &mut Layout,
    surface: &dyn Surface,
    periods: [Option<f64>; 2],
    poles: &[(f64, Point3)],
    points: &mut Vec<Point3>,
) -> Result<(), String> {
    let Some(pu) = periods[0] else {
        return Err("a face without boundary on a surface that does not close".into());
    };
    let (low, high, pole_low, pole_high) = match (periods[1], poles) {
        (Some(pv), _) => (0.0, pv, None, None),
        (None, [(a, at_a), (b, at_b)]) => (
            a.min(*b),
            a.max(*b),
            Some(if a < b { *at_a } else { *at_b }),
            Some(if a < b { *at_b } else { *at_a }),
        ),
        _ => {
            return Err(
                "a face without boundary on a surface with neither a second period nor two poles".into()
            )
        }
    };
    let columns = ((pu / FRAC_PI_8).ceil() as usize).max(4);
    let rows = (((high - low) / FRAC_PI_8).ceil() as usize).max(2);
    let id_at = |column: usize,
                 row: usize,
                 points: &mut Vec<Point3>,
                 made: &mut std::collections::BTreeMap<(usize, usize), usize>| {
        let column = column % columns;
        let key = match (row, pole_low, pole_high) {
            (0, Some(_), _) => (0, 0),
            (r, _, Some(_)) if r == rows => (0, rows),
            (r, None, None) if r == rows => (column, 0),
            (r, _, _) => (column, r),
        };
        *made.entry(key).or_insert_with(|| {
            let v = low + (high - low) * key.1 as f64 / rows as f64;
            let point = match (key.1, pole_low, pole_high) {
                (0, Some(at), _) => at,
                (r, _, Some(at)) if r == rows => at,
                _ => surface.point(Uv::new(pu * key.0 as f64 / columns as f64, v)),
            };
            points.push(point);
            points.len() - 1
        })
    };
    let mut made = std::collections::BTreeMap::new();
    let mut polygon = Vec::new();
    let corner = |column: usize, row: usize| {
        Uv::new(pu * column as f64 / columns as f64, low + (high - low) * row as f64 / rows as f64)
    };
    for column in 0..columns {
        let id = id_at(column, 0, points, &mut made);
        polygon.push(layout.add(corner(column, 0), id));
    }
    for row in 0..rows {
        let id = id_at(columns, row, points, &mut made);
        polygon.push(layout.add(corner(columns, row), id));
    }
    for column in (1..=columns).rev() {
        let id = id_at(column, rows, points, &mut made);
        polygon.push(layout.add(corner(column, rows), id));
    }
    for row in (1..=rows).rev() {
        let id = id_at(0, row, points, &mut made);
        polygon.push(layout.add(corner(0, row), id));
    }
    layout.outer = polygon;
    Ok(())
}
