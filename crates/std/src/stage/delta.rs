//! Edit stage: says what the edit changed, by node and by named face.

use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::model::{Delta, NodeDelta, NodeRef};
use krly3d_kernel::ports::measure::measure;
use krly3d_kernel::ports::stage::{EditRun, EditStage};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};
use std::collections::BTreeSet;

/// Registered as `delta`; named after what it does because `Delta` is what it makes.
pub struct Diff;

impl Plugin<EditStage> for Diff {
    fn id(&self) -> Id {
        Id::new("delta")
    }

    fn call(&self, cx: &Context, run: &EditRun) -> Attempt<EditRun> {
        let Some(after) = &run.model_after else {
            return Attempt::Failed(Error::unsupported(
                "the `delta` stage needs a built model; put `build` before it",
            ));
        };
        let head = |name: &str| NodeRef { name: name.to_string(), version: None };
        let mut delta = Delta::default();
        for (name, versions) in after.nodes() {
            let (Some(now), was) = (after.built(&head(name)), run.model_before.built(&head(name))) else {
                continue;
            };
            if was.is_some_and(|was| was.key == now.key) {
                continue;
            }
            let tool = run
                .after
                .statements()
                .filter(|statement| statement.name == name)
                .last()
                .map(|statement| statement.op.tool.clone());
            let (old, new) = (names(was.map(|was| was.body.as_ref())), names(Some(now.body.as_ref())));
            delta.nodes.push(NodeDelta {
                node: name.to_string(),
                version: versions as u32,
                tool: tool.unwrap_or_else(|| Id::new("?")),
                added: new.difference(&old).map(|name| name.to_string()).collect(),
                removed: old.difference(&new).map(|name| name.to_string()).collect(),
                faces: (old.len(), new.len()),
                volume: if now.body.solid {
                    measure(cx, "volume", &now.body).into_result().ok()
                } else {
                    None
                },
            });
        }
        let kept: BTreeSet<&str> = after.nodes().map(|(name, _)| name).collect();
        delta.gone = run
            .model_before
            .nodes()
            .map(|(name, _)| name)
            .filter(|name| !kept.contains(name))
            .map(str::to_string)
            .collect();
        Attempt::Done(EditRun { delta: Some(delta), ..run.clone() })
    }
}

/// The face names of a body, or none for a node that did not exist.
fn names(body: Option<&Body>) -> BTreeSet<&str> {
    body.map(|body| body.faces.iter().map(|face| face.name.as_str()).collect()).unwrap_or_default()
}
