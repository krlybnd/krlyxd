//! The data of a body. A plain value: what makes it valid is stated in `check`.

use crate::ports::curve::Curve;
use crate::ports::surface::Surface;
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::{Point3, Sim3, Uv, Vec3};
use std::sync::Arc;

/// The parameter within `range` (ascending) of the point of `curve` nearest to `point`:
/// the foot point when it lies within the range, on a closed curve after as many whole
/// turns as it takes, else the nearer end of the range.
pub fn nearest_parameter(curve: &dyn Curve, range: (f64, f64), point: &Point3) -> f64 {
    let mut foot = curve.project(point);
    if let Some(period) = curve.period() {
        foot += ((range.0 - foot) / period).ceil() * period;
    }
    if foot >= range.0 && foot <= range.1 {
        return foot;
    }
    let (start, end) = (curve.point(range.0), curve.point(range.1));
    if (point - start).norm() <= (point - end).norm() {
        range.0
    } else {
        range.1
    }
}

pub type VertexId = usize;
pub type EdgeId = usize;
pub type FaceId = usize;

#[derive(Clone, Debug)]
pub struct Edge {
    pub curve: Arc<dyn Curve>,
    /// Parameter interval on the curve, ascending. The edge runs from the first to the second.
    pub range: (f64, f64),
    /// Start and end vertex, or `None` for a closed edge.
    pub ends: Option<(VertexId, VertexId)>,
    /// A name given by whoever drew the edge. Edges of a solid need none: they are named
    /// by the two faces they separate.
    pub label: Option<String>,
}

impl Edge {
    pub fn point(&self, fraction: f64) -> Point3 {
        self.curve.point(self.parameter(fraction))
    }

    /// The parameter of the point of the edge nearest to `point`: the foot point on the
    /// curve when that lies within the edge, else the nearer end.
    pub fn nearest(&self, point: &Point3) -> f64 {
        nearest_parameter(self.curve.as_ref(), self.range, point)
    }

    /// Parameter at `fraction` of the way from the start (0) to the end (1).
    pub fn parameter(&self, fraction: f64) -> f64 {
        self.range.0 + (self.range.1 - self.range.0) * fraction
    }
}

/// One traversal of an edge by a boundary loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Use {
    pub edge: EdgeId,
    /// Whether the loop runs along the edge from its start to its end.
    pub forward: bool,
}

#[derive(Clone, Debug)]
pub struct Face {
    pub name: String,
    pub surface: Arc<dyn Surface>,
    /// Whether the normal of the surface points out of the material.
    pub outward: bool,
    pub loops: Vec<Vec<Use>>,
}

impl Face {
    /// Unit normal pointing out of the material at the point of the face nearest to `point`.
    pub fn outward_normal(&self, point: &Point3) -> Vec3 {
        let normal = self.surface.normal(self.surface.project(point));
        if self.outward {
            normal
        } else {
            -normal
        }
    }

    pub fn uses(&self) -> impl Iterator<Item = &Use> {
        self.loops.iter().flatten()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Body {
    pub vertices: Vec<Point3>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    /// A solid encloses material; otherwise the body is a sheet such as a profile.
    pub solid: bool,
}

impl Body {
    pub fn face(&self, name: &str) -> Option<(FaceId, &Face)> {
        self.faces.iter().enumerate().find(|(_, face)| face.name == name)
    }

    /// The body after the whole space is moved by `by`. Edge parameters stay valid because
    /// curves keep their parameters when moved.
    pub fn transformed(&self, by: &Sim3) -> Self {
        Self {
            vertices: self.vertices.iter().map(|point| by * point).collect(),
            edges: self
                .edges
                .iter()
                .map(|edge| Edge { curve: edge.curve.transformed(by), ..edge.clone() })
                .collect(),
            faces: self
                .faces
                .iter()
                .map(|face| Face { surface: face.surface.transformed(by), ..face.clone() })
                .collect(),
            solid: self.solid,
        }
    }

    /// The mirror image of the body, or `None` when one of its curves or surfaces cannot be
    /// reflected. A reflection turns space inside out, so every loop runs the other way;
    /// whether a surface's normal still points out of the material is read again from the
    /// geometry, because a reflected surface may carry its normal either way.
    pub fn mirrored(&self, mirror: &Mirror) -> Option<Self> {
        let edges = self
            .edges
            .iter()
            .map(|edge| Some(Edge { curve: edge.curve.mirrored(mirror)?, ..edge.clone() }))
            .collect::<Option<Vec<_>>>()?;
        let faces = self
            .faces
            .iter()
            .map(|face| {
                let surface = face.surface.mirrored(mirror)?;
                let at = face.uses().next().map_or_else(
                    || face.surface.point(Uv::origin()),
                    |used| self.edges[used.edge].point(0.5),
                );
                let image = mirror.point(&at);
                let wanted = mirror.vector(&face.outward_normal(&at));
                Some(Face {
                    name: face.name.clone(),
                    outward: surface.normal(surface.project(&image)).dot(&wanted) > 0.0,
                    surface,
                    loops: face
                        .loops
                        .iter()
                        .map(|uses| {
                            uses.iter().rev().map(|used| Use { forward: !used.forward, ..*used }).collect()
                        })
                        .collect(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            vertices: self.vertices.iter().map(|point| mirror.point(point)).collect(),
            edges,
            faces,
            solid: self.solid,
        })
    }

    /// The same faces bounding the complementary material: every face turned inside out.
    pub fn inverted(&self) -> Self {
        let faces = self.faces.iter().map(|face| Face {
            outward: !face.outward,
            loops: face
                .loops
                .iter()
                .map(|uses| uses.iter().rev().map(|used| Use { forward: !used.forward, ..*used }).collect())
                .collect(),
            ..face.clone()
        });
        Self { faces: faces.collect(), ..self.clone() }
    }

    /// Integral of the outward normal over a face, from its boundary alone.
    pub fn vector_area(&self, face: &Face) -> Vec3 {
        face.uses()
            .map(|used| {
                let edge = &self.edges[used.edge];
                let area = edge.curve.vector_area(edge.range.0, edge.range.1);
                if used.forward {
                    area
                } else {
                    -area
                }
            })
            .sum()
    }

    /// Makes every face name a handle of its own: a name that occurs again gets a running
    /// number, `front`, `front#2`, `front#3`, in the order of the faces.
    pub fn name_apart(&mut self) {
        let mut seen: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for face in &mut self.faces {
            let count = seen.entry(face.name.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                face.name = format!("{}#{count}", face.name);
            }
        }
    }

    /// Several bodies as one, with `rename(index, name)` keeping face names apart.
    pub fn merged(parts: &[Self], rename: impl Fn(usize, &str) -> String) -> Self {
        let mut whole = Self { solid: parts.iter().all(|part| part.solid), ..Self::default() };
        for (index, part) in parts.iter().enumerate() {
            let (vertex_base, edge_base) = (whole.vertices.len(), whole.edges.len());
            whole.vertices.extend(part.vertices.iter().copied());
            whole.edges.extend(part.edges.iter().map(|edge| Edge {
                ends: edge.ends.map(|(start, end)| (start + vertex_base, end + vertex_base)),
                ..edge.clone()
            }));
            whole.faces.extend(part.faces.iter().map(|face| {
                Face {
                    name: rename(index, &face.name),
                    loops: face
                        .loops
                        .iter()
                        .map(|uses| {
                            uses.iter().map(|used| Use { edge: used.edge + edge_base, ..*used }).collect()
                        })
                        .collect(),
                    ..face.clone()
                }
            }));
        }
        whole
    }
}
