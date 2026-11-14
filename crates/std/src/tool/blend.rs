//! Chamfer and fillet of straight edges between flat faces, built in place.
//!
//! Across the edge the two faces are half-lines from the edge point `p`, going into the
//! faces along the unit directions `u1` and `u2`, at an angle `θ` to each other. A chamfer
//! of size `d` cuts from `p + d·u1` to `p + d·u2`. A fillet of radius `r` is the ball rolled
//! into the corner: it touches the faces at `t = r / tan(θ/2)` from the edge and its centre
//! lies on the bisector at `r / sin(θ/2)`.
//!
//! The edge is replaced by a band between two new edges, one in each face; each face is
//! shortened to its new edge; and the face closing each end of the edge gets the end of the
//! band — a line, or for a fillet an arc — as a new edge of its own. Nothing is sampled
//! and no boolean is run, so the result is exact and costs a handful of vector operations.
//!
//! Taken: convex straight edges between flat faces, whose ends each meet exactly one more
//! flat face along two straight edges; for a fillet that end face must be square to the
//! edge, where the end of the band is a circular arc. Anything else is declined with the
//! reason.

use crate::curve::{Circle, Line};
use crate::surface::{Cone, Cylinder, Plane, Torus};
use krly3d_core::math::frame::frame;
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Schema};
use krly3d_kernel::topo::{Body, Edge, EdgeId, Face, FaceId, Use};
use krly3d_kernel::{Context, Plugin};
use nalgebra::Translation3;
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Chamfer,
    Fillet,
}

/// One plugin registered once per shape.
pub struct Blend {
    shape: Shape,
}

impl Blend {
    pub fn all() -> [Self; 2] {
        [Self { shape: Shape::Chamfer }, Self { shape: Shape::Fillet }]
    }

    fn name(&self) -> &'static str {
        match self.shape {
            Shape::Chamfer => "chamfer",
            Shape::Fillet => "fillet",
        }
    }
}

impl Plugin<Tool> for Blend {
    fn id(&self) -> Id {
        Id::new(self.name())
    }

    fn schema(&self) -> Option<Schema> {
        let edges = Param::required(
            "edges",
            Kind::Texts,
            "the edges, each named after its two faces in either order: front|top",
        );
        Some(match self.shape {
            Shape::Chamfer => Schema {
                about: "straight edges between flat faces cut back by a flat face named chamfer(a|b)",
                inputs: (1, 1),
                params: vec![
                    edges,
                    Param::required("d", Kind::Number, "how far the cut reaches into each face"),
                ],
            },
            Shape::Fillet => Schema {
                about: "straight edges between flat faces rounded by a cylinder face named fillet(a|b)",
                inputs: (1, 1),
                params: vec![edges, Param::required("r", Kind::Number, "the radius of the rounding")],
            },
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        match self.blend_all(cx, request) {
            Ok(body) => Attempt::Done(body),
            Err(Refusal::Declined(reason)) => Attempt::Declined(reason),
            Err(Refusal::Failed(error)) => Attempt::Failed(error),
        }
    }
}

/// Why an edge was not blended: the case is not built (declined), or the request is wrong.
enum Refusal {
    Declined(String),
    Failed(Error),
}

impl From<Error> for Refusal {
    fn from(error: Error) -> Self {
        Self::Failed(error)
    }
}

fn declined<T>(reason: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal::Declined(reason.into()))
}

fn wrong<T>(reason: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal::Failed(Error::input(reason)))
}

fn broken<T>(reason: &str) -> Result<T, Refusal> {
    Err(Refusal::Failed(Error::geometry(reason)))
}

