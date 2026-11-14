//! `ray`: what a ray meets, read as a sentence.

use super::{face_name, node, point_value};
use krly3d_core::{text, Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::probe::{self, Hit};
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Kind, Param, Schema, Value};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};
use std::fmt::Write;

pub struct Ray;

impl Plugin<Query> for Ray {
    fn id(&self) -> Id {
        Id::new("ray")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "what a ray meets, as a sentence: `air 45 > [front] solid 10 > [back] air` reads air for 45, then face front, solid for 10, then face back, then air",
            inputs: (0, 0),
            params: vec![
                node(),
                Param::required("origin", Kind::Triple, "where the ray starts"),
                Param::required("direction", Kind::Triple, "which way it goes; its length does not matter"),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        ray(cx, request).into()
    }
}

fn ray(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let body = &request.node("node")?.body;
    let (origin, direction) = (request.params.point("origin")?, request.params.vector("direction")?);
    let length = direction.norm();
    if !(length.is_finite() && length > 0.0) {
        return Err(Error::input("the direction of a ray needs a length"));
    }
    let on_line = probe::cast(cx, body, vec![probe::Ray { origin, direction }]).into_result()?;
    // The probe answers for the whole line. The ray is the part from the origin on; a hit
    // within the length tolerance of the origin is where the ray starts.
    let ahead: Vec<(f64, &Hit)> = on_line
        .iter()
        .flatten()
        .map(|hit| ((hit.t * length).max(0.0), hit))
        .filter(|(_, hit)| hit.t * length >= -cx.accuracy().length())
        .collect();
    let hits = ahead.iter().map(|(distance, hit)| {
        Value::map([
            ("face", Value::from(face_name(body, hit.face))),
            ("distance", Value::from(*distance)),
            ("point", point_value(&hit.point)),
            ("entering", Value::from(hit.entering)),
            ("on_edge", Value::from(hit.on_edge)),
        ])
    });
    Ok(Answer {
        text: sentence(body, &ahead),
        data: Value::map([
            ("inside", Value::from(starts_inside(body, &ahead))),
            ("hits", Value::List(hits.collect())),
        ]),
        exact: Exactness::Exact,
    })
}

/// Before its first hit a ray is on the side that the hit does not lead to. A ray that
/// hits nothing is outside: from inside a solid every direction leads out.
fn starts_inside(body: &Body, ahead: &[(f64, &Hit)]) -> bool {
    body.solid && ahead.first().is_some_and(|(_, hit)| !hit.entering)
}

/// What the ray is in, for how long, and the face that ends it, hit after hit.
fn sentence(body: &Body, ahead: &[(f64, &Hit)]) -> String {
    let medium = |inside: bool| if inside { "solid" } else { "air" };
    let (mut inside, mut passed) = (starts_inside(body, ahead), 0.0);
    let mut text = String::new();
    for (distance, hit) in ahead {
        let stretch = text::num(distance - passed);
        let _ = write!(text, "{} {stretch} > [{}] ", medium(inside), face_name(body, hit.face));
        // A sheet has two sides and no inside.
        inside = body.solid && hit.entering;
        passed = *distance;
    }
    text.push_str(medium(inside));
    text
}

#[cfg(test)]
mod tests {
    use crate::query::testing::{asked, block, entry, items, part, plate_with_hole, refused, triple, word};
    use krly3d_core::ErrorKind;
    use krly3d_kernel::schema::{Params, Value};

    fn from(origin: [f64; 3], direction: [f64; 3]) -> Params {
        part().with("origin", origin).with("direction", direction)
    }

    #[test]
    fn a_ray_through_a_block_reads_air_solid_air_with_the_two_faces_between() {
        let answer = asked(&block(), "ray", from([0.0, -50.0, 10.0], [0.0, 1.0, 0.0]));

        assert_eq!(answer.text, "air 45 > [front] solid 10 > [back] air");
    }

    #[test]
    fn a_ray_from_inside_the_material_starts_with_solid() {
        let answer = asked(&block(), "ray", from([0.0, 0.0, 10.0], [0.0, 1.0, 0.0]));

        assert_eq!(answer.text, "solid 5 > [back] air");
        assert_eq!(entry(&answer.data, "inside"), &Value::from(true));
    }

    #[test]
    fn a_ray_that_meets_nothing_is_all_air() {
        let answer = asked(&block(), "ray", from([0.0, -50.0, 30.0], [0.0, 1.0, 0.0]));

        assert_eq!(answer.text, "air");
        assert!(items(entry(&answer.data, "hits")).is_empty());
    }

    #[test]
    fn what_lies_behind_the_origin_is_not_part_of_the_ray() {
        let answer = asked(&block(), "ray", from([0.0, 50.0, 10.0], [0.0, 1.0, 0.0]));

        assert_eq!(answer.text, "air");
    }

    #[test]
    fn a_ray_across_a_hole_leaves_the_material_and_enters_it_again() {
        let answer = asked(&plate_with_hole(), "ray", from([20.0, -50.0, 5.0], [0.0, 1.0, 0.0]));

        assert_eq!(answer.text, "air 30 > [s1] solid 31 > [s5] air 8 > [s5] solid 1 > [s3] air");
    }

    #[test]
    fn distances_are_lengths_whatever_the_length_of_the_direction() {
        let answer = asked(&block(), "ray", from([0.0, -50.0, 10.0], [0.0, 4.0, 0.0]));

        assert_eq!(answer.text, "air 45 > [front] solid 10 > [back] air");
    }

    #[test]
    fn a_ray_that_starts_on_a_face_meets_it_after_no_distance() {
        let answer = asked(&block(), "ray", from([0.0, -5.0, 10.0], [0.0, 1.0, 0.0]));

        assert_eq!(answer.text, "air 0 > [front] solid 10 > [back] air");
    }

    #[test]
    fn a_sheet_is_crossed_without_entering_anything() {
        let outline = Params::new().with("node", "outline");

        let answer = asked(
            &plate_with_hole(),
            "ray",
            outline.with("origin", [0.0, 0.0, -5.0]).with("direction", [0.0, 0.0, 1.0]),
        );

        assert_eq!(answer.text, "air 5 > [profile] air");
    }

    #[test]
    fn every_hit_comes_as_data_with_its_face_and_point() {
        let answer = asked(&block(), "ray", from([0.0, -50.0, 10.0], [0.0, 1.0, 0.0]));

        let hits = items(entry(&answer.data, "hits"));
        assert_eq!(hits.len(), 2);
        assert_eq!(word(entry(&hits[0], "face")), "front");
        assert_eq!(triple(entry(&hits[0], "point")), [0.0, -5.0, 10.0]);
        assert_eq!(entry(&hits[0], "distance"), &Value::from(45.0));
        assert_eq!(
            (entry(&hits[0], "entering"), entry(&hits[1], "entering")),
            (&Value::from(true), &Value::from(false))
        );
    }

    #[test]
    fn a_direction_without_length_is_refused() {
        let error = refused(&block(), "ray", from([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]));

        assert_eq!(error.kind, ErrorKind::Input);
    }
}
