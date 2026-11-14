//! A solid through two or more flat sections: its side is the NURBS surface that passes
//! through every section, its ends are the first and the last.
//!
//! Every section is turned into one curve exactly: lines as segments, arcs as rational
//! quadratics, all raised to degree two and joined edge after edge, edge `k` of `n` taking
//! the parameters `[k/n, (k+1)/n]`. Sections drawn with the same number of edges therefore
//! correspond edge to edge (a square to the four quarters of a circle); the curves are then
//! given the same knots by inserting each other's. Their control points are interpolated
//! from section to section (cubic when there are four sections or more, otherwise of
//! degree one less than the sections), with parameters spaced by the distances between the
//! sections' centres. The side meets each end in exactly the section's own edges.

use crate::curve::{Circle, Ellipse, Line};
use crate::surface::Nurbs;
use krly3d_core::math::nurbs::{
    self, averaged_knots, chord_parameters, elevated, inserted, interpolate, Weighted,
};
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::Schema;
use krly3d_kernel::topo::{Body, BodyBuilder, Bound, EdgeId, Use};
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct Loft;

impl Plugin<Tool> for Loft {
    fn id(&self) -> Id {
        Id::new("loft")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "a solid passing through flat profiles in the order given; edge k of one section runs to edge k of the next, so draw them with as many edges and starting alike; faces: start, end, side",
            inputs: (2, usize::MAX),
            params: Vec::new(),
        })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        loft(cx, &request.inputs).into()
    }
}

/// One section: the profile, its loop in order, and the loop as one curve.
struct Section<'a> {
    profile: &'a Body,
    uses: Vec<Use>,
    curve: nurbs::Curve,
    centre: Point3,
}

fn loft(cx: &Context, inputs: &[Arc<Body>]) -> Result<Body, Error> {
    let mut sections = inputs.iter().map(|profile| section(profile)).collect::<Result<Vec<_>, Error>>()?;
    compatible(&mut sections)?;
    let surface = Nurbs::new(skinned(&sections)?)?;
    build(cx, &sections, surface)
}

fn section(profile: &Body) -> Result<Section<'_>, Error> {
    let face = match (profile.solid, profile.faces.as_slice()) {
        (false, [face]) if face.loops.len() == 1 => face,
        _ => return Err(Error::input("loft takes flat profiles of one outline each, without holes")),
    };
    let uses = face.loops[0].clone();
    let pieces = uses.len();
    let mut joined: Option<nurbs::Curve> = None;
    for (index, used) in uses.iter().enumerate() {
        let piece = degree_two(&piece_of(profile, used)?)?;
        let shifted =
            reparametrised(&piece, index as f64 / pieces as f64, (index + 1) as f64 / pieces as f64);
        joined = Some(match joined {
            None => shifted,
            Some(before) => joined_curves(&before, &shifted)?,
        });
    }
    let curve = joined.ok_or_else(|| Error::input("a profile without edges"))?;
    let points: Vec<Point3> = profile.edges.iter().map(|edge| edge.point(0.5)).collect();
    let centre =
        Point3::from(points.iter().map(|point| point.coords).sum::<Vec3>() / points.len().max(1) as f64);
    Ok(Section { profile, uses, curve, centre })
}

/// One edge as a NURBS curve, in the direction the loop runs it.
fn piece_of(profile: &Body, used: &Use) -> Result<nurbs::Curve, Error> {
    let edge = &profile.edges[used.edge];
    let (from, to) = if used.forward { (edge.range.0, edge.range.1) } else { (edge.range.1, edge.range.0) };
    if let Some(circle) = edge.curve.as_any().downcast_ref::<Circle>() {
        return nurbs::arc(&circle.center(), &circle.axis(), &circle.point(from), to - from);
    }
    if edge.curve.as_any().downcast_ref::<Line>().is_some() {
        return nurbs::segment(&edge.curve.point(from), &edge.curve.point(to));
    }
    if let Some(ellipse) = edge.curve.as_any().downcast_ref::<Ellipse>() {
        // The unit circle stretched by the semi-axes: an affine image of a rational arc is
        // the rational arc of the image, so the ellipse stays exact.
        let unit =
            nurbs::arc(&Point3::origin(), &Vec3::z(), &Point3::new(from.cos(), from.sin(), 0.0), to - from)?;
        let (a, b) = ellipse.semi_axes();
        let placed = *ellipse.frame();
        return Ok(unit.mapped(|point| placed * Point3::new(point.x * a, point.y * b, 0.0)));
    }
    Err(Error::unsupported("a loft section may hold only lines, circular arcs and ellipses"))
}