impl Blend {
    fn blend_all(&self, cx: &Context, request: &ToolRequest) -> Result<Body, Refusal> {
        let [solid] = request.inputs.as_slice() else {
            return wrong("a blend takes one solid");
        };
        if !solid.solid {
            return wrong("a blend takes a solid");
        }
        let size = request
            .params
            .number(if self.shape == Shape::Chamfer { "d" } else { "r" })
            .map_err(Refusal::Failed)?;
        if size <= cx.accuracy().length() {
            return wrong(format!("the size must be positive, got {size}"));
        }
        let names = request.params.texts("edges").map_err(Refusal::Failed)?;
        if names.is_empty() {
            return wrong("name at least one edge");
        }
        let mut body = Body::clone(solid);
        for name in &names {
            let named = |refusal| match refusal {
                Refusal::Declined(reason) => Refusal::Declined(format!("{name}: {reason}")),
                failed => failed,
            };
            if let Some(rim) = rim(cx, &body, name)? {
                body = self.blend_rim(cx, body, &rim, size).map_err(named)?;
                continue;
            }
            let corner = corner(cx, &body, name)?;
            body = self.blend(cx, body, &corner, size).map_err(|refusal| match refusal {
                Refusal::Declined(reason) => Refusal::Declined(format!("{name}: {reason}")),
                failed => failed,
            })?;
        }
        Ok(body)
    }

    fn blend(&self, cx: &Context, mut body: Body, corner: &Corner, size: f64) -> Result<Body, Refusal> {
        let [u1, u2] = corner.into;
        let half = 0.5 * u1.dot(&u2).clamp(-1.0, 1.0).acos();
        let reach = match self.shape {
            Shape::Chamfer => size,
            Shape::Fillet => size / half.tan(),
        };
        let ends = [end(cx, &body, corner, 0, reach)?, end(cx, &body, corner, 1, reach)?];
        if self.shape == Shape::Fillet && ends.iter().any(|end| !end.square) {
            return declined(
                "a face closing an end of the edge is not square to it; the fillet would end in an ellipse, for which there is no curve yet",
            );
        }
        let surface = self.band_surface(corner, &ends, size, half)?;

        // The vertices of the edge move into the first face; the second face gets new ones.
        let first_new = body.vertices.len();
        for end in &ends {
            body.vertices[end.vertex] = end.points[0];
            body.vertices.push(end.points[1]);
        }
        let in_second = |which: usize| first_new + which;
        for (which, end) in ends.iter().enumerate() {
            shorten(cx, &mut body, end.edges[0], end.vertex, end.vertex, &end.points[0])?;
            shorten(cx, &mut body, end.edges[1], end.vertex, in_second(which), &end.points[1])?;
        }

        // The edge itself becomes the border of the band in the first face; the border in
        // the second face is new.
        let first_border = corner.edge;
        body.edges[first_border] =
            straight(&ends[0].points[0], &ends[1].points[0], (ends[0].vertex, ends[1].vertex))?;
        body.edges.push(straight(&ends[0].points[1], &ends[1].points[1], (in_second(0), in_second(1)))?);
        let second_border = body.edges.len() - 1;
        for used in body.faces[corner.faces[1]].loops.iter_mut().flatten() {
            if used.edge == first_border {
                used.edge = second_border;
            }
        }

        // Across each end, a line or an arc from the first face's point to the second's,
        // put into the end face between the two edges it now joins.
        let mut across = [0; 2];
        for (which, end) in ends.iter().enumerate() {
            let joins = (end.vertex, in_second(which));
            body.edges.push(match self.shape {
                Shape::Chamfer => straight(&end.points[0], &end.points[1], joins)?,
                Shape::Fillet => arc(&surface, &end.points, joins)?,
            });
            across[which] = body.edges.len() - 1;
            insert_between(&mut body, end.face, end.edges, across[which])?;
        }

        // The band: each of its four edges used the other way from the face beside it.
        let mut band = vec![
            opposite(&body, corner.faces[0], first_border)?,
            opposite(&body, corner.faces[1], second_border)?,
            opposite(&body, ends[0].face, across[0])?,
            opposite(&body, ends[1].face, across[1])?,
        ];
        chain(&body, &mut band)?;
        let middle = body.edges[first_border].point(0.5);
        let outward = surface.normal(surface.project(&middle)).dot(&(u1 + u2)) < 0.0;
        body.faces.push(Face {
            name: format!("{}({})", self.name(), corner.name),
            surface,
            outward,
            loops: vec![band],
        });
        Ok(body)
    }

