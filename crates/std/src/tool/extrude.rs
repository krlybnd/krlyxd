//! A profile pushed straight along the normal of its plane.

use crate::curve::Line;
use krly3d_core::math::{Sim3, Vec3};
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::sweep::{sweep, Motion, SweepRequest};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Schema};
use krly3d_kernel::topo::{Body, BodyBuilder, Bound, EdgeId, Use};
use krly3d_kernel::{Context, Plugin};
use nalgebra::{Rotation3, Translation3};
use std::sync::Arc;

pub struct Extrude;

impl Plugin<Tool> for Extrude {
    fn id(&self) -> Id {
        Id::new("extrude")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a solid made by pushing a profile straight along the normal of its plane",
            inputs: (1, 1),
            params: vec![Param::required(
                "h",
                Kind::Number,
                "how far to push; negative pushes the other way",
            )],
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        extrude(cx, request).into()
    }
}

fn extrude(cx: &Context, request: &ToolRequest) -> Result<Body, Error> {
    let height = request.params.number("h")?;
    let (profile, face) = match request.inputs.as_slice() {
        [profile] if !profile.solid && profile.faces.len() == 1 => (profile, &profile.faces[0]),
        _ => return Err(Error::input("extrude takes one flat profile")),
    };
    let anchor = profile.edges.first().map(|edge| edge.point(0.5));
    let anchor = anchor.ok_or_else(|| Error::input("the profile has no edge"))?;
    let normal = face.outward_normal(&anchor);
    let shift = normal * height;
    let motion = Motion::shift(shift)?;
    let lift = Sim3::from_parts(Translation3::from(shift), Rotation3::identity(), 1.0);
    let mut builder = BodyBuilder::new(cx.accuracy());

    // Every profile edge appears twice, where it is and lifted; every profile vertex
    // becomes a straight edge between the two.
    let (mut near, mut far) = (Vec::new(), Vec::new());
    for edge in &profile.edges {
        for (edges, curve) in
            [(&mut near, Arc::clone(&edge.curve)), (&mut far, edge.curve.transformed(&lift))]
        {
            edges.push(match edge.ends {
                Some(_) => builder.edge(curve, edge.range.0, edge.range.1),
                None => builder.ring(curve)?,
            });
        }
    }
    let mut risers = Vec::with_capacity(profile.vertices.len());
    for point in &profile.vertices {
        risers.push(builder.edge(Arc::new(Line::new(*point, shift)?), 0.0, 1.0));
    }

    // Direction of travel of a use at the middle of its edge; the profile region is on its left.
    let travel = |used: &Use| {
        let edge = &profile.edges[used.edge];
        let tangent = edge.curve.tangent(edge.parameter(0.5));
        if used.forward {
            tangent
        } else {
            -tangent
        }
    };
    let cap = |edges: &[EdgeId], moved: Vec3| -> Vec<Bound> {
        let bound = |used: &Use| {
            let middle = profile.edges[used.edge].point(0.5) + moved;
            Bound::new(edges[used.edge], middle, normal.cross(&travel(used)))
        };
        face.uses().map(bound).collect()
    };
    let along = normal * height.signum();
    let (near_name, far_name) = if height > 0.0 { ("bottom", "top") } else { ("top", "bottom") };
    builder.face(near_name, Arc::clone(&face.surface), &anchor, &-along, &cap(&near, Vec3::zeros()))?;
    builder.face(far_name, face.surface.transformed(&lift), &(anchor + shift), &along, &cap(&far, shift))?;

    for used in face.uses() {
        let edge = &profile.edges[used.edge];
        let side = sweep(cx, &SweepRequest { curve: Arc::clone(&edge.curve), motion }).into_result()?;
        let middle = edge.point(0.5);
        let mut bounds = vec![
            Bound::new(near[used.edge], middle, shift),
            Bound::new(far[used.edge], middle + shift, -shift),
        ];
        if let Some((start, end)) = edge.ends {
            bounds.push(Bound::new(risers[start], edge.point(0.0), edge.curve.tangent(edge.range.0)));
            bounds.push(Bound::new(risers[end], edge.point(1.0), -edge.curve.tangent(edge.range.1)));
        }
        let name = edge.label.clone().unwrap_or_else(|| format!("s{}", used.edge + 1));
        // Away from the profile region is to the right of the direction of travel.
        builder.face(&name, side, &middle, &travel(used).cross(&normal), &bounds)?;
    }
    Ok(builder.finish(true))
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, face_names, made, volume};
    use krly3d_kernel::schema::Params;
    use std::f64::consts::PI;

    #[test]
    fn a_rectangle_pushed_up_is_a_block_with_six_named_faces() {
        let cx = context();
        let base = made(&cx, "rectangle", Params::new().with("w", 40.0).with("d", 10.0), &[]);

        let block = made(&cx, "extrude", Params::new().with("h", 20.0), &[base]);

        assert_eq!(face_names(&block), ["back", "bottom", "front", "left", "right", "top"]);
        assert_eq!((block.vertices.len(), block.edges.len()), (8, 12));
        assert!(agree(volume(&cx, &block), 8000.0));
        assert!(agree(area(&cx, &block), 2800.0));
    }

    #[test]
    fn a_circle_pushed_up_is_a_cylinder_without_any_vertex() {
        let cx = context();
        let base = made(&cx, "circle", Params::new().with("r", 3.0), &[]);

        let cylinder = made(&cx, "extrude", Params::new().with("h", 10.0), &[base]);

        assert_eq!(face_names(&cylinder), ["bottom", "side", "top"]);
        assert_eq!((cylinder.vertices.len(), cylinder.edges.len()), (0, 2));
        assert!(agree(volume(&cx, &cylinder), 90.0 * PI));
        assert!(agree(area(&cx, &cylinder), 78.0 * PI));
    }

    #[test]
    fn a_hole_in_the_profile_becomes_a_hole_through_the_solid() {
        let cx = context();
        let outline = Params::new().with("path", "M -30,-20 L 30,-20 L 30,20 L -30,20 Z O 20,15 4");
        let plate = made(&cx, "profile", outline, &[]);

        let solid = made(&cx, "extrude", Params::new().with("h", 10.0), &[plate]);

        assert!(agree(volume(&cx, &solid), 24_000.0 - 160.0 * PI));
        assert!(agree(area(&cx, &solid), 2.0 * (2400.0 - 16.0 * PI) + 2000.0 + 80.0 * PI));
    }

    #[test]
    fn pushing_down_gives_the_same_solid_with_top_and_bottom_swapped() {
        let cx = context();
        let base = made(&cx, "rectangle", Params::new().with("w", 4.0).with("d", 2.0), &[]);

        let block = made(&cx, "extrude", Params::new().with("h", -3.0), &[base]);

        assert!(agree(volume(&cx, &block), 24.0));
        let (_, top) = block.face("top").unwrap();
        assert!(top.outward_normal(&block.vertices[0]).z > 0.0);
        assert!(block.vertices.iter().all(|point| point.z <= 0.0));
    }

    #[test]
    fn an_outline_with_arcs_keeps_its_exact_volume() {
        let cx = context();
        // A unit square with a half disc of radius 0.5 added on its right side.
        let outline =
            made(&cx, "profile", Params::new().with("path", "M 0,0 L 1,0 A 1,0.5 1,1 ccw L 0,1 Z"), &[]);

        let solid = made(&cx, "extrude", Params::new().with("h", 2.0), &[outline]);

        assert!(agree(volume(&cx, &solid), 2.0 + 0.25 * PI));
        assert!(agree(area(&cx, &solid), 2.0 * (1.0 + 0.125 * PI) + 2.0 * (3.0 + 0.5 * PI)));
    }
}
