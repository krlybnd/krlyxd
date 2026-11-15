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

use super::{Abort, Work};
use krly3d_core::math::{Point3, Uv};
use krly3d_core::{Attempt, ErrorKind};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::ports::trim::{place, Place, TrimRequest};
use krly3d_kernel::topo::Body;
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

    fn fix(&mut self, work: &mut Work<'_>, polygon: &[usize]) -> Result<(), Abort> {
        for index in 0..polygon.len() {
            work.step()?;
            let (a, b) = (polygon[index], polygon[(index + 1) % polygon.len()]);
            self.fixed.insert((a.min(b), a.max(b)));
        }
        Ok(())
    }
}

/// Lays out face `face` of `body`; `edges` are the edges' points as numbers into `points`,
/// to which the points of seams and poles are added.
pub(super) fn lay_out(
    work: &mut Work<'_>,
    body: &Arc<Body>,
    face: usize,
    edges: &[Vec<usize>],
    points: &mut Vec<Point3>,
    tolerance: f64,
) -> Result<Layout, Abort> {
    work.check()?;
    let data = &body.faces[face];
    let surface = data.surface.as_ref();
    let periods = surface.periods();
    let mut poles = Vec::new();
    for v in surface.poles() {
        work.step()?;
        poles.push((v, surface.point(Uv::new(0.0, v))));
    }
    let mut loops = Vec::with_capacity(data.loops.len());
    for uses in &data.loops {
        work.step()?;
        let mut ids = Vec::new();
        for used in uses {
            work.step()?;
            let along = &edges[used.edge];
            for step in 0..along.len() - 1 {
                work.step()?;
                ids.push(along[if used.forward { step } else { along.len() - 1 - step }]);
            }
        }
        let (uv, winding) = unwrapped(work, surface, &ids, points, &poles, periods, tolerance)?;
        loops.push(Loop { ids, uv, winding });
    }
    let centre = if loops.is_empty() {
        Uv::new(periods[0].map_or(0.0, |p| 0.5 * p), 0.0)
    } else {
        let mut sum = nalgebra::Vector2::zeros();
        let mut count = 0;
        for l in &loops {
            work.step()?;
            for uv in &l.uv {
                work.step()?;
                sum += uv.coords;
                count += 1;
            }
        }
        Uv::from(sum / count as f64)
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
    let mut around = Vec::new();
    let mut directions = BTreeSet::new();
    for (index, l) in loops.iter().enumerate() {
        work.step()?;
        if l.winding != [0, 0] {
            around.push(index);
            directions.insert(usize::from(l.winding[0] == 0));
        }
    }
    let direction = match directions.len() {
        0 => None,
        1 => Some(usize::from(loops[around[0]].winding[0] == 0)),
        _ => return Err("boundary loops go round the surface in both directions".into()),
    };
    match (loops.len(), around.len(), direction) {
        (0, _, _) => whole(work, &mut layout, surface, periods, &poles, points)?,
        (_, 0, _) => {
            let areas = loops.iter().map(|l| area(work, &layout, l)).collect::<Result<Vec<_>, _>>()?;
            let outer = (0..loops.len())
                .try_fold(None, |best, i| {
                    work.step()?;
                    Ok::<_, Abort>(Some(match best {
                        Some(b) if areas[i].total_cmp(&areas[b]).is_lt() => b,
                        _ => i,
                    }))
                })?
                .unwrap_or(0);
            layout.outer = place_loop(work, &mut layout, &loops[outer], 0.0, None)?;
            let reference = loops[outer].uv[0];
            for (_, other) in loops.iter().enumerate().filter(|(index, _)| *index != outer) {
                let hole = place_loop(work, &mut layout, other, 0.0, Some((reference, periods)))?;
                layout.holes.push(hole);
            }
        }
        (_, 2, Some(d)) => seam(work, &mut layout, surface, &loops, &around, d, periods, points, tolerance)?,
        (_, 1, Some(0)) => pole(work, &mut layout, body, face, &loops, around[0], periods, &poles, points)?,
        _ => {
            return Err(format!(
                "a face with {} boundary loops, {} of them going round, cannot be laid out flat yet",
                loops.len(),
                around.len()
            )
            .into());
        }
    }
    let (outer, holes) = (layout.outer.clone(), layout.holes.clone());
    layout.fix(work, &outer)?;
    for hole in &holes {
        layout.fix(work, hole)?;
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
#[allow(clippy::too_many_arguments)]
fn unwrapped(
    work: &mut Work<'_>,
    surface: &dyn Surface,
    ids: &[usize],
    points: &[Point3],
    poles: &[(f64, Point3)],
    periods: [Option<f64>; 2],
    tolerance: f64,
) -> Result<(Vec<Uv>, [i64; 2]), Abort> {
    let mut uv: Vec<Uv> = Vec::with_capacity(ids.len());
    for id in ids {
        work.step()?;
        let point = points[*id];
        let mut here = surface.project(&point);
        for (v, at) in poles {
            work.step()?;
            if (at - point).norm() <= tolerance {
                here = Uv::new(uv.last().map_or(here.x, |before| before.x), *v);
                break;
            }
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
    Ok((uv, winding))
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

fn area(work: &mut Work<'_>, layout: &Layout, l: &Loop) -> Result<f64, Abort> {
    let mut flat = Vec::with_capacity(l.uv.len());
    for uv in &l.uv {
        work.step()?;
        flat.push([uv.x * layout.scale[0], uv.y * layout.scale[1]]);
    }
    let mut sum = -0.0;
    for i in 0..flat.len() {
        work.step()?;
        let (a, b) = (flat[i], flat[(i + 1) % flat.len()]);
        sum += a[0] * b[1] - b[0] * a[1];
    }
    Ok(sum.abs())
}

/// Lays out a loop that does not go round, shifted by whole periods (for a hole: next to
/// `reference`) and by `shift` along u.
fn place_loop(
    work: &mut Work<'_>,
    layout: &mut Layout,
    l: &Loop,
    shift: f64,
    reference: Option<(Uv, [Option<f64>; 2])>,
) -> Result<Vec<usize>, Abort> {
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
        .map(|(uv, id)| {
            work.step()?;
            Ok(layout.add(Uv::new(uv.x + offset[0], uv.y + offset[1]), *id))
        })
        .collect()
}

/// The points of a straight cut in the parameters from `from` to `to`, its ends left out,
/// close enough together that the surface between them stays within the tolerance.
fn cut_points(
    work: &mut Work<'_>,
    surface: &dyn Surface,
    from: Uv,
    to: Uv,
    tolerance: f64,
    depth: u32,
    out: &mut Vec<Uv>,
) -> Result<(), Abort> {
    work.step()?;
    let middle = Uv::from(0.5 * (from.coords + to.coords));
    let chord = 0.5 * (surface.point(from).coords + surface.point(to).coords);
    let off = (surface.point(middle).coords - chord).norm();
    if depth < 2 || (depth < DEEPEST && off > tolerance) {
        cut_points(work, surface, from, middle, tolerance, depth + 1, out)?;
        out.push(middle);
        cut_points(work, surface, middle, to, tolerance, depth + 1, out)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn seam(
    work: &mut Work<'_>,
    layout: &mut Layout,
    surface: &dyn Surface,
    loops: &[Loop],
    around: &[usize],
    d: usize,
    periods: [Option<f64>; 2],
    points: &mut Vec<Point3>,
    tolerance: f64,
) -> Result<(), Abort> {
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
    let mut start = 0;
    for index in 0..second.uv.len() {
        work.step()?;
        if gap(&second.uv[index]).total_cmp(&gap(&second.uv[start])).is_lt() {
            start = index;
        }
    }
    let mut shift = [0.0, 0.0];
    shift[d] = ((end[d] - second.uv[start][d]) / period).round() * period;
    let mut polygon: Vec<usize> = first
        .uv
        .iter()
        .zip(&first.ids)
        .map(|(uv, id)| {
            work.step()?;
            Ok(layout.add(*uv, *id))
        })
        .collect::<Result<_, Abort>>()?;
    polygon.push(layout.add(end, first.ids[0]));
    let joined = Uv::new(second.uv[start].x + shift[0], second.uv[start].y + shift[1]);
    let mut cut = Vec::new();
    cut_points(work, surface, end, joined, tolerance, 0, &mut cut)?;
    let cut_ids: Vec<usize> = cut
        .iter()
        .map(|uv| {
            work.step()?;
            points.push(surface.point(*uv));
            Ok(points.len() - 1)
        })
        .collect::<Result<_, Abort>>()?;
    for (uv, id) in cut.iter().zip(&cut_ids) {
        work.step()?;
        polygon.push(layout.add(*uv, *id));
    }
    let count = second.uv.len();
    for step in 0..=count {
        work.step()?;
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
        work.step()?;
        let mut moved = *uv;
        moved[d] += back;
        polygon.push(layout.add(moved, *id));
    }
    layout.outer = polygon;
    let reference = first.uv[0];
    for (_, other) in loops.iter().enumerate().filter(|(index, _)| !around.contains(index)) {
        let mut middle = reference;
        middle[d] += 0.5 * turn;
        let hole = place_loop(work, layout, other, 0.0, Some((middle, periods)))?;
        layout.holes.push(hole);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn pole(
    work: &mut Work<'_>,
    layout: &mut Layout,
    body: &Arc<Body>,
    face: usize,
    loops: &[Loop],
    around: usize,
    periods: [Option<f64>; 2],
    poles: &[(f64, Point3)],
    points: &mut Vec<Point3>,
) -> Result<(), Abort> {
    let l = &loops[around];
    let Some(period) = periods[0] else {
        return Err("a loop goes round a direction that does not close".into());
    };
    let mut reached = None;
    for (v, at) in poles {
        work.check()?;
        // An arbitrary trim plugin can change its answer even in one context.
        // A later budget refusal must not suppress another invocation of that plugin.
        work.cacheable = false;
        let found = place(work.cx, &TrimRequest { body: Arc::clone(body), face, point: *at });
        work.check()?;
        match found {
            Attempt::Done(found) if found != Place::Outside => {
                reached = Some((v, at));
                break;
            }
            Attempt::Failed(error)
                if matches!(error.kind, ErrorKind::Cancelled | ErrorKind::DeadlineExceeded) =>
            {
                return Err(error.into());
            }
            _ => {}
        }
    }
    let Some((v, at)) = reached else {
        return Err("a loop goes round the surface but the face reaches no pole".into());
    };
    let turn = l.winding[0] as f64 * period;
    let mut polygon: Vec<usize> =
        l.uv.iter()
            .zip(&l.ids)
            .map(|(uv, id)| {
                work.step()?;
                Ok(layout.add(*uv, *id))
            })
            .collect::<Result<_, Abort>>()?;
    let mut end = l.uv[0];
    end.x += turn;
    polygon.push(layout.add(end, l.ids[0]));
    points.push(*at);
    let pole_id = points.len() - 1;
    // Along the pole, back to where the loop started, in steps that keep triangles shapely.
    let steps = ((turn.abs() / FRAC_PI_8).ceil() as usize).max(2);
    for step in 0..=steps {
        work.step()?;
        let u = end.x - turn * step as f64 / steps as f64;
        polygon.push(layout.add(Uv::new(u, *v), pole_id));
    }
    layout.outer = polygon;
    for (_, other) in loops.iter().enumerate().filter(|(index, _)| *index != around) {
        let mut middle = l.uv[0];
        middle.x += 0.5 * turn;
        let hole = place_loop(work, layout, other, 0.0, Some((middle, periods)))?;
        layout.holes.push(hole);
    }
    Ok(())
}

/// A face without boundary: the whole rectangle of parameters, its seams laid out on both
/// sides under the same numbers and each pole as one point.
fn whole(
    work: &mut Work<'_>,
    layout: &mut Layout,
    surface: &dyn Surface,
    periods: [Option<f64>; 2],
    poles: &[(f64, Point3)],
    points: &mut Vec<Point3>,
) -> Result<(), Abort> {
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
            );
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
        work.step()?;
        let id = id_at(column, 0, points, &mut made);
        polygon.push(layout.add(corner(column, 0), id));
    }
    for row in 0..rows {
        work.step()?;
        let id = id_at(columns, row, points, &mut made);
        polygon.push(layout.add(corner(columns, row), id));
    }
    for column in (1..=columns).rev() {
        work.step()?;
        let id = id_at(column, rows, points, &mut made);
        polygon.push(layout.add(corner(column, rows), id));
    }
    for row in (1..=rows).rev() {
        work.step()?;
        let id = id_at(0, row, points, &mut made);
        polygon.push(layout.add(corner(0, row), id));
    }
    layout.outer = polygon;
    Ok(())
}
