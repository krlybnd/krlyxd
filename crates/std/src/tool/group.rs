//! Several bodies kept together as one, without joining them.

use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::Schema;
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};

pub struct Group;

impl Plugin<Tool> for Group {
    fn id(&self) -> Id {
        Id::new("group")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "the given bodies as one body, as they are: nothing is joined or cut, so they may touch or overlap; faces are named node:face",
            inputs: (2, usize::MAX),
            params: Vec::new(),
        })
    }

    fn call(&self, _: &Context, request: &ToolRequest) -> Attempt<Body> {
        if request.inputs.len() < 2 {
            return Attempt::Failed(Error::input("a group takes at least two bodies"));
        }
        let parts: Vec<Body> = request.inputs.iter().map(|body| Body::clone(body)).collect();
        let source = |index: usize| match request.sources.get(index) {
            Some(name) if !name.is_empty() => name.clone(),
            _ => format!("part{}", index + 1),
        };
        let mut grouped = Body::merged(&parts, |index, face| format!("{}:{face}", source(index)));
        grouped.name_apart();
        Attempt::Done(grouped)
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, context, face_names, made, volume};
    use krly3d_core::Id;
    use krly3d_kernel::ports::tool::run_named;
    use krly3d_kernel::schema::Params;

    #[test]
    fn a_group_keeps_its_parts_as_they_are_and_names_faces_after_their_nodes() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 2.0).with("d", 2.0).with("h", 2.0), &[]);
        let rod = made(
            &cx,
            "cylinder",
            Params::new().with("r", 0.5).with("h", 4.0).with("at", [5.0, 0.0, 0.0]),
            &[],
        );
        let sources = ["block".to_string(), "rod".to_string()];

        let group =
            run_named(&cx, &Id::new("group"), &Params::new(), &[block, rod], &sources).into_result().unwrap();

        assert_eq!(face_names(&group)[..3], ["block:back", "block:bottom", "block:front"]);
        assert!(group.face("rod:side").is_some());
        assert!(agree(volume(&cx, &std::sync::Arc::new(group)), 8.0 + std::f64::consts::PI));
    }
}
