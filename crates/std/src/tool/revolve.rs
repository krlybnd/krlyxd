//! A profile turned about an axis of its own plane.

use crate::curve::Circle;
use crate::surface::Plane;
use krly3d_core::math::frame::{frame, origin, x_axis, y_axis};
use krly3d_core::math::{Point3, Sim3, Vec3};
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::sweep::{sweep, Motion, SweepRequest};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Schema};
use krly3d_kernel::topo::{Body, BodyBuilder, Bound, Edge, EdgeId, Use};
use krly3d_kernel::{Context, Plugin};
use std::f64::consts::TAU;
use std::sync::Arc;

pub struct Revolve;

impl Plugin<Tool> for Revolve {
    fn id(&self) -> Id {
        Id::new("revolve")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a solid made by turning a profile about an axis of its plane",
            inputs: (1, 1),
            params: vec![
                Param::optional(
                    "angle",
                    Kind::Number,
                    360.0,
                    "how far to turn, in degrees; 360 closes the solid",
                ),
                Param::optional(
                    "axis",
                    Kind::Text,
                    "y",
                    "the axis of the profile plane to turn about: y (upright for xz and yz planes) or x",
                ),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        revolve(cx, request).into()
    }
}

fn revolve(cx: &Context, request: &ToolRequest) -> Result<Body, Error> {
    let accuracy = cx.accuracy();
    let degrees = request.params.number("angle")?;
    if degrees == 0.0 || degrees.abs() > 360.0 {
        return Err(Error::input(format!(
            "angle must be between −360 and 360 degrees and not zero, got {degrees}"
        )));
    }
    let (profile, face) = match request.inputs.as_slice() {
        [profile] if !profile.solid && profile.faces.len() == 1 => (profile, &profile.faces[0]),
        _ => return Err(Error::input("revolve takes one flat profile")),
    };
    let plane = face
        .surface
        .as_any()
        .downcast_ref::<Plane>()
        .ok_or_else(|| Error::input("revolve takes a flat profile"))?;
    let axis = match request.params.text("axis")? {
        "y" => y_axis(plane.frame()),
        "x" => x_axis(plane.frame()),
        other => return Err(Error::input(format!("axis must be y or x, got `{other}`"))),
    };
    let pivot = origin(plane.frame());
    let normal = plane.unit_normal();
    // Signed distance from the axis within the profile plane.
    let across = axis.cross(&normal);
    let reach = |point: &Point3| (point - pivot).dot(&across);
    let on_axis = |point: &Point3| accuracy.is_zero_length(reach(point));
    let samples = profile.edges.iter().flat_map(|edge| [0.0, 0.5, 1.0].map(|fraction| edge.point(fraction)));
    let side = samples
        .clone()
        .map(|point| reach(&point))
        .max_by(|a, b| a.abs().total_cmp(&b.abs()))
        .unwrap_or(0.0)
        .signum();
    if samples.clone().any(|point| reach(&point) * side < -accuracy.length()) {
        return Err(Error::input("the profile crosses the axis it is turned about"));
    }
    let lies_on_axis = |edge: &Edge| [0.0, 0.5, 1.0].iter().all(|fraction| on_axis(&edge.point(*fraction)));

    let closed = degrees.abs() == 360.0;
    let angle = if closed { TAU.copysign(degrees) } else { degrees.to_radians() };
    let motion = Motion::turn(pivot, axis, angle)?;
    let turned = Sim3::from_isometry(motion.at(1.0), 1.0);
    // A point on the profile side of the axis moves against the plane normal when turned
    // by a positive angle.
    let onward = -normal * side * angle.signum();
    let mut builder = BodyBuilder::new(accuracy);

    // Each profile vertex off the axis draws a circle, or an arc of one for a partial turn.
    let mut trails: Vec<Option<EdgeId>> = Vec::with_capacity(profile.vertices.len());
    for point in &profile.vertices {
        trails.push(if on_axis(point) {
            None
        } else {
            let foot = pivot + axis * (point - pivot).dot(&axis);
            let circle = Arc::new(Circle::new(frame(&foot, &axis, &(point - foot))?, reach(point).abs())?);
            Some(if closed { builder.ring(circle)? } else { builder.edge(circle, 0.0, angle) })
        });
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
    let name =
        |used: &Use| profile.edges[used.edge].label.clone().unwrap_or_else(|| format!("s{}", used.edge + 1));
    let swept =
        |edge: &Edge| sweep(cx, &SweepRequest { curve: Arc::clone(&edge.curve), motion }).into_result();
    // The face swept by an edge is bounded by the trails of the edge's end points. Seen at
    // such an end point, the face leaves the trail along the edge.
    let trail_bounds = |edge: &Edge| {
        let mut bounds = Vec::new();
        if let Some((start, end)) = edge.ends {
            let (leaving, arriving) = (edge.curve.tangent(edge.range.0), -edge.curve.tangent(edge.range.1));
            bounds.extend(trails[start].map(|trail| Bound::new(trail, edge.point(0.0), leaving)));
            bounds.extend(trails[end].map(|trail| Bound::new(trail, edge.point(1.0), arriving)));
        }
        bounds
    };
    let middle = |used: &Use| profile.edges[used.edge].point(0.5);
    let away = |used: &Use| travel(used).cross(&normal);

    if closed {
        for used in face.uses() {
            let edge = &profile.edges[used.edge];
            if !lies_on_axis(edge) {
                builder.face(&name(used), swept(edge)?, &middle(used), &away(used), &trail_bounds(edge))?;
            }
        }
        return Ok(builder.finish(true));
    }

    // A partial turn keeps the profile where it starts and where it ends as two flat caps.
    // An edge on the axis does not move, so both caps share it.
    let (mut near, mut far) = (Vec::new(), Vec::new());
    for edge in &profile.edges {
        let placed = match edge.ends {
            Some(_) => builder.edge(Arc::clone(&edge.curve), edge.range.0, edge.range.1),
            None => builder.ring(Arc::clone(&edge.curve))?,
        };
        near.push(placed);
        far.push(match edge.ends {
            _ if lies_on_axis(edge) => placed,
            Some(_) => builder.edge(edge.curve.transformed(&turned), edge.range.0, edge.range.1),
            None => builder.ring(edge.curve.transformed(&turned))?,
        });
    }
    let anchor = profile.edges.iter().map(|edge| edge.point(0.5)).find(|point| !on_axis(point));
    let anchor = anchor.ok_or_else(|| Error::input("the profile lies on the axis"))?;
    let turn = |vector: Vec3| turned.isometry.rotation * vector;
    let into_region = |used: &Use| normal.cross(&travel(used));
    let near_bounds: Vec<_> =
        face.uses().map(|used| Bound::new(near[used.edge], middle(used), into_region(used))).collect();
    let far_bounds: Vec<_> = face
        .uses()
        .map(|used| Bound::new(far[used.edge], turned * middle(used), turn(into_region(used))))
        .collect();
    builder.face("start", Arc::clone(&face.surface), &anchor, &-onward, &near_bounds)?;
    builder.face("end", face.surface.transformed(&turned), &(turned * anchor), &turn(onward), &far_bounds)?;
    for used in face.uses() {
        let edge = &profile.edges[used.edge];
        if lies_on_axis(edge) {
            continue;
        }
        let mut bounds = trail_bounds(edge);
        bounds.push(Bound::new(near[used.edge], middle(used), onward));
        bounds.push(Bound::new(far[used.edge], turned * middle(used), -turn(onward)));
        builder.face(&name(used), swept(edge)?, &middle(used), &away(used), &bounds)?;
    }
    Ok(builder.finish(true))
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, face_names, made, volume};
    use krly3d_core::Id;
    use krly3d_kernel::ports::tool::run;
    use krly3d_kernel::schema::Params;
    use std::f64::consts::PI;

    fn upright(path: &str) -> Params {
        Params::new().with("path", path).with("plane", "xz")
    }

    #[test]
    fn a_rectangle_beside_the_axis_turns_into_a_tube() {
        let cx = context();
        // From radius 2 to 5, from height 0 to 4.
        let section = made(&cx, "profile", upright("M 2,0 L 5,0 L 5,4 L 2,4 Z"), &[]);

        let tube = made(&cx, "revolve", Params::new(), &[section]);

        assert_eq!(tube.faces.len(), 4);
        assert!(agree(volume(&cx, &tube), PI * (25.0 - 4.0) * 4.0));
        assert!(agree(area(&cx, &tube), 2.0 * PI * (25.0 - 4.0) + 2.0 * PI * (5.0 + 2.0) * 4.0));
    }

    #[test]
    fn an_edge_on_the_axis_sweeps_no_face() {
        let cx = context();
        let names = vec!["bottom".into(), "side".into(), "axis".into()];
        let triangle = made(&cx, "profile", upright("M 0,0 L 4,0 L 0,9 Z").with("names", names), &[]);

        let cone = made(&cx, "revolve", Params::new(), &[triangle]);

        assert_eq!(face_names(&cone), ["bottom", "side"]);
        assert!(agree(volume(&cx, &cone), 48.0 * PI));
        assert!(agree(area(&cx, &cone), PI * 4.0 * (4.0 + 97.0_f64.sqrt())));
    }

    #[test]
    fn a_partial_turn_is_closed_by_two_flat_caps() {
        let cx = context();
        let section = made(&cx, "profile", upright("M 2,0 L 5,0 L 5,4 L 2,4 Z"), &[]);

        let wedge = made(&cx, "revolve", Params::new().with("angle", 90.0), &[section]);

        assert_eq!(wedge.faces.len(), 6);
        assert!(agree(volume(&cx, &wedge), 0.25 * PI * 21.0 * 4.0));
        assert!(agree(area(&cx, &wedge), 0.25 * (2.0 * PI * 21.0 + 2.0 * PI * 7.0 * 4.0) + 2.0 * 12.0));
    }

    #[test]
    fn a_turn_beyond_a_half_turn_keeps_every_face_the_right_way_round() {
        let cx = context();
        let section = made(&cx, "profile", upright("M 2,0 L 5,0 L 5,4 L 2,4 Z"), &[]);

        let most = made(&cx, "revolve", Params::new().with("angle", 270.0), &[section]);

        assert!(agree(volume(&cx, &most), 0.75 * PI * 21.0 * 4.0));
    }

    #[test]
    fn turning_the_other_way_gives_the_same_volume() {
        let cx = context();
        let triangle = made(&cx, "profile", upright("M 0,0 L 4,0 L 0,9 Z"), &[]);

        let slice = made(&cx, "revolve", Params::new().with("angle", -120.0), &[triangle]);

        assert!(agree(volume(&cx, &slice), 48.0 * PI / 3.0));
    }

    #[test]
    fn a_partial_turn_of_a_disc_is_a_piece_of_a_torus() {
        let cx = context();
        let disc = made(&cx, "profile", upright("O 6,0 2"), &[]);

        let bend = made(&cx, "revolve", Params::new().with("angle", 200.0), &[disc]);

        let share = 200.0 / 360.0;
        assert!(agree(volume(&cx, &bend), share * 2.0 * PI * PI * 6.0 * 4.0));
        assert!(agree(area(&cx, &bend), share * 4.0 * PI * PI * 12.0 + 2.0 * PI * 4.0));
    }

    #[test]
    fn a_profile_across_the_axis_is_rejected() {
        let cx = context();
        let section = made(&cx, "profile", upright("M -1,0 L 5,0 L 5,4 Z"), &[]);

        let error = run(&cx, &Id::new("revolve"), &Params::new(), &[section]).into_result().unwrap_err();

        assert_eq!(error.message, "the profile crosses the axis it is turned about");
    }
}
