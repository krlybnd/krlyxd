//! The vis layer built: every version of every node, made once per session.
//!
//! The statements, their versions, the memo and the text format are the document mould
//! shared with krly3d (`krly3d_core::document`); only what a node is made into is vision's.

use crate::context::Context;
use crate::datum::Datum;
use crate::ports::make::{make, MakeRequest};
use krly3d_core::claim::{Amount, Phrase};
use krly3d_core::document::{self, Cache, Document, Key, Keyed, NodeRef, Versions};
use krly3d_core::{text, Error};

/// Name of the layer that holds the statements of vision.
pub const VISION: &str = "vis";

#[derive(Clone, Debug)]
pub struct Built {
    pub datum: Datum,
    pub key: Key,
}

impl Keyed for Built {
    fn key(&self) -> Key {
        self.key
    }
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    versions: Versions<Built>,
}

impl Model {
    pub fn get(&self, node: &NodeRef) -> Option<&Built> {
        self.versions.get(node)
    }

    pub fn nodes(&self) -> impl Iterator<Item = (&str, usize)> {
        self.versions.nodes()
    }
}

/// Builds the statements of `document` in order; a node already made with the same key in
/// `cache` is not made again, so a picture is read and decoded once per session.
pub fn build(cx: &Context, document: &Document, cache: &Cache<Datum>) -> Result<Model, Error> {
    let versions = document::build(
        document.statements(),
        cache,
        |statement, inputs: &[&Built]| {
            let request = MakeRequest {
                params: statement.op.params.clone(),
                inputs: inputs.iter().map(|built| built.datum.clone()).collect(),
            };
            make(cx, &statement.op.tool, &request).into_result()
        },
        |datum, key, _| Built { datum, key },
    )?;
    Ok(Model { versions })
}

/// A claim about a node with whether it holds and what was found instead.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub node: String,
    pub claim: String,
    pub holds: bool,
    pub found: String,
}

/// Judges a claim such as `polygons=1` or `width=156.25±0.5` against what the node made;
/// the claim grammar is krly3d's (`krly3d_core::claim`), the measures are the datum's.
pub fn judge(node: &str, datum: &Datum, claim: &str) -> Result<Verdict, Error> {
    let malformed =
        || Error::input(format!("`{claim}` is not a claim; write it like `polygons=1` or `width>=10`"));
    let phrase = Phrase::parse(claim).ok_or_else(malformed)?;
    let amount = Amount::parse(phrase.value, phrase.relation).ok_or_else(malformed)?;
    let actual = datum.measure(phrase.subject).ok_or_else(|| {
        Error::input(format!(
            "a {} has no measure `{}`; it has polygons, points, pixels, width, height and area",
            datum.kind(),
            phrase.subject
        ))
    })?;
    Ok(Verdict {
        node: node.to_string(),
        claim: claim.to_string(),
        holds: phrase.relation.holds(actual, amount.value, amount.tolerance),
        found: text::num(actual),
    })
}

#[cfg(test)]
mod tests {
    use super::judge;
    use crate::datum::Datum;
    use crate::image::Pixel;
    use crate::ports::outline::Shape;
    use std::sync::Arc;

    fn square() -> Datum {
        let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let polygon = corners.iter().map(|&(x, y)| Pixel::new(x, y)).collect();
        Datum::Outline {
            shape: Arc::new(Shape { polygons: vec![polygon], pixels: 100, threshold: 128.0, dark: true }),
        }
    }

    #[test]
    fn a_claim_about_an_outline_is_judged_on_its_measures() {
        assert!(judge("top", &square(), "area=100").unwrap().holds);
        assert!(!judge("top", &square(), "width>12").unwrap().holds);
    }

    #[test]
    fn a_claim_about_a_measure_the_datum_has_not_says_which_it_has() {
        let refused = judge("top", &square(), "volume=1").unwrap_err();

        assert!(refused.message.contains("polygons, points"), "{}", refused.message);
    }
}
