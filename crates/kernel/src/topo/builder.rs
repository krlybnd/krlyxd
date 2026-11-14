//! Building a body from statements a tool can make without case analysis: "this face lies
//! on this surface, its material faces that way, and these edges bound it, with the face
//! on that side of each".

use super::body::{Body, Edge, EdgeId, Face, FaceId, Use, VertexId};
use crate::ports::curve::Curve;
use crate::ports::surface::Surface;
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::{Accuracy, Error};
use std::sync::Arc;

pub struct BodyBuilder {
    accuracy: Accuracy,
    body: Body,
}

/// A boundary edge of a face under construction: seen at the point `at` of the edge, the
/// face lies in the direction `inward` (which need not be exactly tangent to the surface).
/// The point matters on a curved edge, where "towards the face" turns along the edge.
#[derive(Clone, Copy, Debug)]
pub struct Bound {
    pub edge: EdgeId,
    pub at: Point3,
    pub inward: Vec3,
}

impl Bound {
    pub fn new(edge: EdgeId, at: Point3, inward: Vec3) -> Self {
        Self { edge, at, inward }
    }
}

impl BodyBuilder {
    pub fn new(accuracy: &Accuracy) -> Self {
        Self { accuracy: *accuracy, body: Body::default() }
    }

    /// The vertex at `point`; points closer than the length tolerance share one vertex.
    pub fn vertex(&mut self, point: Point3) -> VertexId {
        match self.body.vertices.iter().position(|known| self.accuracy.same_point(known, &point)) {
            Some(known) => known,
            None => {
                self.body.vertices.push(point);
                self.body.vertices.len() - 1
            }
        }
    }

    /// An edge along `curve` between two parameters, running from the smaller to the larger.
    pub fn edge(&mut self, curve: Arc<dyn Curve>, from: f64, to: f64) -> EdgeId {
        let range = (from.min(to), from.max(to));
        let ends = (self.vertex(curve.point(range.0)), self.vertex(curve.point(range.1)));
        self.body.edges.push(Edge { curve, range, ends: Some(ends), label: None });
        self.body.edges.len() - 1
    }

    /// A closed edge: the whole of a closed curve, without a vertex.
    pub fn ring(&mut self, curve: Arc<dyn Curve>) -> Result<EdgeId, Error> {
        let period = curve
            .period()
            .ok_or_else(|| Error::topology(format!("a {} does not close on itself", kind(curve.as_ref()))))?;
        self.body.edges.push(Edge { curve, range: (0.0, period), ends: None, label: None });
        Ok(self.body.edges.len() - 1)
    }

    pub fn label(&mut self, edge: EdgeId, label: &str) {
        self.body.edges[edge].label = Some(label.to_string());
    }

    pub fn edge_data(&self, edge: EdgeId) -> &Edge {
        &self.body.edges[edge]
    }

    /// A face on `surface` whose material side is the one `out` points away from at the
    /// point `at`, bounded by `bounds`. The direction of each edge use and the order of the
    /// uses in their loops are worked out here.
    pub fn face(
        &mut self,
        name: &str,
        surface: Arc<dyn Surface>,
        at: &Point3,
        out: &Vec3,
        bounds: &[Bound],
    ) -> Result<FaceId, Error> {
        let agreement = surface.normal(surface.project(at)).dot(out);
        if agreement == 0.0 || !agreement.is_finite() {
            return Err(Error::topology(format!(
                "the outward direction given for face `{name}` lies in the face"
            )));
        }
        let mut face = Face { name: name.to_string(), surface, outward: agreement > 0.0, loops: Vec::new() };
        let mut uses = Vec::with_capacity(bounds.len());
        for bound in bounds {
            uses.push(Use { edge: bound.edge, forward: self.runs_forward(&face, bound)? });
        }
        face.loops = self.chain(name, uses)?;
        self.body.faces.push(face);
        Ok(self.body.faces.len() - 1)
    }

    pub fn finish(mut self, solid: bool) -> Body {
        self.body.solid = solid;
        self.body
    }

    /// The material of a face is on the left of a boundary edge seen from outside, and the
    /// left of a direction `t` under the outward normal `n` is `n × t`.
    fn runs_forward(&self, face: &Face, bound: &Bound) -> Result<bool, Error> {
        let curve = &self.body.edges[bound.edge].curve;
        let tangent = curve.tangent(curve.project(&bound.at));
        let left = face.outward_normal(&bound.at).cross(&tangent);
        let side = left.dot(&bound.inward);
        if side.abs() <= self.accuracy.angle() * left.norm() * bound.inward.norm() {
            return Err(Error::topology(format!(
                "cannot tell on which side of a {} edge face `{}` lies",
                kind(curve.as_ref()),
                face.name
            )));
        }
        Ok(side > 0.0)
    }

    /// Orders edge uses into closed loops by following shared vertices.
    fn chain(&self, name: &str, mut uses: Vec<Use>) -> Result<Vec<Vec<Use>>, Error> {
        let ends = |used: &Use| {
            self.body.edges[used.edge].ends.map(
                |(start, end)| {
                    if used.forward {
                        (start, end)
                    } else {
                        (end, start)
                    }
                },
            )
        };
        let mut loops = Vec::new();
        while let Some(first) = uses.pop() {
            let Some((start, mut at)) = ends(&first) else {
                loops.push(vec![first]);
                continue;
            };
            let mut current = vec![first];
            while at != start {
                let next = uses
                    .iter()
                    .position(|candidate| ends(candidate).is_some_and(|(from, _)| from == at))
                    .ok_or_else(|| {
                        Error::topology(format!(
                            "the boundary of face `{name}` does not close: nothing continues from {:?}",
                            self.body.vertices[at]
                        ))
                    })?;
                let used = uses.swap_remove(next);
                at = ends(&used).map_or(at, |(_, to)| to);
                current.push(used);
            }
            loops.push(current);
        }
        Ok(loops)
    }
}

fn kind(curve: &dyn Curve) -> &'static str {
    curve.classes().first().copied().unwrap_or("curve")
}
