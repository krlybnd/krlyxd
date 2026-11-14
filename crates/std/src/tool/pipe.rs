//! A round bar bent along a path, as a pipe is bent: straight runs between the points,
//! each corner bent round with the given radius.
//!
//! The path is the polyline through `points` with every corner replaced by the arc of
//! radius `bend` tangent to both runs: at a corner turning by `φ` the arc starts and ends
//! `bend·tan(φ/2)` from the corner. A circle of radius `r` square to the path sweeps a
//! cylinder along a run and a piece of a torus (ring `bend`, tube `r`) around an arc, so
//! every face is exact and neighbouring faces meet in the circles where the path turns
//! from run to arc. Flat discs close the two ends.
//!
//! With `radii` the bar thickens or thins from point to point: a run from one point to
//! the next sweeps a frustum of a cone between their radii, and the bend at a point keeps
//! that point's radius, so that every face is still exact.

use crate::curve::Circle;
use crate::surface::{Cone, Cylinder, Plane, Torus};
use krly3d_core::math::frame::{frame, frame_about};
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::{Accuracy, Attempt, Error, Id};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Schema};
use krly3d_kernel::topo::{Body, BodyBuilder, Bound, EdgeId};
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct Pipe;

impl Plugin<Tool> for Pipe {
    fn id(&self) -> Id {
        Id::new("pipe")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a round bar of radius `r` along the path through `points`, each corner bent with radius `bend`; faces: start, end, run1…, bend1…",
            inputs: (0, 0),
            params: vec![
                Param::alternative("r", Kind::Number, "radius of the bar, the same all along"),
                Param::alternative("radii", Kind::Numbers, "radius of the bar at each point, in place of `r`"),
                Param::required("points", Kind::Numbers, "the corners of the path as x,y,z,x,y,z,…; at least two points"),
                Param::optional("bend", Kind::Number, 0.0, "radius the corners are bent with; above `r`, and needed when the path has a corner"),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        let built = request.params.numbers("points").and_then(|points| {
            let count = points.len() / 3;
            let radii = match (request.params.get("r"), request.params.get("radii")) {
                (Some(_), None) => vec![request.params.number("r")?; count],
                (None, Some(_)) => request.params.numbers("radii")?,
                _ => return Err(Error::input("give the radius by `r` or, point by point, by `radii`")),
            };
            if radii.len() != count {
                return Err(Error::input(format!("`radii` needs one radius for each of the {count} points")));
            }
            Path::through(cx.accuracy(), &points, &radii, request.params.number("bend")?)
        });
        match built {
            Ok(Ok(path)) => path.solid(cx.accuracy()).into(),
            Ok(Err(reason)) => Attempt::Declined(reason),
            Err(error) => Attempt::Failed(error),
        }
    }
}

/// One piece of the path: a straight run or an arc around a corner.
enum Piece {
    /// A straight run, `near` thick where it starts and `far` where it ends.
    Run { from: Point3, to: Point3, near: f64, far: f64 },
    /// A bend of the bar's thickness `radius`.
    Arc { centre: Point3, axis: Vec3, from: Point3, to: Point3, radius: f64 },
}

struct Path {
    bend: f64,
    pieces: Vec<Piece>,
}

impl Path {
    /// The path, or why it cannot be bent (the outer result), or why the input is wrong.
    fn through(
        accuracy: &Accuracy,
        numbers: &[f64],
        radii: &[f64],
        bend: f64,
    ) -> Result<Result<Self, String>, Error> {
        if numbers.len() < 6 || !numbers.len().is_multiple_of(3) {
            return Err(Error::input("`points` needs at least two points, three numbers each"));
        }
        if let Some(radius) = radii.iter().find(|radius| **radius <= accuracy.length()) {
            return Err(Error::input(format!("a radius must be above zero, got {radius}")));
        }
        let points: Vec<Point3> = numbers.chunks(3).map(|xyz| Point3::new(xyz[0], xyz[1], xyz[2])).collect();
        if points.windows(2).any(|pair| accuracy.same_point(&pair[0], &pair[1])) {
            return Err(Error::input("two points in a row are the same point"));
        }
        // Where each corner's arc starts and ends, as a distance from the corner.
        let mut cut = vec![0.0; points.len()];
        let mut corners = Vec::new();
        for index in 1..points.len() - 1 {
            let (into, out) = (
                (points[index] - points[index - 1]).normalize(),
                (points[index + 1] - points[index]).normalize(),
            );
            let turn = into.dot(&out).clamp(-1.0, 1.0).acos();
            if turn <= accuracy.angle() {
                continue;
            }
            let radius = radii[index];
            if bend <= radius {
                return Ok(Err(format!(
                    "the path turns at point {}; give `bend` above its radius {radius}, or the bar would fold into itself",
                    index + 1
                )));
            }
            if std::f64::consts::PI - turn <= accuracy.angle() {
                return Ok(Err(format!("the path turns back on itself at point {}", index + 1)));
            }
            cut[index] = bend * (0.5 * turn).tan();
            corners.push((index, into, out, turn));
        }
        for index in 0..points.len() - 1 {
            if cut[index] + cut[index + 1] > (points[index + 1] - points[index]).norm() + accuracy.length() {
                return Ok(Err(format!(
                    "the bends at points {} and {} need more room than the run between them",
                    index + 1,
                    index + 2
                )));
            }
        }
        let mut pieces = Vec::new();
        let mut start = points[0];
        for index in 1..points.len() {
            let corner = corners.iter().find(|(at, ..)| *at == index);
            let into = (points[index] - points[index - 1]).normalize();
            let before = points[index] - into * cut[index];
            if !accuracy.same_point(&start, &before) {
                pieces.push(Piece::Run {
                    from: start,
                    to: before,
                    near: radii[index - 1],
                    far: radii[index],
                });
            }
            start = before;
            if let Some((_, into, out, turn)) = corner {
                let after = points[index] + out * cut[index];
                let inward = (out - into).normalize();
                let centre = points[index] + inward * (bend / (0.5 * turn).cos());
                pieces.push(Piece::Arc {
                    centre,
                    axis: into.cross(out).normalize(),
                    from: before,
                    to: after,
                    radius: radii[index],
                });
                start = after;
            }
        }
        Ok(Ok(Self { bend, pieces }))
    }

