//! The result of a boolean: the pieces that are kept, as one body.

use super::work::Work;
use krly3d_kernel::topo::{Body, Face, Use};

/// A piece of a face that goes into the result.
pub(crate) struct Kept {
    pub(crate) face: Face,
    /// Whether the piece bounds the result from its other side.
    pub(crate) turned: bool,
    pub(crate) name: String,
}

/// Pieces of both solids already refer to the same edges and points, so sewing is only
/// leaving out what no kept piece uses.
pub(crate) fn assemble(work: &Work, kept: Vec<Kept>) -> Body {
    let mut edge_of = vec![None; work.pieces.len()];
    let mut vertex_of = vec![None; work.points.len()];
    let mut body = Body { solid: true, ..Body::default() };
    for piece in kept {
        let mut loops = Vec::with_capacity(piece.face.loops.len());
        for uses in &piece.face.loops {
            let mut renumbered = Vec::with_capacity(uses.len());
            for used in uses {
                let edge = *edge_of[used.edge].get_or_insert_with(|| {
                    let mut edge = work.pieces[used.edge].clone();
                    edge.ends = edge.ends.map(|(start, end)| {
                        let mut renumber = |vertex: usize| {
                            *vertex_of[vertex].get_or_insert_with(|| {
                                body.vertices.push(work.points[vertex]);
                                body.vertices.len() - 1
                            })
                        };
                        (renumber(start), renumber(end))
                    });
                    body.edges.push(edge);
                    body.edges.len() - 1
                });
                renumbered.push(Use { edge, forward: used.forward != piece.turned });
            }
            if piece.turned {
                renumbered.reverse();
            }
            loops.push(renumbered);
        }
        body.faces.push(Face {
            name: piece.name,
            outward: piece.face.outward != piece.turned,
            loops,
            ..piece.face
        });
    }
    // A face divided into several kept pieces, or two tools with one name, repeat names.
    body.name_apart();
    body
}
