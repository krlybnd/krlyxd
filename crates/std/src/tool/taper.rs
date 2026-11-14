//! A profile pushed along its normal while its sides lean in (or out) by a draft angle.
//!
//! At height `z` every point of the outline has moved `z·tan θ` towards the inside of the
//! profile. A straight side therefore sweeps a plane, and an arc of radius `ρ` a cone
//! whose radius changes by `tan θ` per unit of height; the top is the outline offset by
//! `h·tan θ`. Where two straight sides meet, the corner runs up the line in which their
//! planes cross; where a side meets the next tangentially, the corner moves straight
//! inward. A corner between an arc and a side that is not tangent to it would run up a
//! conic, for which there is no curve yet, and is declined, as is an outline that the
//! offset closes up before the top.

use crate::curve::{Circle, Line};
use crate::surface::{Cone, Plane};
use krly3d_core::math::frame::{frame, frame_about};
use krly3d_core::math::{Point3, Sim3, Vec3};
use krly3d_core::{Accuracy, Attempt, Error, Id};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Schema};
use krly3d_kernel::topo::{Body, BodyBuilder, Bound, Edge, EdgeId, Use};
use krly3d_kernel::{Context, Plugin};
use nalgebra::{Rotation3, Translation3};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct Taper;

impl Plugin<Tool> for Taper {
    fn id(&self) -> Id {
        Id::new("taper")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a solid made by pushing a profile along the normal of its plane while its sides lean in by `angle`; faces are named as extrude names them",
            inputs: (1, 1),
            params: vec![
                Param::required("h", Kind::Number, "how far to push, above zero"),
                Param::required(
                    "angle",
                    Kind::Number,
                    "draft angle in degrees between a side and the push; positive narrows the solid, negative widens it",
                ),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        match taper(cx, request) {
            Ok(body) => Attempt::Done(body),
            Err(Refusal::Declined(reason)) => Attempt::Declined(reason),
            Err(Refusal::Failed(error)) => Attempt::Failed(error),
        }
    }
}

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

/// What the push does to the plane of the profile.
struct Push {
    /// Unit normal of the profile plane, the direction of the push.
    normal: Vec3,
    height: f64,
    /// How far the outline moves inward over the whole height: `h·tan θ`.
    inset: f64,
}

impl Push {
    fn lift(&self) -> Vec3 {
        self.normal * self.height
    }
}

fn taper(cx: &Context, request: &ToolRequest) -> Result<Body, Refusal> {
    let accuracy = cx.accuracy();
    let height = request.params.number("h")?;
    let degrees = request.params.number("angle")?;
    if height <= accuracy.length() {
        return Err(Error::input(format!("h must be above zero, got {height}")).into());
    }
    if !(degrees.abs().to_radians() > accuracy.angle() && degrees.abs() < 90.0) {
        return Err(Error::input(format!(
            "angle must lie strictly between -90 and 90 degrees and not be 0 (use extrude), got {degrees}"
        ))
        .into());
    }
    let (profile, face) = match request.inputs.as_slice() {
        [profile] if !profile.solid && profile.faces.len() == 1 => (profile, &profile.faces[0]),
        _ => return Err(Error::input("taper takes one flat profile").into()),
    };
    let anchor = profile
        .edges
        .first()
        .map(|edge| edge.point(0.5))
        .ok_or_else(|| Error::input("the profile has no edge"))?;
    let push =
        Push { normal: face.outward_normal(&anchor), height, inset: height * degrees.to_radians().tan() };

    let tops = top_corners(accuracy, profile, &push)?;
    let mut builder = BodyBuilder::new(accuracy);
    let mut near = Vec::with_capacity(profile.edges.len());
    let mut far = Vec::with_capacity(profile.edges.len());
    let mut sides: Vec<Arc<dyn Surface>> = Vec::with_capacity(profile.edges.len());
    for (index, edge) in profile.edges.iter().enumerate() {
        let inward = inward_of(profile, index)?;
        let (top, side) = top_edge(accuracy, edge, inward, &push, &tops)?;
        near.push(match edge.ends {
            Some(_) => builder.edge(Arc::clone(&edge.curve), edge.range.0, edge.range.1),
            None => builder.ring(Arc::clone(&edge.curve))?,
        });
        far.push(match top.ends {
            Some(_) => builder.edge(Arc::clone(&top.curve), top.range.0, top.range.1),
            None => builder.ring(Arc::clone(&top.curve))?,
        });
        sides.push(side);
    }
    let mut risers = BTreeMap::new();
    for (vertex, top) in &tops {
        let bottom = profile.vertices[*vertex];
        risers.insert(*vertex, builder.edge(Arc::new(Line::new(bottom, top - bottom)?), 0.0, 1.0));
    }

    let travel = |used: &Use| {
        let edge = &profile.edges[used.edge];
        let tangent = edge.curve.tangent(edge.parameter(0.5));
        if used.forward {
            tangent
        } else {
            -tangent
        }
    };
    let normal = push.normal;
    let bottom_bounds: Vec<Bound> = face
        .uses()
        .map(|used| {
            Bound::new(near[used.edge], profile.edges[used.edge].point(0.5), normal.cross(&travel(used)))
        })
        .collect();
    let mut top_bounds = Vec::with_capacity(bottom_bounds.len());
    for used in face.uses() {
        let top = builder.edge_data(far[used.edge]);
        let middle = top.point(0.5);
        let tangent = top.curve.tangent(top.parameter(0.5));
        let along = if used.forward { tangent } else { -tangent };
        top_bounds.push(Bound::new(far[used.edge], middle, normal.cross(&along)));
    }
    let lift = Sim3::from_parts(Translation3::from(push.lift()), Rotation3::identity(), 1.0);
    builder.face("bottom", Arc::clone(&face.surface), &anchor, &-normal, &bottom_bounds)?;
    let top_anchor = builder.edge_data(far[0]).point(0.5);
    builder.face("top", face.surface.transformed(&lift), &top_anchor, &normal, &top_bounds)?;

    for used in face.uses() {
        let edge = &profile.edges[used.edge];
        let (bottom_middle, top_middle) = (edge.point(0.5), builder.edge_data(far[used.edge]).point(0.5));
        let mut bounds = vec![
            Bound::new(near[used.edge], bottom_middle, top_middle - bottom_middle),
            Bound::new(far[used.edge], top_middle, bottom_middle - top_middle),
        ];
        if let Some((start, end)) = edge.ends {
            bounds.push(Bound::new(risers[&start], edge.point(0.0), edge.curve.tangent(edge.range.0)));
            bounds.push(Bound::new(risers[&end], edge.point(1.0), -edge.curve.tangent(edge.range.1)));
        }
        let name = edge.label.clone().unwrap_or_else(|| format!("s{}", used.edge + 1));
        // Away from the profile region is to the right of the direction of travel.
        builder.face(
            &name,
            Arc::clone(&sides[used.edge]),
            &bottom_middle,
            &travel(used).cross(&normal),
            &bounds,
        )?;
    }
    Ok(builder.finish(true))
}

/// The unit direction in the profile plane from the middle of an edge into the region:
/// the region lies to the left of the way its boundary runs.
fn inward_of(profile: &Body, edge: EdgeId) -> Result<Vec3, Refusal> {
    let face = &profile.faces[0];
    let used = face
        .uses()
        .find(|used| used.edge == edge)
        .ok_or_else(|| Error::geometry("a profile edge is not used by its face"))?;
    let data = &profile.edges[edge];
    let middle = data.point(0.5);
    let tangent = data.curve.tangent(data.parameter(0.5));
    let travel = if used.forward { tangent } else { -tangent };
    Ok(face.outward_normal(&middle).cross(&travel).normalize())
}

/// Where each vertex of the outline is at the top.
fn top_corners(accuracy: &Accuracy, profile: &Body, push: &Push) -> Result<BTreeMap<usize, Point3>, Refusal> {
    let mut tops = BTreeMap::new();
    for uses in &profile.faces[0].loops {
        for (position, arriving) in uses.iter().enumerate() {
            let leaving = uses[(position + 1) % uses.len()];
            let Some((start, end)) = profile.edges[arriving.edge].ends else { continue };
            let vertex = if arriving.forward { end } else { start };
            let at = profile.vertices[vertex];
            let heading = |used: &Use, at_start: bool| {
                let edge = &profile.edges[used.edge];
                let parameter = if used.forward == at_start { edge.range.0 } else { edge.range.1 };
                let tangent = edge.curve.tangent(parameter).normalize();
                if used.forward {
                    tangent
                } else {
                    -tangent
                }
            };
            let (into, out) = (heading(arriving, false), heading(&leaving, true));
            let left = |direction: Vec3| push.normal.cross(&direction);
            let moved = if accuracy.parallel(&into, &out) && into.dot(&out) > 0.0 {
                // Tangent sides share their inward direction at the corner.
                at + left(into) * push.inset
            } else if is_line(&profile.edges[arriving.edge]) && is_line(&profile.edges[leaving.edge]) {
                // The two offset lines, each moved inward, cross at the new corner.
                let (from_in, from_out) = (at + left(into) * push.inset, at + left(out) * push.inset);
                let turn = into.cross(&out).dot(&push.normal);
                let along = (from_out - from_in).cross(&out).dot(&push.normal) / turn;
                from_in + into * along
            } else {
                return declined(
                    "an arc meets a side at a corner without being tangent to it; that corner would run up a conic, for which there is no curve yet",
                );
            };
            tops.insert(vertex, moved + push.lift());
        }
    }
    Ok(tops)
}

fn is_line(edge: &Edge) -> bool {
    edge.curve.as_any().downcast_ref::<Line>().is_some()
}

/// The edge at the top that an outline edge becomes, and the surface the side between
/// them lies on.
fn top_edge(
    accuracy: &Accuracy,
    edge: &Edge,
    inward: Vec3,
    push: &Push,
    tops: &BTreeMap<usize, Point3>,
) -> Result<(Edge, Arc<dyn Surface>), Refusal> {
    if let Some(circle) = edge.curve.as_any().downcast_ref::<Circle>() {
        let towards_centre = (circle.center() - edge.point(0.5)).dot(&inward) > 0.0;
        let shrink = if towards_centre { 1.0 } else { -1.0 };
        let radius = circle.radius() - shrink * push.inset;
        if radius <= accuracy.length() {
            return declined("the taper closes an arc of the outline before the top");
        }
        let lifted = Translation3::from(push.lift()) * *circle.frame();
        let top = Circle::new(lifted, radius)?;
        // The radius shrinks by tan θ per unit of height; the apex is where it reaches zero.
        let slope = shrink * push.inset / push.height;
        let apex_height = circle.radius() / slope;
        let apex = circle.center() + push.normal * apex_height;
        let opening = if apex_height > 0.0 { -push.normal } else { push.normal };
        let cone = Cone::new(frame_about(&apex, &opening)?, slope.abs().atan())?;
        let top_edge = Edge { curve: Arc::new(top), range: edge.range, ends: edge.ends, label: None };
        return Ok((top_edge, Arc::new(cone)));
    }
    let (Some((start, end)), true) = (edge.ends, is_line(edge)) else {
        return declined("only lines and circular arcs can be tapered");
    };
    let (from, to) = (tops[&start], tops[&end]);
    let (bottom_from, bottom_to) = (edge.point(0.0), edge.point(1.0));
    if (to - from).dot(&(bottom_to - bottom_from)) <= accuracy.length() * (bottom_to - bottom_from).norm() {
        return declined("the taper closes the outline before the top");
    }
    let top = Line::new(from, to - from)?;
    let rise = (from + (to - from) * 0.5) - edge.point(0.5);
    let across = (bottom_to - bottom_from).normalize();
    let plane = Plane::new(frame(&edge.point(0.5), &across.cross(&rise), &across)?);
    Ok((Edge { curve: Arc::new(top), range: (0.0, 1.0), ends: edge.ends, label: None }, Arc::new(plane)))
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, face_names, made, volume};
    use krly3d_core::{Attempt, Id};
    use krly3d_kernel::ports::tool::run;
    use krly3d_kernel::schema::Params;
    use std::f64::consts::PI;