    /// Direction of the path at the start or end of a piece.
    fn heading(piece: &Piece, at_end: bool) -> Vec3 {
        match piece {
            Piece::Run { from, to, .. } => (to - from).normalize(),
            Piece::Arc { centre, axis, from, to, .. } => {
                let point = if at_end { to } else { from };
                axis.cross(&(point - centre)).normalize()
            }
        }
    }

    fn solid(&self, accuracy: &Accuracy) -> Result<Body, Error> {
        let mut builder = BodyBuilder::new(accuracy);
        // The circle where the bar starts, between each two pieces, and where it ends.
        let mut rings: Vec<(EdgeId, Point3, Vec3, f64)> = Vec::with_capacity(self.pieces.len() + 1);
        let section = |builder: &mut BodyBuilder,
                       at: Point3,
                       along: Vec3,
                       radius: f64|
         -> Result<(EdgeId, Point3, Vec3, f64), Error> {
            let circle = Circle::new(frame_about(&at, &along)?, radius)?;
            Ok((builder.ring(Arc::new(circle))?, at, along, radius))
        };
        let first = &self.pieces[0];
        rings.push(section(
            &mut builder,
            start_of(first),
            Self::heading(first, false),
            thickness(first, false),
        )?);
        for piece in &self.pieces {
            rings.push(section(
                &mut builder,
                end_of(piece),
                Self::heading(piece, true),
                thickness(piece, true),
            )?);
        }
        let on_ring = |at: &Point3, along: &Vec3, radius: f64| {
            at + frame_about(at, along).map_or(Vec3::x(), |placed| placed * Vec3::x()) * radius
        };

        let (start_edge, start_at, start_along, start_radius) = rings[0];
        let start_rim = on_ring(&start_at, &start_along, start_radius);
        let start_plane = Plane::new(frame_about(&start_at, &start_along)?);
        builder.face(
            "start",
            Arc::new(start_plane),
            &start_at,
            &-start_along,
            &[Bound::new(start_edge, start_rim, start_at - start_rim)],
        )?;
        let (end_edge, end_at, end_along, end_radius) = rings[rings.len() - 1];
        let end_rim = on_ring(&end_at, &end_along, end_radius);
        let end_plane = Plane::new(frame_about(&end_at, &end_along)?);
        builder.face(
            "end",
            Arc::new(end_plane),
            &end_at,
            &end_along,
            &[Bound::new(end_edge, end_rim, end_at - end_rim)],
        )?;

        let (mut runs, mut bends) = (0, 0);
        for (index, piece) in self.pieces.iter().enumerate() {
            let ((near, near_at, near_along, near_radius), (far, far_at, far_along, far_radius)) =
                (rings[index], rings[index + 1]);
            let (near_rim, far_rim) =
                (on_ring(&near_at, &near_along, near_radius), on_ring(&far_at, &far_along, far_radius));
            let (name, surface): (String, Arc<dyn Surface>) = match piece {
                Piece::Run { from, to, near, far } => {
                    runs += 1;
                    (format!("run{runs}"), run_surface(accuracy, from, to, *near, *far)?)
                }
                Piece::Arc { centre, axis, from, radius, .. } => {
                    bends += 1;
                    let ring = frame(centre, axis, &(from - centre))?;
                    (format!("bend{bends}"), Arc::new(Torus::new(ring, self.bend, *radius)?))
                }
            };
            let outward = near_rim - near_at;
            builder.face(
                &name,
                surface,
                &near_rim,
                &outward,
                &[Bound::new(near, near_rim, near_along), Bound::new(far, far_rim, -far_along)],
            )?;
        }
        Ok(builder.finish(true))
    }
}

/// The thickness of the bar where a piece starts or ends.
fn thickness(piece: &Piece, at_end: bool) -> f64 {
    match piece {
        Piece::Run { near, far, .. } => {
            if at_end {
                *far
            } else {
                *near
            }
        }
        Piece::Arc { radius, .. } => *radius,
    }
}

/// The surface a run sweeps: a cylinder when its ends are equally thick, else the cone
/// whose apex is where the radius, changing evenly along the run, would reach zero.
fn run_surface(
    accuracy: &Accuracy,
    from: &Point3,
    to: &Point3,
    near: f64,
    far: f64,
) -> Result<Arc<dyn Surface>, Error> {
    let length = (to - from).norm();
    let along = (to - from) / length;
    if accuracy.is_zero_length(far - near) {
        return Ok(Arc::new(Cylinder::new(frame_about(from, &along)?, near)?));
    }
    let slope = (far - near) / length;
    let apex = from - along * (near / slope);
    let opening = if slope > 0.0 { along } else { -along };
    Ok(Arc::new(Cone::new(frame_about(&apex, &opening)?, slope.abs().atan())?))
}

fn start_of(piece: &Piece) -> Point3 {
    match piece {
        Piece::Run { from, .. } | Piece::Arc { from, .. } => *from,
    }
}

fn end_of(piece: &Piece) -> Point3 {
    match piece {
        Piece::Run { to, .. } | Piece::Arc { to, .. } => *to,
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, face_names, made, volume};
    use krly3d_core::{Attempt, Id};
    use krly3d_kernel::ports::tool::run;
    use krly3d_kernel::schema::{Params, Value};
    use std::f64::consts::PI;

    fn pipe(points: &[f64], radius: f64, bend: f64) -> Params {
        let numbers: Vec<krly3d_kernel::schema::Value> = points.iter().map(|value| (*value).into()).collect();
        Params::new()
            .with("r", radius)
            .with("bend", bend)
            .with("points", krly3d_kernel::schema::Value::List(numbers))
    }

    #[test]
    fn a_straight_pipe_is_a_cylinder() {
        let cx = context();

        let bar = made(&cx, "pipe", pipe(&[1.0, 2.0, 3.0, 4.0, 6.0, 15.0], 0.5, 0.0), &[]);

        assert_eq!(face_names(&bar), ["end", "run1", "start"]);
        assert!(agree(volume(&cx, &bar), PI * 0.25 * 13.0));
    }

    #[test]
    fn a_pipe_bent_at_a_right_angle_has_the_volume_and_area_of_pappus() {
        let cx = context();
        // Runs of 10 to the corner each way, bent with radius 3: each run loses 3 to the
        // quarter arc of length 3π/2.
        let bar = made(&cx, "pipe", pipe(&[0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0], 1.0, 3.0), &[]);

        assert_eq!(face_names(&bar), ["bend1", "end", "run1", "run2", "start"]);
        let path = 14.0 + 1.5 * PI;
        assert!(agree(volume(&cx, &bar), PI * path));
        assert!(agree(area(&cx, &bar), 2.0 * PI * path + 2.0 * PI));
    }

    #[test]
    fn a_pipe_bent_twice_in_different_planes_keeps_its_closed_form_volume() {
        let cx = context();
        let points = [0.0, 0.0, 0.0, 8.0, 0.0, 0.0, 8.0, 8.0, 0.0, 8.0, 8.0, 9.0];

        let bar = made(&cx, "pipe", pipe(&points, 0.5, 2.0), &[]);

        // Two quarter bends of radius 2: the path is 25 − 4·2 + 2·π.
        assert!(agree(volume(&cx, &bar), PI * 0.25 * (25.0 - 8.0 + 2.0 * PI)));
    }

    #[test]
    fn a_tapering_bent_bar_is_frustums_and_torus_pieces_with_their_closed_form_volume() {
        let cx = context();
        let numbers: Vec<Value> =
            [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0].iter().map(|value| (*value).into()).collect();
        let radii: Vec<Value> = [2.0, 1.0, 0.5].iter().map(|value| (*value).into()).collect();
        let params = Params::new()
            .with("points", Value::List(numbers))
            .with("radii", Value::List(radii))
            .with("bend", 3.0);

        let bar = made(&cx, "pipe", params, &[]);

        // Runs of 7 from radius 2 to 1 and from 1 to 0.5, and a quarter bend of radius 3
        // with a tube of 1: π·h/3·(R² + R·r + r²) twice, and π·1²·3·π/2 by Pappus.
        let frustum = |h: f64, a: f64, b: f64| PI * h / 3.0 * (a * a + a * b + b * b);
        let expected = frustum(7.0, 2.0, 1.0) + frustum(7.0, 1.0, 0.5) + PI * 1.5 * PI;
        assert!(agree(volume(&cx, &bar), expected), "{} vs {expected}", volume(&cx, &bar));
    }

    #[test]
    fn a_corner_without_room_or_radius_is_declined() {
        let cx = context();
        let corner = [0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 2.0, 0.0];

        for (bend, reason) in [(0.0, "give `bend`"), (5.0, "more room")] {
            let attempt = run(&cx, &Id::new("pipe"), &pipe(&corner, 0.5, bend), &[]);
            assert!(matches!(&attempt, Attempt::Declined(text) if text.contains(reason)), "{attempt:?}");
        }
    }
}
