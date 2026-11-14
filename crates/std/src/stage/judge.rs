//! Edit stage: holds an edit to the expectations stated with its statements.

use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::model::{Delta, NodeRef};
use krly3d_kernel::ports::stage::{EditRun, EditStage};
use krly3d_kernel::predicate::{Claim, Verdict};
use krly3d_kernel::{Context, Plugin};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct Judge;

impl Plugin<EditStage> for Judge {
    fn id(&self) -> Id {
        Id::new("judge")
    }

    fn call(&self, cx: &Context, run: &EditRun) -> Attempt<EditRun> {
        match verdicts(cx, run) {
            Ok(verdicts) => {
                let blocked = verdicts.iter().any(|verdict| !verdict.holds);
                let delta = run.delta.clone().map(|delta| Delta { verdicts, ..delta });
                Attempt::Done(EditRun { delta, blocked, ..run.clone() })
            }
            Err(error) => Attempt::Failed(error),
        }
    }
}

/// Judges the expectations of the statements this edit brings: those after the part the
/// document has in common with what it was before. A change is measured against the
/// first body the tool was given, the one it works on.
fn verdicts(cx: &Context, run: &EditRun) -> Result<Vec<Verdict>, Error> {
    let model = run
        .model_after
        .as_ref()
        .ok_or_else(|| Error::unsupported("the `judge` stage needs a built model; put `build` before it"))?;
    let unchanged =
        run.before.statements().zip(run.after.statements()).take_while(|(was, is)| was == is).count();
    let mut versions: BTreeMap<&str, u32> = BTreeMap::new();
    let mut verdicts = Vec::new();
    for (index, statement) in run.after.statements().enumerate() {
        let version = versions.entry(statement.name.as_str()).or_insert(0);
        *version += 1;
        if index < unchanged || statement.expect.is_empty() {
            continue;
        }
        let node = NodeRef { name: statement.name.clone(), version: Some(*version) };
        let built = model.built(&node).ok_or_else(|| Error::not_found(format!("`{node}` was not built")))?;
        // Another node is seen as it is at this point of the document.
        let other = |name: &str| {
            let version = versions.get(name).copied()?;
            model
                .built(&NodeRef { name: name.to_string(), version: Some(version) })
                .map(|built| Arc::clone(&built.body))
        };
        for written in statement.expect.iter().flat_map(|line| line.split_whitespace()) {
            let named = |error: Error| Error::new(error.kind, format!("{node}: {}", error.message));
            let (holds, found) = Claim::parse(written)
                .and_then(|claim| claim.judge(cx, built.inputs.first(), &built.body, &other))
                .map_err(named)?;
            verdicts.push(Verdict { node: node.to_string(), claim: written.to_string(), holds, found });
        }
    }
    Ok(verdicts)
}
