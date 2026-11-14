//! Node stage: makes every face name of the body a usable handle.

use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::stage::{NodeRun, NodeStage};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

pub struct Name;

impl Plugin<NodeStage> for Name {
    fn id(&self) -> Id {
        Id::new("name")
    }

    fn call(&self, _: &Context, run: &NodeRun) -> Attempt<NodeRun> {
        let Some(body) = &run.body else {
            return Attempt::Failed(Error::unsupported("the `name` stage needs a body; put `run` before it"));
        };
        let mut named = Body::clone(body);
        named.name_apart();
        Attempt::Done(NodeRun { body: Some(Arc::new(named)), ..run.clone() })
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{context, made};
    use krly3d_core::{Attempt, Id};
    use krly3d_kernel::ports::stage::{contract, NodeRun, NodeStage};
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::topo::Body;
    use std::sync::Arc;

    #[test]
    fn repeated_face_names_get_a_running_number() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 1.0).with("d", 1.0).with("h", 1.0), &[]);
        let twice = Body::merged(&[Body::clone(&block), Body::clone(&block)], |_, name| name.to_string());
        let run = NodeRun {
            node: "pair".into(),
            tool: Id::new("box"),
            params: Params::new(),
            inputs: vec![],
            sources: vec![],
            body: Some(Arc::new(twice)),
            shape: None,
        };

        let Attempt::Done(named) = cx.call::<NodeStage>(&Id::new("name"), &run) else { panic!() };

        let body = named.body.unwrap();
        assert!(body.face("top").is_some() && body.face("top#2").is_some());
        contract::check(&cx, &Id::new("name"), &run).unwrap();
    }
}