    /// The plane through the chamfer's four corners, or the cylinder of the rolled ball.
    fn band_surface(
        &self,
        corner: &Corner,
        ends: &[End; 2],
        size: f64,
        half: f64,
    ) -> Result<Arc<dyn Surface>, Refusal> {
        let [u1, u2] = corner.into;
        Ok(match self.shape {
            Shape::Chamfer => {
                let across = ends[0].points[1] - ends[0].points[0];
                let normal = corner.along.cross(&across);
                Arc::new(Plane::new(frame(&ends[0].points[0], &normal, &across).map_err(Refusal::Failed)?))
            }
            Shape::Fillet => {
                let centre = corner.start + (u1 + u2).normalize() * (size / half.sin());
                let axis = frame(&centre, &corner.along, &u1).map_err(Refusal::Failed)?;
                Arc::new(Cylinder::new(axis, size).map_err(Refusal::Failed)?)
            }
        })
    }
}

/// A circular edge all the way round between a flat face and a cylinder square to it, as
/// at the rim of a boss or of a hole.
struct Rim {
    edge: EdgeId,
    /// The flat face and the cylinder.
    cap: FaceId,
    side: FaceId,
    circle: Circle,
    /// Unit normal of the flat face, out of the material.
    out: Vec3,
    /// `+1` when the material is inside the cylinder (a boss), `−1` when outside (a hole).
    inside: f64,
    name: String,
}

/// The named edge when it is a rim, `None` when it is some other edge.
fn rim(cx: &Context, body: &Body, name: &str) -> Result<Option<Rim>, Refusal> {
    let wanted = ordered(name);
    let found = (0..body.edges.len()).find(|edge| {
        let faces = faces_of(body, *edge);
        faces.len() == 2
            && ordered(&format!("{}|{}", body.faces[faces[0]].name, body.faces[faces[1]].name)) == wanted
    });
    let Some(edge) = found else { return Ok(None) };
    let data = &body.edges[edge];
    let Some(circle) = data.curve.as_any().downcast_ref::<Circle>() else { return Ok(None) };
    if data.ends.is_some() {
        return declined("only a circular edge all the way round is blended so far, not an arc");
    }
    let faces = faces_of(body, edge);
    let is = |face: FaceId, flat: bool| {
        let surface = &body.faces[face].surface;
        if flat {
            surface.as_any().downcast_ref::<Plane>().is_some()
        } else {
            surface.as_any().downcast_ref::<Cylinder>().is_some()
        }
    };
    let (cap, side) = match (faces[0], faces[1]) {
        (a, b) if is(a, true) && is(b, false) => (a, b),
        (a, b) if is(b, true) && is(a, false) => (b, a),
        _ => return declined("a circular edge is blended only between a flat face and a cylinder"),
    };
    let rim_point = circle.point(0.0);
    let out = body.faces[cap].outward_normal(&rim_point);
    let accuracy = cx.accuracy();
    let Some(cylinder) = body.faces[side].surface.as_any().downcast_ref::<Cylinder>() else {
        return declined("a circular edge is blended only between a flat face and a cylinder");
    };
    if !accuracy.parallel(&out, &circle.axis())
        || !accuracy.is_zero_length(cylinder.radius() - circle.radius())
    {
        return declined("the cylinder is not square to the flat face");
    }
    // A boss's side faces away from its axis; a hole's side faces towards it.
    let away = rim_point - circle.center();
    let inside = if body.faces[side].outward_normal(&rim_point).dot(&away) > 0.0 { 1.0 } else { -1.0 };
    Ok(Some(Rim { edge, cap, side, circle: *circle, out, inside, name: wanted }))
}