fn degree_two(curve: &nurbs::Curve) -> Result<nurbs::Curve, Error> {
    if curve.basis.degree == 1 {
        elevated(curve)
    } else {
        Ok(curve.clone())
    }
}

/// The same curve with its domain mapped onto `[from, to]`.
fn reparametrised(curve: &nurbs::Curve, from: f64, to: f64) -> nurbs::Curve {
    let (start, end) = curve.domain();
    let knots =
        curve.basis.knots.iter().map(|knot| from + (knot - start) / (end - start) * (to - from)).collect();
    nurbs::Curve { basis: nurbs::Basis { degree: curve.basis.degree, knots }, points: curve.points.clone() }
}

/// Two curves of the same degree, the second starting where the first ends, as one: the
/// joint keeps a knot of full multiplicity, so the shape is unchanged.
fn joined_curves(first: &nurbs::Curve, second: &nurbs::Curve) -> Result<nurbs::Curve, Error> {
    let p = first.basis.degree;
    // The weights of the two must agree at the joint; scaling a rational curve's
    // homogeneous points by a constant does not change it.
    let scale = first.points[first.points.len() - 1].w / second.points[0].w;
    let mut points = first.points.clone();
    points.extend(second.points.iter().skip(1).map(|point| point * scale));
    let mut knots = first.basis.knots[..first.basis.knots.len() - 1].to_vec();
    knots.extend(second.basis.knots.iter().skip(p + 1));
    nurbs::Curve::new(p, knots, points)
}