    fn tapered(
        path: &str,
        height: f64,
        degrees: f64,
    ) -> (krly3d_kernel::Context, std::sync::Arc<krly3d_kernel::topo::Body>) {
        let cx = context();
        let outline = made(&cx, "profile", Params::new().with("path", path), &[]);
        let solid = made(&cx, "taper", Params::new().with("h", height).with("angle", degrees), &[outline]);
        (cx, solid)
    }

    #[test]
    fn a_square_tapered_in_is_a_frustum_of_a_pyramid() {
        // Sides lean in by 45°: a 10 by 10 square becomes 6 by 6 at a height of 2.
        let (cx, solid) = tapered("M -5,-5 L 5,-5 L 5,5 L -5,5 Z", 2.0, 45.0);

        assert_eq!(face_names(&solid), ["bottom", "s1", "s2", "s3", "s4", "top"]);
        assert!(agree(volume(&cx, &solid), 2.0 / 3.0 * (100.0 + 36.0 + 60.0)));
        let slant = 2.0 * 2.0_f64.sqrt();
        assert!(agree(area(&cx, &solid), 100.0 + 36.0 + 4.0 * slant * (10.0 + 6.0) / 2.0));
    }

    #[test]
    fn a_circle_tapered_out_is_a_frustum_of_a_cone() {
        let (cx, solid) = tapered("O 0,0 3", 4.0, -30.0);

        let top = 3.0 + 4.0 * (30.0_f64).to_radians().tan();
        assert!(agree(volume(&cx, &solid), PI * 4.0 / 3.0 * (9.0 + 3.0 * top + top * top)));
    }