impl Blend {
    /// A rim cut back to a cone or rounded to a quarter of a torus: the flat face's circle
    /// moves in (on a boss) or out (on a hole) by the reach, the cylinder's moves down by it.
    fn blend_rim(&self, cx: &Context, mut body: Body, rim: &Rim, size: f64) -> Result<Body, Refusal> {
        let accuracy = cx.accuracy();
        let (centre, radius, out) = (rim.circle.center(), rim.circle.radius(), rim.out);
        let flat_radius = radius - rim.inside * size;
        if flat_radius <= accuracy.length() {
            return declined(format!("the {} is as wide as the boss", self.name()));
        }
        // How far the cylinder reaches down from the rim: its other edges set it.
        let depth = body.faces[rim.side]
            .uses()
            .filter(|used| used.edge != rim.edge)
            .flat_map(|used| [0.0, 0.25, 0.5, 0.75].map(|fraction| body.edges[used.edge].point(fraction)))
            .map(|point| (centre - point).dot(&out))
            .fold(f64::INFINITY, f64::min);
        if depth <= size + accuracy.length() {
            return declined(format!(
                "the {} reaches across face `{}`",
                self.name(),
                body.faces[rim.side].name
            ));
        }
        let level = |shift: f64| Translation3::from(out * shift) * *rim.circle.frame();
        let on_cap = Circle::new(level(0.0), flat_radius)?;
        let on_side = Circle::new(level(-size), radius)?;
        let surface: Arc<dyn Surface> = match self.shape {
            Shape::Chamfer => {
                // Radius `flat_radius` at the rim's height, `radius` a reach below.
                let slope = rim.inside;
                let apex = centre + out * (flat_radius * slope);
                let opening = -out * slope;
                Arc::new(Cone::new(
                    krly3d_core::math::frame::frame_about(&apex, &opening)?,
                    std::f64::consts::FRAC_PI_4,
                )?)
            }
            Shape::Fillet => {
                let ring = frame(&(centre - out * size), &out, &(rim.circle.point(0.0) - centre))?;
                if flat_radius <= size {
                    return declined("the fillet is wider than the boss is round");
                }
                Arc::new(Torus::new(ring, flat_radius, size)?)
            }
        };
        body.edges[rim.edge] = Edge { curve: Arc::new(on_cap), range: (0.0, TAU), ends: None, label: None };
        body.edges.push(Edge { curve: Arc::new(on_side), range: (0.0, TAU), ends: None, label: None });
        let lowered = body.edges.len() - 1;
        for used in body.faces[rim.side].loops.iter_mut().flatten() {
            if used.edge == rim.edge {
                used.edge = lowered;
            }
        }
        let band = vec![vec![opposite(&body, rim.cap, rim.edge)?], vec![opposite(&body, rim.side, lowered)?]];
        // Out of the material is towards the corner that was cut away: up and, on a boss,
        // away from the axis, on a hole towards it.
        let across = (rim.circle.point(0.0) - centre).normalize();
        let towards_corner = (out + across * rim.inside).normalize();
        let middle = match self.shape {
            Shape::Chamfer => centre + across * (0.5 * (flat_radius + radius)) - out * (0.5 * size),
            Shape::Fillet => centre + across * flat_radius - out * size + towards_corner * size,
        };
        let outward = surface.normal(surface.project(&middle)).dot(&towards_corner) > 0.0;
        body.faces.push(Face {
            name: format!("{}({})", self.name(), rim.name),
            surface,
            outward,
            loops: band,
        });
        Ok(body)
    }
}

/// The edge to blend, read across: where it is and which way the faces go.
struct Corner {
    edge: EdgeId,
    /// Where the edge starts, and its unit direction from start to end.
    start: Point3,
    along: Vec3,
    /// Unit directions from the edge into each face, square to the edge.
    into: [Vec3; 2],
    faces: [FaceId; 2],
    /// The edge name with the face names in alphabetical order.
    name: String,
}

/// One end of the edge to blend.
struct End {
    /// The vertex of the edge at this end.
    vertex: usize,
    /// The other edge at this end lying in each of the two faces.
    edges: [EdgeId; 2],
    /// The face that closes this end.
    face: FaceId,
    /// Where the band meets each of those edges.
    points: [Point3; 2],
    /// Whether the end face is square to the edge.
    square: bool,
}