/// Gives every section the union of all their knots.
fn compatible(sections: &mut [Section<'_>]) -> Result<(), Error> {
    let mut all: Vec<f64> = Vec::new();
    for section in sections.iter() {
        let knots = &section.curve.basis.knots;
        let mut counted: Vec<(f64, usize)> = Vec::new();
        for knot in knots {
            match counted.last_mut() {
                Some((value, count)) if value == knot => *count += 1,
                _ => counted.push((*knot, 1)),
            }
        }
        for (value, count) in counted {
            let have = all.iter().filter(|knot| **knot == value).count();
            all.extend(std::iter::repeat_n(value, count.saturating_sub(have)));
        }
    }
    all.sort_by(f64::total_cmp);
    for section in sections.iter_mut() {
        for (value, wanted) in multiplicities(&all) {
            let have = section.curve.basis.knots.iter().filter(|knot| **knot == value).count();
            for _ in have..wanted {
                section.curve = inserted(&section.curve, value)?;
            }
        }
    }
    Ok(())
}

fn multiplicities(knots: &[f64]) -> Vec<(f64, usize)> {
    let mut counted: Vec<(f64, usize)> = Vec::new();
    for knot in knots {
        match counted.last_mut() {
            Some((value, count)) if value == knot => *count += 1,
            _ => counted.push((*knot, 1)),
        }
    }
    counted
}

/// The surface through the sections: their control points interpolated across them.
fn skinned(sections: &[Section<'_>]) -> Result<nurbs::Surface, Error> {
    let degree = (sections.len() - 1).min(3);
    let distances: Vec<f64> =
        sections.windows(2).map(|pair| (pair[1].centre - pair[0].centre).norm()).collect();
    let parameters = chord_parameters(&distances);
    let knots = averaged_knots(&parameters, degree);
    let count = sections[0].curve.points.len();
    let mut net: Vec<Vec<Weighted>> = Vec::with_capacity(count);
    for i in 0..count {
        let column: Vec<Weighted> = sections.iter().map(|section| section.curve.points[i]).collect();
        net.push(interpolate(&column, &parameters, degree, &knots)?);
    }
    let u = sections[0].curve.basis.clone();
    let v = nurbs::Basis::new(degree, knots, sections.len())?;
    nurbs::Surface::new(u, v, net)
}

fn build(cx: &Context, sections: &[Section<'_>], surface: Nurbs) -> Result<Body, Error> {
    let mut builder = BodyBuilder::new(cx.accuracy());
    let ends = [&sections[0], &sections[sections.len() - 1]];
    let mut rims: Vec<Vec<EdgeId>> = Vec::with_capacity(2);
    for section in ends {
        let edges = section.profile.edges.iter().map(|edge| match edge.ends {
            Some(_) => Ok(builder.edge(Arc::clone(&edge.curve), edge.range.0, edge.range.1)),
            None => builder.ring(Arc::clone(&edge.curve)),
        });
        rims.push(edges.collect::<Result<Vec<_>, Error>>()?);
    }
    let along_v = |point: &Point3| {
        let uv = surface.project(point);
        let d = surface.data().derivatives(uv.x, uv.y, 1);
        d[0][1]
    };
    // The start lies against the direction the side runs, the end along it.
    for (which, (section, name)) in ends.iter().zip(["start", "end"]).enumerate() {
        let face = &section.profile.faces[0];
        let anchor = section.profile.edges[0].point(0.5);
        let out = along_v(&anchor) * if which == 0 { -1.0 } else { 1.0 };
        let bounds: Vec<Bound> = section
            .uses
            .iter()
            .map(|used| {
                let edge = &section.profile.edges[used.edge];
                let middle = edge.point(0.5);
                let tangent = edge.curve.tangent(edge.parameter(0.5)) * if used.forward { 1.0 } else { -1.0 };
                // Inward in the plane of the section: the region is to the left of the loop
                // seen from the side its normal points to.
                Bound::new(rims[which][used.edge], middle, face.outward_normal(&middle).cross(&tangent))
            })
            .collect();
        builder.face(name, Arc::clone(&face.surface), &anchor, &out, &bounds)?;
    }
    let mut bounds = Vec::new();
    for (which, section) in ends.iter().enumerate() {
        for used in &section.uses {
            let middle = section.profile.edges[used.edge].point(0.5);
            let inward = along_v(&middle) * if which == 0 { 1.0 } else { -1.0 };
            bounds.push(Bound::new(rims[which][used.edge], middle, inward));
        }
    }
    // Out of the side is away from the region of the first section.
    let first = ends[0];
    let used = &first.uses[0];
    let edge = &first.profile.edges[used.edge];
    let (middle, tangent) =
        (edge.point(0.5), edge.curve.tangent(edge.parameter(0.5)) * if used.forward { 1.0 } else { -1.0 });
    let outward = tangent.cross(&first.profile.faces[0].outward_normal(&middle));
    builder.face("side", Arc::new(surface), &middle, &outward, &bounds)?;
    Ok(builder.finish(true))
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{context, face_names, made, measured};
    use krly3d_core::{Attempt, Id};
    use krly3d_kernel::ports::tool::run;
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::topo::{body_bounds, Body};
    use krly3d_kernel::Context;
    use std::f64::consts::PI;
    use std::sync::Arc;

    fn at(cx: &Context, path: &str, height: f64) -> Arc<Body> {
        made(cx, "profile", Params::new().with("path", path).with("at", [0.0, 0.0, height]), &[])
    }

    fn close(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() <= 1e-9 * expected.abs()
    }

    #[test]
    fn a_loft_through_four_sections_is_bounded_by_its_widest_section_on_every_side() {
        let cx = context();
        let sections: Vec<Arc<Body>> = [
            ("R 0,0 40,20 5", 0.0),
            ("R 0,0 50,30 5", 10.0),
            ("R 0,0 40,20 5", 20.0),
            ("R 0,0 40,20 5", 30.0),
        ]
        .iter()
        .map(|(path, height)| at(&cx, path, *height))
        .collect();

        let solid = made(&cx, "loft", Params::new(), &sections);

        // The straight sides are flat across, so the widest point of each side is a whole
        // line of extremes, which the bounds must still reach on both sides alike.
        let around = body_bounds(&cx, &solid).into_result().unwrap();
        for (axis, reach) in [(0, 25.0), (1, 15.0)] {
            assert!(around.low[axis] <= -reach + 1e-9 && around.high[axis] >= reach - 1e-9, "{around:?}");
        }
    }

    #[test]
    fn a_loft_between_two_circles_is_a_frustum_of_a_cone() {
        let cx = context();

        let solid = made(&cx, "loft", Params::new(), &[at(&cx, "O 0,0 4", 0.0), at(&cx, "O 0,0 2", 6.0)]);

        assert_eq!(face_names(&solid), ["end", "side", "start"]);
        let volume = measured(&cx, "volume", 3, &solid);
        assert!(close(volume, PI * 6.0 / 3.0 * (16.0 + 8.0 + 4.0)), "{volume}");
    }

    #[test]
    fn a_loft_through_three_equal_circles_in_a_line_is_a_cylinder() {
        let cx = context();
        let sections: Vec<Arc<Body>> = [0.0, 3.0, 10.0].iter().map(|z| at(&cx, "O 1,1 3", *z)).collect();

        let solid = made(&cx, "loft", Params::new(), &sections);

        assert!(close(measured(&cx, "volume", 3, &solid), PI * 9.0 * 10.0));
        assert!(close(measured(&cx, "area", 2, &solid), 2.0 * PI * 9.0 + 2.0 * PI * 3.0 * 10.0));
    }

    #[test]
    fn a_square_lofted_to_a_circle_keeps_the_volume_of_its_ruled_sides() {
        let cx = context();
        // Between a cylinder on the circle and a prism on the square.
        let square = "M 2,-2 L 2,2 L -2,2 L -2,-2 Z";
        let circle = "M 2,0 A 0,0 0,2 ccw A 0,0 -2,0 ccw A 0,0 0,-2 ccw A 0,0 2,0 ccw";
        let circle = format!("{circle} Z");
        let solid = made(&cx, "loft", Params::new(), &[at(&cx, square, 0.0), at(&cx, &circle, 4.0)]);

        let volume = measured(&cx, "volume", 3, &solid);
        assert!(volume > 4.0 * PI * 4.0 && volume < 16.0 * 4.0, "{volume}");
    }

    #[test]
    fn a_loft_between_two_ellipses_has_the_volume_of_its_quadratic_sections() {
        let cx = context();

        let solid = made(&cx, "loft", Params::new(), &[at(&cx, "E 0,0 4,2", 0.0), at(&cx, "E 0,0 2,3", 6.0)]);

        // Two sections make a ruled solid whose section at s ∈ [0, 1] is the ellipse with
        // semi-axes 4 − 2s and 2 + s, of area π(4 − 2s)(2 + s); its integral over the height
        // is 6π·∫(8 − 2s²)ds = 6π·(8 − 2/3).
        let volume = measured(&cx, "volume", 3, &solid);
        assert!(close(volume, 6.0 * PI * (8.0 - 2.0 / 3.0)), "{volume}");
    }

    #[test]
    fn a_section_with_a_hole_is_refused() {
        let cx = context();

        let attempt = run(
            &cx,
            &Id::new("loft"),
            &Params::new(),
            &[at(&cx, "O 0,0 4 O 0,0 1", 0.0), at(&cx, "O 0,0 2", 3.0)],
        );

        assert!(matches!(attempt, Attempt::Failed(_)));
    }
}
