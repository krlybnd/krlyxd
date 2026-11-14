//! `faces`: the zoomed-out view of a node, one line per face.

use super::{node, point_value, surface_text, vector_value};
use krly3d_core::{text, Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Schema, Value};
use krly3d_kernel::topo::anchor;
use krly3d_kernel::{Context, Plugin};

pub struct Faces;

impl Plugin<Query> for Faces {
    fn id(&self) -> Id {
        Id::new("faces")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "the faces of a node, one line each: name, surface, and a point that lies on the face",
            inputs: (0, 0),
            params: vec![node()],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        faces(cx, request).into()
    }
}

fn faces(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let body = &request.node("node")?.body;
    let (mut lines, mut listed) = (Vec::new(), Vec::new());
    for (face, data) in body.faces.iter().enumerate() {
        let anchor = anchor(cx, body, face).into_result()?;
        let surface = surface_text(data, &anchor)?;
        lines.push(format!("{}: {surface} at {}", data.name, text::point(&anchor.point)));
        listed.push(Value::map([
            ("name", Value::from(data.name.as_str())),
            ("surface", Value::from(surface)),
            ("at", point_value(&anchor.point)),
            ("normal", vector_value(&anchor.normal)),
        ]));
    }
    Ok(Answer {
        text: lines.join("\n"),
        data: Value::map([("faces", Value::List(listed))]),
        exact: Exactness::Exact,
    })
}

#[cfg(test)]
mod tests {
    use crate::query::testing::{asked, block, entry, items, kernel, part, refused, triple, word};
    use krly3d_core::math::Point3;
    use krly3d_core::{Attempt, ErrorKind};
    use krly3d_kernel::ports::trim::{place, Place, TrimRequest};
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::Kernel;
    use std::sync::Arc;

    /// One node of every kind of solid there is a tool for, and a flat profile.
    fn assortment() -> Kernel {
        let upright = |path: &str| Params::new().with("path", path).with("plane", "xz");
        let holed = Params::new().with("path", "M -30,-20 L 30,-20 L 30,20 L -30,20 Z O 20,15 4");
        kernel(vec![
            ("can", "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[]),
            ("ball", "sphere", Params::new().with("r", 5.0), &[]),
            ("ring", "torus", Params::new().with("ring", 6.0).with("tube", 2.0), &[]),
            ("cone", "cone", Params::new().with("r", 4.0).with("h", 9.0), &[]),
            ("section", "profile", upright("M 4,0 L 6,0 L 6,2 L 4,2 Z"), &[]),
            ("washer", "revolve", Params::new(), &["section"]),
            ("wedge", "revolve", Params::new().with("angle", 250.0), &["section"]),
            ("outline", "profile", holed, &[]),
            ("plate", "extrude", Params::new().with("h", 10.0), &["outline"]),
        ])
    }

    #[test]
    fn a_block_lists_each_face_with_the_way_it_faces_and_its_centre() {
        let block = block();

        let answer = asked(&block, "faces", part());

        let mut lines: Vec<&str> = answer.text.lines().collect();
        lines.sort_unstable();
        assert_eq!(
            lines,
            [
                "back: plane n=+y at (0 5 10)",
                "bottom: plane n=-z at (0 0 0)",
                "front: plane n=-y at (0 -5 10)",
                "left: plane n=-x at (-20 0 10)",
                "right: plane n=+x at (20 0 10)",
                "top: plane n=+z at (0 0 20)",
            ]
        );
    }

    #[test]
    fn curved_faces_are_described_by_their_surface() {
        let assortment = assortment();
        let lines = |node: &str| asked(&assortment, "faces", Params::new().with("node", node)).text;

        assert_eq!(lines("ball"), "skin: sphere r=5 at (5 0 0)");
        assert_eq!(lines("ring"), "skin: torus ring=6 tube=2 axis=+z at (8 0 0)");
        assert!(lines("can").contains("side: cylinder r=3 axis=+z at "), "{}", lines("can"));
    }

    #[test]
    fn the_point_given_for_a_face_lies_inside_the_face_with_the_outward_normal_there() {
        let assortment = assortment();
        for node in ["can", "ball", "ring", "cone", "section", "washer", "wedge", "plate"] {
            let body = assortment.body(node).unwrap();

            let answer = asked(&assortment, "faces", Params::new().with("node", node));

            let listed = items(entry(&answer.data, "faces"));
            assert_eq!(listed.len(), body.faces.len());
            for (face, stated) in listed.iter().enumerate() {
                let point = Point3::from(triple(entry(stated, "at")));
                let placed =
                    place(assortment.context(), &TrimRequest { body: Arc::clone(&body), face, point });
                assert_eq!(word(entry(stated, "name")), body.faces[face].name);
                assert_eq!(placed, Attempt::Done(Place::Inside), "{node}: {stated:?}");
                assert!(body.faces[face].surface.distance(&point).abs() < 1e-9, "{node}: {stated:?}");
                assert_eq!(
                    triple(entry(stated, "normal")),
                    <[f64; 3]>::from(body.faces[face].outward_normal(&point)),
                    "{node}: {stated:?}"
                );
            }
        }
    }

    #[test]
    fn a_face_around_a_hole_is_given_a_point_on_its_material() {
        let assortment = assortment();

        let answer = asked(&assortment, "faces", Params::new().with("node", "washer"));

        assert!(answer.text.contains("s3: plane n=+z at (-4.5 0 2)"), "{}", answer.text);
    }

    #[test]
    fn an_earlier_version_of_a_node_is_asked_for_by_its_number() {
        let mut block = block();
        let lift = Params::new().with("move", [0.0, 0.0, 5.0]);
        block
            .apply(krly3d_kernel::model::Statement::new("part", "transform", lift, &["part"]).unwrap())
            .unwrap();

        let (first, latest) =
            (asked(&block, "faces", Params::new().with("node", "part@1")), asked(&block, "faces", part()));

        assert!(first.text.contains("top: plane n=+z at (0 0 20)"), "{}", first.text);
        assert!(latest.text.contains("top: plane n=+z at (0 0 25)"), "{}", latest.text);
    }

    #[test]
    fn a_node_that_does_not_exist_is_refused_by_name() {
        let error = refused(&block(), "faces", Params::new().with("node", "lid"));

        assert_eq!((error.kind, error.message.as_str()), (ErrorKind::NotFound, "there is no node `lid`"));
    }
}