/// Finds the named edge and reads its corner, declining edges that are not straight, not
/// between two flat faces, or not convex.
fn corner(cx: &Context, body: &Body, name: &str) -> Result<Corner, Refusal> {
    let wanted = ordered(name);
    let found: Vec<(EdgeId, Vec<FaceId>)> = (0..body.edges.len())
        .map(|edge| (edge, faces_of(body, edge)))
        .filter(|(_, faces)| {
            faces.len() == 2
                && ordered(&format!("{}|{}", body.faces[faces[0]].name, body.faces[faces[1]].name)) == wanted
        })
        .collect();
    let (edge, faces) = match found.as_slice() {
        [one] => one.clone(),
        [] => return wrong(format!("there is no edge `{name}`")),
        _ => return wrong(format!("`{name}` names more than one edge")),
    };
    let data = &body.edges[edge];
    if data.curve.as_any().downcast_ref::<Line>().is_none() || data.ends.is_none() {
        return declined("only straight edges are blended so far");
    }
    let (start, end) = (data.point(0.0), data.point(1.0));
    let along = (end - start).normalize();
    let mut into = [Vec3::zeros(); 2];
    for (slot, face) in into.iter_mut().zip(&faces) {
        let face = &body.faces[*face];
        if face.surface.as_any().downcast_ref::<Plane>().is_none() {
            return declined(format!("face `{}` is not flat", face.name));
        }
        let Some(used) = face.uses().find(|used| used.edge == edge) else {
            return broken("a face lost its edge");
        };
        // Seen from outside, the face lies to the left of the way its boundary runs.
        let travel = if used.forward { along } else { -along };
        *slot = face.outward_normal(&start).cross(&travel).normalize();
    }
    let bend = into[0].dot(&body.faces[faces[1]].outward_normal(&start));
    if bend.abs() <= cx.accuracy().angle() {
        return declined("the faces meet flat; there is no corner");
    }
    if bend > 0.0 {
        return declined("the edge is concave; filling a corner is not built yet");
    }
    Ok(Corner { edge, start, along, into, faces: [faces[0], faces[1]], name: wanted })
}

/// The faces whose boundary uses the edge.
fn faces_of(body: &Body, edge: EdgeId) -> Vec<FaceId> {
    (0..body.faces.len()).filter(|face| body.faces[*face].uses().any(|used| used.edge == edge)).collect()
}

/// Reads one end (`which` 0 at the start, 1 at the end) and where the band meets it.
fn end(cx: &Context, body: &Body, corner: &Corner, which: usize, reach: f64) -> Result<End, Refusal> {
    let Some(ends) = body.edges[corner.edge].ends else {
        return declined("the edge is closed");
    };
    let vertex = if which == 0 { ends.0 } else { ends.1 };
    let at = body.vertices[vertex];
    let meeting: Vec<EdgeId> = (0..body.edges.len())
        .filter(|edge| *edge != corner.edge)
        .filter(|edge| body.edges[*edge].ends.is_some_and(|(from, to)| from == vertex || to == vertex))
        .collect();
    if meeting.len() != 2 {
        return declined("an end of the edge meets more than one other face");
    }
    let mut edges = [0; 2];
    let mut points = [Point3::origin(); 2];
    for side in 0..2 {
        let face = corner.faces[side];
        let Some(edge) =
            meeting.iter().copied().find(|edge| body.faces[face].uses().any(|used| used.edge == *edge))
        else {
            return declined("an end of the edge meets more than one other face");
        };
        let data = &body.edges[edge];
        if data.curve.as_any().downcast_ref::<Line>().is_none() {
            return declined("an edge at an end of the edge is not straight");
        }
        let far =
            if data.ends.is_some_and(|(from, _)| from == vertex) { data.point(1.0) } else { data.point(0.0) };
        let leaving = (far - at).normalize();
        let into = leaving.dot(&corner.into[side]);
        if into <= cx.accuracy().angle() {
            return declined("an edge at an end of the edge does not lead into its face");
        }
        // Along that edge as far as it takes to be `reach` from the blended edge.
        let distance = reach / into;
        if distance >= (far - at).norm() - cx.accuracy().length() {
            return declined(format!("the blend reaches across face `{}`", body.faces[face].name));
        }
        edges[side] = edge;
        points[side] = at + leaving * distance;
    }
    let closing: Vec<FaceId> = (0..body.faces.len())
        .filter(|face| {
            let uses = |edge: EdgeId| body.faces[*face].uses().any(|used| used.edge == edge);
            uses(edges[0]) && uses(edges[1])
        })
        .collect();
    let [face] = closing.as_slice() else {
        return declined("the two edges at an end do not close one face");
    };
    let Some(plane) = body.faces[*face].surface.as_any().downcast_ref::<Plane>() else {
        return declined(format!("face `{}` at an end of the edge is not flat", body.faces[*face].name));
    };
    let square = cx.accuracy().parallel(&plane.normal(plane.project(&at)), &corner.along);
    Ok(End { vertex, edges, face: *face, points, square })
}

