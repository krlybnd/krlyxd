//! What an edit changed, in terms an agent can act on: named faces, not indices.

use crate::ports::measure::Measured;
use crate::predicate::Verdict;
use krly3d_core::Id;
use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub struct NodeDelta {
    pub node: String,
    pub version: u32,
    pub tool: Id,
    /// Face names that the new version has and the version before it had not.
    pub added: Vec<String>,
    /// Face names of the version before that are gone.
    pub removed: Vec<String>,
    /// Number of faces before and after.
    pub faces: (usize, usize),
    /// Volume of the new version, when it is a solid that can be measured.
    pub volume: Option<Measured>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Delta {
    pub nodes: Vec<NodeDelta>,
    /// Nodes that existed before the edit and no longer do.
    pub gone: Vec<String>,
    /// The expectations stated with the edit, each with whether it holds.
    pub verdicts: Vec<Verdict>,
    /// Whether the edit was kept. A dry run keeps nothing, and neither does an edit one of
    /// whose expectations does not hold.
    pub committed: bool,
}

impl Delta {
    /// Whether the edit was dropped because an expectation did not hold.
    pub fn refused(&self) -> bool {
        !self.committed && self.verdicts.iter().any(|verdict| !verdict.holds)
    }

    /// One short line per changed node, e.g.
    /// `ok part@1 box. +back +bottom +front +left +right +top. faces 0>6. volume =8000`.
    pub fn text(&self) -> String {
        let mut text = String::new();
        let lead = match (self.committed, self.refused()) {
            (true, _) => "ok",
            (false, true) => "refused",
            (false, false) => "dry",
        };
        for node in &self.nodes {
            let _ = write!(text, "{lead} {}@{} {}.", node.node, node.version, node.tool);
            if !(node.added.is_empty() && node.removed.is_empty()) {
                for name in &node.added {
                    let _ = write!(text, " +{name}");
                }
                for name in &node.removed {
                    let _ = write!(text, " -{name}");
                }
                text.push('.');
            }
            let _ = write!(text, " faces {}>{}.", node.faces.0, node.faces.1);
            if let Some(volume) = node.volume {
                let _ = write!(text, " volume {}{}", volume.exact.symbol(), volume.value);
            }
            text.push('\n');
        }
        for verdict in &self.verdicts {
            let outcome =
                if verdict.holds { "ok".to_string() } else { format!("FAILED, found {}", verdict.found) };
            let _ = writeln!(text, "expect {} {}: {outcome}", verdict.node, verdict.claim);
        }
        for node in &self.gone {
            let _ = writeln!(text, "{lead} {node} is gone.");
        }
        if self.nodes.is_empty() && self.gone.is_empty() {
            let _ = writeln!(text, "{lead}. nothing changed");
        }
        text
    }
}
