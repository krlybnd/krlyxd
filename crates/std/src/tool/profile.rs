//! A flat profile drawn as a path of lines and circular arcs.

use super::placement;
use crate::curve::{Circle, Ellipse, Line};
use crate::surface::Plane;
use krly3d_core::math::frame::{frame, origin, z_axis};
use krly3d_core::math::{Iso3, Point3, Uv, Vec3};
use krly3d_core::{Accuracy, Attempt, Error, Id};
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::{Kind, Param, Params, Schema};
use krly3d_kernel::topo::{Body, BodyBuilder, Bound, EdgeId};
use krly3d_kernel::{Context, Plugin};
use nalgebra::Translation3;
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

pub struct Profile;

impl Plugin<Tool> for Profile {
    fn id(&self) -> Id {
        Id::new("profile")
    }

    fn schema(&self) -> Option<Schema> {
        let [plane, at] = placement();
        Some(Schema {
            about: "a flat region bounded by lines and circular arcs, drawn as a path",
            inputs: (0, 0),
            params: vec![
                Param::required(
                    "path",
                    Kind::Text,
                    "M x,y starts a loop; L x,y is a line; A cx,cy x,y ccw|cw an arc about a centre; Z closes the loop; O cx,cy r is a whole circle; E cx,cy a,b a whole ellipse with semi-axes a along x and b along y; R cx,cy w,h r a w by h rectangle with corners rounded by r",
                ),
                plane,
                at,
                Param::optional("names", Kind::Texts, Vec::new(), "names of the edges in drawing order; s1, s2, … when left out"),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        draw(cx.accuracy(), &request.params).into()
    }
}

/// One piece of the path, in the coordinates of the drawing plane.
enum Stroke {
    Line { from: Uv, to: Uv },
    Arc { center: Uv, from: Uv, to: Uv, counter_clockwise: bool },
    Circle { center: Uv, radius: f64 },
    Ellipse { center: Uv, axes: Uv },
}

/// An edge as built, with what the face needs to know about how it was drawn.
struct Drawn {
    edge: EdgeId,
    /// The middle of the edge and the direction of drawing there.
    middle: Point3,
    travel: Vec3,
    /// Signed area between the edge and the origin, about the plane normal.
    area: f64,
}

fn draw(accuracy: &Accuracy, params: &Params) -> Result<Body, Error> {
    let plane = plane_frame(params.text("plane")?, &params.point("at")?)?;
    let names = params.texts("names")?;
    let normal = z_axis(&plane);
    let mut builder = BodyBuilder::new(accuracy);
    let mut loops = Vec::new();
    let mut count = 0;
    for strokes in parse(params.text("path")?, accuracy)? {
        let mut drawn = Vec::with_capacity(strokes.len());
        for stroke in &strokes {
            let piece = build(&mut builder, &plane, stroke, accuracy)?;
            let label = names.get(count).cloned().unwrap_or_else(|| format!("s{}", count + 1));
            builder.label(piece.edge, &label);
            count += 1;
            drawn.push(piece);
        }
        loops.push(drawn);
    }
    let areas: Vec<f64> = loops.iter().map(|drawn| drawn.iter().map(|piece| piece.area).sum()).collect();
    let outer = (0..loops.len())
        .max_by(|a, b| areas[*a].abs().total_cmp(&areas[*b].abs()))
        .filter(|outer| areas[*outer].abs() > accuracy.area())
        .ok_or_else(|| Error::input("the path encloses no area"))?;
    // The region lies to the left of an outer loop drawn counter-clockwise and of a hole
    // drawn clockwise; the sign of each loop's area says which way it was drawn.
    let mut bounds = Vec::with_capacity(count);
    for (index, drawn) in loops.iter().enumerate() {
        let left = if index == outer { areas[index].signum() } else { -areas[index].signum() };
        for piece in drawn {
            bounds.push(Bound::new(piece.edge, piece.middle, normal.cross(&piece.travel) * left));
        }
    }
    builder.face("profile", Arc::new(Plane::new(plane)), &origin(&plane), &normal, &bounds)?;
    Ok(builder.finish(false))
}

/// The frame of a named drawing plane. Upright planes have the world z axis as their
/// y axis, so that a profile reads as (across, up).
pub(crate) fn plane_frame(name: &str, at: &Point3) -> Result<Iso3, Error> {
    let (x, y) = match name {
        "xy" => (Vec3::x(), Vec3::y()),
        "xz" => (Vec3::x(), Vec3::z()),
        "yz" => (Vec3::y(), Vec3::z()),
        other => return Err(Error::input(format!("plane must be xy, xz or yz, got `{other}`"))),
    };
    frame(at, &x.cross(&y), &x)
}

fn build(
    builder: &mut BodyBuilder,
    plane: &Iso3,
    stroke: &Stroke,
    accuracy: &Accuracy,
) -> Result<Drawn, Error> {
    let world = |point: &Uv| plane * Point3::new(point.x, point.y, 0.0);
    let normal = z_axis(plane);
    let centred = |center: &Uv| Iso3::from_parts(Translation3::from(world(center).coords), plane.rotation);
    match stroke {
        Stroke::Line { from, to } => {
            let line = Line::new(world(from), world(to) - world(from))?;
            let area = line.vector_area(0.0, 1.0).dot(&normal);
            let (middle, travel) = (line.point(0.5), line.direction());
            Ok(Drawn { edge: builder.edge(Arc::new(line), 0.0, 1.0), middle, travel, area })
        }
        Stroke::Arc { center, from, to, counter_clockwise } => {
            let radius = (from - center).norm();
            if !accuracy.is_zero_length((to - center).norm() - radius) {
                return Err(Error::input(format!(
                    "the arc about {center} does not end at the radius it starts from"
                )));
            }
            let circle = Circle::new(centred(center), radius)?;
            // The edge always runs counter-clockwise; an arc drawn clockwise is the same
            // edge travelled backwards.
            let (start, end) = if *counter_clockwise { (from, to) } else { (to, from) };
            let first = (start.y - center.y).atan2(start.x - center.x);
            let mut last = (end.y - center.y).atan2(end.x - center.x);
            if last <= first {
                last += TAU;
            }
            let sense = if *counter_clockwise { 1.0 } else { -1.0 };
            let halfway = 0.5 * (first + last);
            let (middle, travel) = (circle.point(halfway), circle.tangent(halfway) * sense);
            let area = circle.vector_area(first, last).dot(&normal) * sense;
            Ok(Drawn { edge: builder.edge(Arc::new(circle), first, last), middle, travel, area })
        }
        Stroke::Circle { center, radius } => {
            let circle = Circle::new(centred(center), *radius)?;
            let area = circle.vector_area(0.0, TAU).dot(&normal);
            let (middle, travel) = (circle.point(PI), circle.tangent(PI));
            Ok(Drawn { edge: builder.ring(Arc::new(circle))?, middle, travel, area })
        }
        Stroke::Ellipse { center, axes } => {
            let ellipse = Ellipse::new(centred(center), axes.x, axes.y)?;
            let area = ellipse.vector_area(0.0, TAU).dot(&normal);
            let (middle, travel) = (ellipse.point(PI), ellipse.tangent(PI));
            Ok(Drawn { edge: builder.ring(Arc::new(ellipse))?, middle, travel, area })
        }
    }
}

/// Splits the path into loops of strokes.
fn parse(path: &str, accuracy: &Accuracy) -> Result<Vec<Vec<Stroke>>, Error> {
    let mut words = path.split_whitespace();
    let mut loops = Vec::new();
    let mut open: Option<(Uv, Uv, Vec<Stroke>)> = None;
    while let Some(command) = words.next() {
        match (command, open.as_mut()) {
            ("M", None) => {
                let start = pair(words.next(), "M")?;
                open = Some((start, start, Vec::new()));
            }
            ("E", None) => {
                let center = pair(words.next(), "E")?;
                loops.push(vec![Stroke::Ellipse { center, axes: pair(words.next(), "E")? }]);
            }
            ("O", None) => {
                let center = pair(words.next(), "O")?;
                loops.push(vec![Stroke::Circle { center, radius: number(words.next(), "O")? }]);
            }
            ("R", None) => {
                let center = pair(words.next(), "R")?;
                let size = pair(words.next(), "R")?;
                loops.push(rounded(center, size, number(words.next(), "R")?)?);
            }
            ("M" | "O" | "E" | "R", Some(_)) => {
                return Err(Error::input(format!(
                    "`{command}` starts a new loop before the last one is closed with Z"
                )))
            }
            ("L", Some((_, at, strokes))) => {
                let to = pair(words.next(), "L")?;
                strokes.push(Stroke::Line { from: *at, to });
                *at = to;
            }
            ("A", Some((_, at, strokes))) => {
                let center = pair(words.next(), "A")?;
                let to = pair(words.next(), "A")?;
                let counter_clockwise = match words.next() {
                    Some("ccw") => true,
                    Some("cw") => false,
                    other => return Err(Error::input(format!("an arc ends with ccw or cw, got {other:?}"))),
                };
                strokes.push(Stroke::Arc { center, from: *at, to, counter_clockwise });
                *at = to;
            }
            ("Z", Some((start, at, strokes))) => {
                if !accuracy.is_zero_length((*at - *start).norm()) {
                    strokes.push(Stroke::Line { from: *at, to: *start });
                }
                loops.extend(open.take().map(|(_, _, strokes)| strokes));
            }
            ("L" | "A" | "Z", None) => {
                return Err(Error::input(format!("`{command}` comes before any loop is started with M")))
            }
            (other, _) => return Err(Error::input(format!("unknown path command `{other}`"))),
        }
    }
    if open.is_some() {
        return Err(Error::input("the path ends inside an open loop; close it with Z"));
    }
    Ok(loops)
}

/// A rectangle of `size` about `center` with its corners rounded by `radius`: four sides and
/// four quarter arcs, counter-clockwise from the lower side, so that rounded rectangles of any
/// size correspond edge to edge in a loft.
fn rounded(center: Uv, size: Uv, radius: f64) -> Result<Vec<Stroke>, Error> {
    let (half_x, half_y) = (0.5 * size.x, 0.5 * size.y);
    if !(radius > 0.0 && radius < half_x.min(half_y)) {
        return Err(Error::input(format!(
            "`R` needs a corner radius above 0 and below half the shorter side ({}), got {radius}; a full half is an `O` or an `E`",
            half_x.min(half_y)
        )));
    }
    let (inner_x, inner_y) = (half_x - radius, half_y - radius);
    let at = |x: f64, y: f64| Uv::new(center.x + x, center.y + y);
    let corners = [(inner_x, -inner_y), (inner_x, inner_y), (-inner_x, inner_y), (-inner_x, -inner_y)];
    // Each side runs up to a corner, whose arc turns a quarter about the corner's centre.
    let sides = [(-inner_x, -half_y), (half_x, -inner_y), (inner_x, half_y), (-half_x, inner_y)];
    let ends = [(inner_x, -half_y), (half_x, inner_y), (-inner_x, half_y), (-half_x, -inner_y)];
    let after = [(half_x, -inner_y), (inner_x, half_y), (-half_x, inner_y), (-inner_x, -half_y)];
    let mut strokes = Vec::with_capacity(8);
    for k in 0..4 {
        strokes.push(Stroke::Line { from: at(sides[k].0, sides[k].1), to: at(ends[k].0, ends[k].1) });
        strokes.push(Stroke::Arc {
            center: at(corners[k].0, corners[k].1),
            from: at(ends[k].0, ends[k].1),
            to: at(after[k].0, after[k].1),
            counter_clockwise: true,
        });
    }
    Ok(strokes)
}

fn number(word: Option<&str>, command: &str) -> Result<f64, Error> {
    word.and_then(|text| text.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .ok_or_else(|| Error::input(format!("`{command}` is followed by {word:?} where a number belongs")))
}

fn pair(word: Option<&str>, command: &str) -> Result<Uv, Error> {
    let both = word.and_then(|text| text.split_once(','));
    match both {
        Some((x, y)) => Ok(Uv::new(number(Some(x), command)?, number(Some(y), command)?)),
        None => Err(Error::input(format!("`{command}` is followed by {word:?} where a point x,y belongs"))),
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, made};
    use krly3d_core::Id;
    use krly3d_kernel::ports::tool::run;
    use krly3d_kernel::schema::Params;
    use std::f64::consts::PI;

    fn path(text: &str) -> Params {
        Params::new().with("path", text)
    }

    #[test]
    fn a_closed_polygon_becomes_one_face_with_an_edge_per_side() {
        let cx = context();

        let profile = made(&cx, "profile", path("M 0,0 L 4,0 L 4,3 Z"), &[]);

        assert_eq!((profile.faces.len(), profile.edges.len(), profile.vertices.len()), (1, 3, 3));
        assert!(agree(area(&cx, &profile), 6.0));
    }

    #[test]
    fn the_drawing_direction_does_not_matter() {
        let cx = context();

        let clockwise = made(&cx, "profile", path("M 0,0 L 0,3 L 4,3 L 4,0 Z"), &[]);

        assert!(agree(area(&cx, &clockwise), 12.0));
    }

    #[test]
    fn an_arc_bulges_the_region_by_its_segment() {
        let cx = context();

        // A unit square with a half disc of radius 0.5 added on its right side.
        let profile = made(&cx, "profile", path("M 0,0 L 1,0 A 1,0.5 1,1 ccw L 0,1 Z"), &[]);

        assert!(agree(area(&cx, &profile), 1.0 + PI * 0.125));
    }

    #[test]
    fn a_rounded_rectangle_has_four_sides_four_arcs_and_the_area_of_its_corners_cut() {
        let cx = context();

        let profile = made(&cx, "profile", path("R 5,-2 10,6 1.5"), &[]);

        assert_eq!(profile.edges.len(), 8);
        assert!(agree(area(&cx, &profile), 60.0 - (4.0 - PI) * 1.5 * 1.5));
    }

    #[test]
    fn a_rounded_rectangle_refuses_a_corner_as_large_as_half_its_side() {
        let cx = context();

        let refused = run(&cx, &Id::new("profile"), &path("R 0,0 10,6 3"), &[]).into_result();

        assert!(refused.is_err());
    }

    #[test]
    fn a_circle_inside_the_outline_is_a_hole() {
        let cx = context();

        let plate = made(&cx, "profile", path("M -30,-20 L 30,-20 L 30,20 L -30,20 Z O 20,15 4"), &[]);

        assert!(agree(area(&cx, &plate), 2400.0 - 16.0 * PI));
        assert_eq!(plate.faces[0].loops.len(), 2);
    }

    #[test]
    fn edges_take_the_given_names_and_then_numbered_ones() {
        let cx = context();
        let params = path("M 0,0 L 4,0 L 4,3 Z").with("names", vec!["base".into(), "rise".into()]);

        let profile = made(&cx, "profile", params, &[]);

        let labels: Vec<&str> = profile.edges.iter().filter_map(|edge| edge.label.as_deref()).collect();
        assert_eq!(labels, ["base", "rise", "s3"]);
    }

    #[test]
    fn a_path_left_open_is_rejected() {
        let error =
            run(&context(), &Id::new("profile"), &path("M 0,0 L 4,0 L 4,3"), &[]).into_result().unwrap_err();

        assert_eq!(error.message, "the path ends inside an open loop; close it with Z");
    }

    #[test]
    fn an_arc_that_misses_its_radius_is_rejected() {
        let error = run(&context(), &Id::new("profile"), &path("M 1,0 A 0,0 0,2 ccw Z"), &[])
            .into_result()
            .unwrap_err();

        assert!(error.message.contains("does not end at the radius it starts from"), "{error}");
    }
}