/// Moves the end of `edge` that was at `was` to the point `to`, now vertex `vertex`.
fn shorten(
    cx: &Context,
    body: &mut Body,
    edge: EdgeId,
    was: usize,
    vertex: usize,
    to: &Point3,
) -> Result<(), Refusal> {
    let data = &mut body.edges[edge];
    let at = data.curve.project(to);
    match data.ends {
        Some((from, end)) if from == was => {
            data.range.0 = at;
            data.ends = Some((vertex, end));
        }
        Some((from, end)) if end == was => {
            data.range.1 = at;
            data.ends = Some((from, vertex));
        }
        _ => return broken("an edge at an end of the blend lost its vertex"),
    }
    if data.range.1 - data.range.0 <= cx.accuracy().length() {
        return declined("the blend uses up a whole edge next to it");
    }
    Ok(())
}

/// A straight edge from one point to another, between the given vertices.
fn straight(from: &Point3, to: &Point3, ends: (usize, usize)) -> Result<Edge, Refusal> {
    let line = Line::new(*from, to - from).map_err(Refusal::Failed)?;
    Ok(Edge { curve: Arc::new(line), range: (0.0, 1.0), ends: Some(ends), label: None })
}

/// The arc of the fillet's cylinder from one point to the other, the short way round.
fn arc(surface: &Arc<dyn Surface>, points: &[Point3; 2], ends: (usize, usize)) -> Result<Edge, Refusal> {
    let Some(cylinder) = surface.as_any().downcast_ref::<Cylinder>() else {
        return broken("a fillet without its cylinder");
    };
    let base = *cylinder.frame();
    let height = base.inverse_transform_point(&points[0]).z;
    let circle = Circle::new(base * Translation3::new(0.0, 0.0, height), cylinder.radius())
        .map_err(Refusal::Failed)?;
    let (from, to) = (circle.project(&points[0]), circle.project(&points[1]));
    let sweep = (to - from).rem_euclid(TAU);
    let (range, ends) =
        if sweep < PI { ((from, from + sweep), ends) } else { ((to, to + TAU - sweep), (ends.1, ends.0)) };
    Ok(Edge { curve: Arc::new(circle), range, ends: Some(ends), label: None })
}

/// The use of an edge running the other way from its use in `face`.
fn opposite(body: &Body, face: FaceId, edge: EdgeId) -> Result<Use, Refusal> {
    match body.faces[face].uses().find(|used| used.edge == edge) {
        Some(used) => Ok(Use { edge, forward: !used.forward }),
        None => broken("a face lost an edge of the blend"),
    }
}

fn start_of(body: &Body, used: &Use) -> Option<usize> {
    body.edges[used.edge].ends.map(|(from, to)| if used.forward { from } else { to })
}

fn end_of(body: &Body, used: &Use) -> Option<usize> {
    body.edges[used.edge].ends.map(|(from, to)| if used.forward { to } else { from })
}

/// Puts the new edge into the loop of `face` between the two edges it joins.
fn insert_between(body: &mut Body, face: FaceId, joined: [EdgeId; 2], new: EdgeId) -> Result<(), Refusal> {
    let Some(new_ends) = body.edges[new].ends else {
        return broken("the end of a blend is a closed edge");
    };
    for index in 0..body.faces[face].loops.len() {
        let uses = &body.faces[face].loops[index];
        let count = uses.len();
        for position in 0..count {
            let (arriving, leaving) = (uses[position], uses[(position + 1) % count]);
            let pair = [arriving.edge, leaving.edge];
            if pair != joined && pair != [joined[1], joined[0]] {
                continue;
            }
            let forward = match (end_of(body, &arriving), start_of(body, &leaving)) {
                (Some(from), Some(to)) if new_ends == (from, to) => true,
                (Some(from), Some(to)) if new_ends == (to, from) => false,
                _ => return broken("the end of a blend does not join its edges"),
            };
            body.faces[face].loops[index].insert(position + 1, Use { edge: new, forward });
            return Ok(());
        }
    }
    declined("the two edges at an end are not next to each other in the face that closes it")
}

