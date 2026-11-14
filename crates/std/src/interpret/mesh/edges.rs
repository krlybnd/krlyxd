//! Every edge cut into points once, so that both faces beside it share them and the mesh
//! closes without gaps.

use krly3d_core::math::Point3;
use krly3d_kernel::topo::{Body, Edge};

/// Halving steps an edge may take at most: 2^16 pieces is finer than any tolerance needs.
const DEEPEST: u32 = 16;

/// The points of every edge, from its start to its end, as numbers into `points`; a body's
/// vertices keep their own numbers. A closed edge lists its first point again at the end.
/// Neighbouring points are joined by chords that lie within `tolerance` of the curve.
pub(super) fn sample(body: &Body, tolerance: f64, points: &mut Vec<Point3>) -> Vec<Vec<usize>> {
    body.edges
        .iter()
        .map(|edge| {
            let mut parameters = vec![edge.range.0];
            // A closed edge is cut at least four times, so that no two of its chords coincide.
            let shallowest = if edge.ends.is_none() { 2 } else { 0 };
            split(edge, edge.range.0, edge.range.1, tolerance, 0, shallowest, &mut parameters);
            let mut ids = Vec::with_capacity(parameters.len());
            let last = parameters.len() - 1;
            for (index, parameter) in parameters.iter().enumerate() {
                let id = match (edge.ends, index) {
                    (Some((start, _)), 0) => start,
                    (Some((_, end)), at) if at == last => end,
                    (None, at) if at == last => ids[0],
                    _ => {
                        points.push(edge.curve.point(*parameter));
                        points.len() - 1
                    }
                };
                ids.push(id);
            }
            ids
        })
        .collect()
}

/// Appends the parameters after `from` up to and including `to`, halving the stretch until
/// its chord stays within the tolerance at its quarter points.
fn split(edge: &Edge, from: f64, to: f64, tolerance: f64, depth: u32, shallowest: u32, out: &mut Vec<f64>) {
    let (start, end) = (edge.curve.point(from), edge.curve.point(to));
    let off = [0.25, 0.5, 0.75]
        .iter()
        .map(|fraction| {
            (edge.curve.point(from + (to - from) * fraction) - (start + (end - start) * *fraction)).norm()
        })
        .fold(0.0, f64::max);
    if depth < DEEPEST && (depth < shallowest || off > tolerance) {
        let middle = 0.5 * (from + to);
        split(edge, from, middle, tolerance, depth + 1, shallowest, out);
        split(edge, middle, to, tolerance, depth + 1, shallowest, out);
    } else {
        out.push(to);
    }
}
