//! Visitor over a body's boundary representation.
//!
//! `walk` is the single traversal of the kernel: it resolves names, carrying
//! surfaces, anchors and bounds once, and hands read-only views to any number
//! of visitors. Layers (naming, perception keys, PMI, export …) never touch
//! the topology arrays themselves.

use crate::brep::{BrepEnvelope, GeometryError, PatchBounds, Surface, SurfaceGeometry};
use crate::math::Point3;
use crate::naming::{edge_ends, edge_faces, edge_names};
use crate::perception::probe::face_anchor;

pub struct BodyView<'a> {
    pub node: &'a str,
    pub brep: &'a BrepEnvelope,
    pub bounds: Option<PatchBounds>,
}

pub struct FaceView<'a> {
    pub id: u32,
    pub name: &'a str,
    pub surface: &'a SurfaceGeometry,
    /// a point on the face and the outward normal there
    pub anchor: Point3,
    pub normal: Point3,
    pub bounds: PatchBounds,
}

pub struct EdgeView<'a> {
    pub id: u32,
    pub name: &'a str,
    /// the two faces it separates (`None` for a free edge), by name
    pub faces: Option<(&'a str, &'a str)>,
    pub face_ids: Option<(u32, u32)>,
    pub ends: Option<(Point3, Point3)>,
}

pub struct VertexView {
    pub id: u32,
    pub position: Point3,
}

/// Every method has an empty default: a visitor overrides what it needs.
pub trait Visitor {
    fn enter_body(&mut self, _body: &BodyView) {}
    fn face(&mut self, _body: &BodyView, _face: &FaceView) {}
    fn edge(&mut self, _body: &BodyView, _edge: &EdgeView) {}
    fn vertex(&mut self, _body: &BodyView, _vertex: &VertexView) {}
    fn leave_body(&mut self, _body: &BodyView) {}
}

pub fn face_bounds(brep: &BrepEnvelope, face: u32) -> Result<PatchBounds, GeometryError> {
    let f = brep
        .topology
        .faces
        .get(face as usize)
        .ok_or_else(|| GeometryError::InvalidTopology("face is missing".into()))?;
    brep.geometry.surface(f.surface)?.enclose(f.trim.uv_bounds)
}

/// Walk one body with any number of visitors (one traversal, many layers).
pub fn walk(
    node: &str,
    brep: &BrepEnvelope,
    visitors: &mut [&mut dyn Visitor],
) -> Result<(), GeometryError> {
    let body = BodyView { node, brep, bounds: brep.bounds_unchecked()? };
    for v in visitors.iter_mut() {
        v.enter_body(&body);
    }
    for face in &brep.topology.faces {
        let (anchor, normal) = face_anchor(brep, face.id)?;
        let view = FaceView {
            id: face.id,
            name: &face.key,
            surface: brep.geometry.surface(face.surface)?,
            anchor,
            normal,
            bounds: face_bounds(brep, face.id)?,
        };
        for v in visitors.iter_mut() {
            v.face(&body, &view);
        }
    }
    let names = edge_names(brep);
    for edge in &brep.topology.edges {
        let ids = edge_faces(brep, edge.id);
        let view = EdgeView {
            id: edge.id,
            name: &names[edge.id as usize],
            faces: ids.map(|(a, b)| {
                (brep.topology.faces[a as usize].key.as_str(), brep.topology.faces[b as usize].key.as_str())
            }),
            face_ids: ids,
            ends: edge_ends(brep, edge.id),
        };
        for v in visitors.iter_mut() {
            v.edge(&body, &view);
        }
    }
    for vertex in &brep.topology.vertices {
        let view = VertexView { id: vertex.id, position: vertex.position };
        for v in visitors.iter_mut() {
            v.vertex(&body, &view);
        }
    }
    for v in visitors.iter_mut() {
        v.leave_body(&body);
    }
    Ok(())
}