/// Orders the uses of a loop so that each starts where the one before ends.
fn chain(body: &Body, uses: &mut [Use]) -> Result<(), Refusal> {
    for position in 1..uses.len() {
        let wanted = end_of(body, &uses[position - 1]);
        let Some(next) = (position..uses.len()).find(|index| start_of(body, &uses[*index]) == wanted) else {
            return broken("the edges of the blend do not close a loop");
        };
        uses.swap(position, next);
    }
    Ok(())
}

/// An edge name with its two face names in alphabetical order, so that `top|front` and
/// `front|top` are one edge.
fn ordered(name: &str) -> String {
    let mut parts: Vec<&str> = name.split('|').map(str::trim).collect();
    parts.sort_unstable();
    parts.join("|")
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, face_names, made, volume};
    use krly3d_core::{Attempt, Id};
    use krly3d_kernel::ports::tool::run;
    use krly3d_kernel::schema::{Params, Value};
    use krly3d_kernel::topo::Body;
    use krly3d_kernel::Context;
    use std::f64::consts::PI;
    use std::sync::Arc;

    fn block(cx: &Context) -> Arc<Body> {
        made(cx, "box", Params::new().with("w", 60.0).with("d", 40.0).with("h", 10.0), &[])
    }

    fn blend(tool: &str, names: &[&str], size: f64) -> Params {
        let key = if tool == "chamfer" { "d" } else { "r" };
        Params::new().with("edges", Value::texts(names.iter().copied())).with(key, size)
    }

    #[test]
    fn a_chamfer_cuts_the_edge_back_to_a_flat_face_named_after_it() {
        let cx = context();

        let cut = made(&cx, "chamfer", blend("chamfer", &["top|front"], 2.0), &[block(&cx)]);

        assert_eq!(
            face_names(&cut),
            ["back", "bottom", "chamfer(front|top)", "front", "left", "right", "top"]
        );
        assert!(agree(volume(&cx, &cut), 24_000.0 - 0.5 * 4.0 * 60.0));
        let lost = 2.0 * 60.0 * 2.0 + 2.0 * 0.5 * 4.0;
        assert!(agree(area(&cx, &cut), 6_800.0 - lost + 60.0 * 8.0_f64.sqrt()));
    }

    #[test]
    fn a_fillet_rounds_the_edge_with_a_quarter_cylinder() {
        let cx = context();

        let round = made(&cx, "fillet", blend("fillet", &["front|top"], 3.0), &[block(&cx)]);

        assert_eq!(
            face_names(&round),
            ["back", "bottom", "fillet(front|top)", "front", "left", "right", "top"]
        );
        let corner = 9.0 - 9.0 * PI / 4.0;
        assert!(agree(volume(&cx, &round), 24_000.0 - corner * 60.0));
        let lost = 2.0 * 60.0 * 3.0 + 2.0 * corner;
        assert!(agree(area(&cx, &round), 6_800.0 - lost + 60.0 * 1.5 * PI));
    }

    #[test]
    fn parallel_edges_are_blended_one_after_the_other() {
        let cx = context();

        let round = made(&cx, "fillet", blend("fillet", &["front|top", "back|top"], 2.0), &[block(&cx)]);

        assert!(agree(volume(&cx, &round), 24_000.0 - 2.0 * (4.0 - PI) * 60.0));
    }

    #[test]
    fn an_upright_edge_is_rounded_between_the_top_and_bottom_faces() {
        let cx = context();

        let round = made(&cx, "fillet", blend("fillet", &["front|left"], 5.0), &[block(&cx)]);

        assert!(agree(volume(&cx, &round), 24_000.0 - (25.0 - 25.0 * PI / 4.0) * 10.0));
    }

    #[test]
    fn a_second_chamfer_ends_on_the_face_of_the_first() {
        let cx = context();
        let first = made(&cx, "chamfer", blend("chamfer", &["front|top"], 1.0), &[block(&cx)]);

        let cut = made(&cx, "chamfer", blend("chamfer", &["front|left"], 0.5), &[first]);

        // With a, b, c the distances from the left, front and top faces, the second chamfer
        // takes a + b < 1/2 where the first left b + c >= 1: the integral of 9 + b over
        // that triangle of area 1/8.
        assert_eq!(cut.faces.len(), 8);
        let second = 9.0 * 0.125 + 0.125 / 6.0;
        assert!(agree(volume(&cx, &cut), 24_000.0 - 30.0 - second));
    }

    #[test]
    fn the_rim_of_a_boss_is_rounded_by_a_quarter_torus_and_cut_back_by_a_cone() {
        let cx = context();
        let boss = || made(&cx, "cylinder", Params::new().with("r", 5.0).with("h", 8.0), &[]);

        let round = made(&cx, "fillet", blend("fillet", &["top|side"], 1.0), &[boss()]);
        let cut = made(&cx, "chamfer", blend("chamfer", &["side|top"], 1.0), &[boss()]);

        // Pappus: the unit square at the corner (centroid 4.5 from the axis) less the quarter
        // disc of the fillet (centroid 4 + 4/3π), turned about the axis; and the triangle of
        // a 45° chamfer (area 1/2, centroid 5 − 1/3).
        let removed = 2.0 * PI * (4.5 - PI / 4.0 * (4.0 + 4.0 / (3.0 * PI)));
        assert_eq!(face_names(&round), ["bottom", "fillet(side|top)", "side", "top"]);
        assert!(agree(volume(&cx, &round), 200.0 * PI - removed), "{}", volume(&cx, &round));
        assert!(agree(volume(&cx, &cut), 200.0 * PI - 2.0 * PI * (5.0 - 1.0 / 3.0) * 0.5));
    }

    #[test]
    fn the_rim_of_a_hole_is_rounded_outward_into_the_plate() {
        let cx = context();
        let outline = Params::new().with("path", "M -10,-10 L 10,-10 L 10,10 L -10,10 Z O 0,0 3");
        let plate = made(&cx, "extrude", Params::new().with("h", 4.0), &[made(&cx, "profile", outline, &[])]);
        let rim = plate
            .faces
            .iter()
            .find(|face| {
                face.name != "top"
                    && face.name != "bottom"
                    && face.surface.as_any().downcast_ref::<crate::surface::Cylinder>().is_some()
            })
            .unwrap()
            .name
            .clone();

        let cut = made(&cx, "chamfer", blend("chamfer", &[&format!("top|{rim}")], 1.0), &[plate]);

        // The triangle of the chamfer turned about the axis at 3 + 1/3.
        assert!(
            agree(volume(&cx, &cut), 4.0 * (400.0 - 9.0 * PI) - 2.0 * PI * (3.0 + 1.0 / 3.0) * 0.5),
            "{}",
            volume(&cx, &cut)
        );
    }

    #[test]
    fn a_fillet_ending_on_a_rounded_face_is_declined() {
        let cx = context();
        let round = made(&cx, "fillet", blend("fillet", &["front|top"], 2.0), &[block(&cx)]);

        let attempt = run(&cx, &Id::new("fillet"), &blend("fillet", &["front|left"], 1.0), &[round]);

        assert!(
            matches!(&attempt, Attempt::Declined(reason) if reason.contains("front|left: an edge at an end of the edge is not straight")),
            "{attempt:?}"
        );
    }

    #[test]
    fn a_blend_wider_than_its_face_is_declined() {
        let cx = context();

        let attempt = run(&cx, &Id::new("chamfer"), &blend("chamfer", &["front|top"], 12.0), &[block(&cx)]);

        assert!(
            matches!(&attempt, Attempt::Declined(reason) if reason.contains("reaches across")),
            "{attempt:?}"
        );
    }

    #[test]
    fn an_edge_that_is_not_there_is_an_input_error() {
        let cx = context();

        let attempt = run(&cx, &Id::new("fillet"), &blend("fillet", &["front|back"], 1.0), &[block(&cx)]);

        assert!(matches!(attempt, Attempt::Failed(_)));
    }
}