    #[test]
    fn a_rounded_outline_keeps_its_tangent_corners_and_the_steiner_volume() {
        // A 10 by 6 rectangle with corners rounded to 2: area A, perimeter P. Moved in by d,
        // a convex outline with round corners of radius at least d has area A − P·d + π·d².
        let path = "M -3,-3 L 3,-3 A 3,-1 5,-1 ccw L 5,1 A 3,1 3,3 ccw L -3,3 A -3,1 -5,1 ccw L -5,-1 A -3,-1 -3,-3 ccw Z";
        let (cx, solid) = tapered(path, 3.0, 20.0);

        let (a, p, k) =
            (60.0 - (16.0 - 4.0 * PI), 2.0 * (6.0 + 2.0) + 4.0 * PI, (20.0_f64).to_radians().tan());
        let expected = a * 3.0 - p * k * 9.0 / 2.0 + PI * k * k * 27.0 / 3.0;
        assert!(agree(volume(&cx, &solid), expected), "{} vs {expected}", volume(&cx, &solid));
    }

    #[test]
    fn a_taper_that_closes_the_outline_or_a_non_tangent_arc_corner_is_declined() {
        let cx = context();
        let steep = made(&cx, "profile", Params::new().with("path", "M -1,-1 L 1,-1 L 1,1 L -1,1 Z"), &[]);
        let pointed = made(&cx, "profile", Params::new().with("path", "M 0,0 L 4,0 A 0,0 0,4 ccw Z"), &[]);

        for (outline, reason) in [(steep, "closes"), (pointed, "conic")] {
            let attempt =
                run(&cx, &Id::new("taper"), &Params::new().with("h", 5.0).with("angle", 30.0), &[outline]);
            assert!(matches!(&attempt, Attempt::Declined(text) if text.contains(reason)), "{attempt:?}");
        }
    }
}
