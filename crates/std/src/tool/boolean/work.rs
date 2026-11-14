//! The shared state of one boolean: the two solids, and the points and edge pieces both
//! are cut into. Every edge piece exists once, so the faces of both solids that end up in
//! the result refer to the same edges and need no matching afterwards.

use krly3d_core::math::Point3;
use krly3d_core::{Accuracy, Attempt, Error};
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::topo::{Body, Edge};
use std::sync::Arc;

/// Why a boolean does not finish.
#[derive(Debug)]
pub(crate) enum Stop {
    /// The solids meet in a way the algorithm does not handle.
    Declined(String),
    Failed(Error),
}

impl From<Error> for Stop {
    fn from(error: Error) -> Self {
        Self::Failed(error)
    }
}

/// The value of an attempt, or the reason to stop.
pub(crate) fn settle<T>(attempt: Attempt<T>) -> Result<T, Stop> {
    match attempt {
        Attempt::Done(value) => Ok(value),
        Attempt::Declined(reason) => Err(Stop::Declined(reason)),
        Attempt::Failed(error) => Err(Stop::Failed(error)),
    }
}

pub(crate) struct Work<'a> {
    pub(crate) accuracy: Accuracy,
    /// The first solid and the second.
    pub(crate) bodies: [&'a Arc<Body>; 2],
    pub(crate) points: Vec<Point3>,
    /// Pieces of the original edges of both solids, and the new edges along sections.
    pub(crate) pieces: Vec<Edge>,
    /// Per solid and original edge: parameters at which the edge is to be divided.
    pub(crate) marks: [Vec<Vec<f64>>; 2],
    /// Per solid and original edge: its pieces in order of parameter.
    pub(crate) split: [Vec<Vec<usize>>; 2],
    /// Per solid and face: the pieces that lie inside the face and divide it.
    pub(crate) cuts: [Vec<Vec<usize>>; 2],
    /// Pairs of faces, one of each solid, that lie on the same surface.
    pub(crate) same: Vec<(usize, usize)>,
}

impl<'a> Work<'a> {
    pub(crate) fn new(accuracy: &Accuracy, bodies: [&'a Arc<Body>; 2]) -> Self {
        let (edges, faces) =
            ([bodies[0].edges.len(), bodies[1].edges.len()], [bodies[0].faces.len(), bodies[1].faces.len()]);
        Self {
            accuracy: *accuracy,
            bodies,
            points: Vec::new(),
            pieces: Vec::new(),
            marks: edges.map(|count| vec![Vec::new(); count]),
            split: edges.map(|count| vec![Vec::new(); count]),
            cuts: faces.map(|count| vec![Vec::new(); count]),
            same: Vec::new(),
        }
    }

    /// The index of the point; points closer than the length tolerance are one point.
    pub(crate) fn point(&mut self, point: Point3) -> usize {
        match self.points.iter().position(|known| self.accuracy.same_point(known, &point)) {
            Some(known) => known,
            None => {
                self.points.push(point);
                self.points.len() - 1
            }
        }
    }

    /// A new piece along `curve`; `whole` for a closed curve taken all the way round.
    pub(crate) fn piece(&mut self, curve: Arc<dyn Curve>, range: (f64, f64), whole: bool) -> usize {
        let ends = if whole {
            None
        } else {
            Some((self.point(curve.point(range.0)), self.point(curve.point(range.1))))
        };
        self.pieces.push(Edge { curve, range, ends, label: None });
        self.pieces.len() - 1
    }

    /// Divides every original edge at its marks.
    pub(crate) fn split_edges(&mut self) {
        for side in [0, 1] {
            for index in 0..self.bodies[side].edges.len() {
                let edge = self.bodies[side].edges[index].clone();
                let mut stops = std::mem::take(&mut self.marks[side][index]);
                stops.sort_by(f64::total_cmp);
                let mut bounds: Vec<f64> = Vec::new();
                let same =
                    |a: f64, b: f64| self.accuracy.same_point(&edge.curve.point(a), &edge.curve.point(b));
                for stop in stops {
                    let at_an_end =
                        edge.ends.is_some() && (same(stop, edge.range.0) || same(stop, edge.range.1));
                    if !at_an_end && !bounds.last().is_some_and(|last| same(*last, stop)) {
                        bounds.push(stop);
                    }
                }
                let whole = edge.ends.is_none() && bounds.is_empty();
                match (edge.ends.is_some(), bounds.first().copied()) {
                    // An open edge keeps its ends; a closed one starts and ends at its first mark.
                    (true, _) => {
                        bounds.insert(0, edge.range.0);
                        bounds.push(edge.range.1);
                    }
                    (false, Some(first)) => bounds.push(first + (edge.range.1 - edge.range.0)),
                    (false, None) => bounds = vec![edge.range.0, edge.range.1],
                }
                let pieces: Vec<usize> = bounds
                    .windows(2)
                    .map(|pair| self.piece(Arc::clone(&edge.curve), (pair[0], pair[1]), whole))
                    .collect();
                self.split[side][index] = pieces;
            }
        }
    }
}
