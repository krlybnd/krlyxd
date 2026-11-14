//! Port of the `index` family: a search index derived from a body.
//!
//! An object family. An index knows its elements — the faces of the body —, the sparse
//! keys of each (the matrix K) and which elements are related (the matrix R). A search
//! states what the element looked for is like (`q`) and what is next to it (`q_near`),
//! and every element is scored by one formula:
//!
//! ```text
//! s = K·q + α·R·K·q_near + α²·R²·K·q_near
//! ```
//!
//! Matching keys, plus two hops of passing the matches of the neighbours along the
//! relations. An index is derived data: it is never stored, and what it finds are
//! candidates to look at, not statements about the body.

pub mod contract;

use crate::context::Context;
use crate::plugin::Family;
use crate::topo::Body;
use krly3d_core::{Attempt, Error, Id};
use std::fmt::Debug;
use std::sync::Arc;

/// The family whose plugins derive an index from a body, keyed by the name of the index.
pub struct Indexing;

impl Family for Indexing {
    const NAME: &'static str = "index";
    type Request = IndexRequest;
    type Response = Arc<dyn Index>;
    type Context = crate::context::Context;
}

pub struct IndexRequest {
    pub body: Arc<Body>,
}

/// The name of the index of keys and relations that searches use.
pub const KEYS: &str = "keys";

/// What a match of a neighbour counts for, per hop along a relation.
pub const ALPHA: f64 = 0.5;

/// How many hops along the relations a match is passed.
pub const HOPS: usize = 2;

pub trait Index: Debug + Send + Sync + 'static {
    /// The names of the elements. An element is known by its position here.
    fn elements(&self) -> &[String];

    /// The keys of one element: its row of K.
    fn keys(&self, element: usize) -> &[String];

    /// The elements related to one element: its row of R. Relations hold both ways.
    fn related(&self, element: usize) -> &[usize];

    /// The score of every element for a search, by the formula of the family with
    /// [`ALPHA`] and [`HOPS`].
    fn score(&self, search: &Search) -> Vec<f64>;
}

/// What a search looks for: keys with weights, for the element itself and for what is
/// next to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Search {
    pub this: Vec<(String, f64)>,
    pub near: Vec<(String, f64)>,
}

impl Search {
    /// Reads a search written as words: `cylinder r:3 near:plane near:n:+z^2`.
    ///
    /// A word is a key. A bare word is a kind (`cylinder` is `kind:cylinder`), `near:`
    /// in front makes it a key of the neighbours, and `^2` behind gives it a weight.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let mut search = Self::default();
        for word in text.split_whitespace() {
            let (key, weight) = match word.rsplit_once('^') {
                Some((key, weight)) => (key, weight.parse::<f64>().ok().filter(|weight| weight.is_finite())),
                None => (word, Some(1.0)),
            };
            let weight = weight
                .ok_or_else(|| Error::input(format!("`{word}`: what follows `^` is a weight, a number")))?;
            let (key, terms) = match key.strip_prefix("near:") {
                Some(key) => (key, &mut search.near),
                None => (key, &mut search.this),
            };
            if key.is_empty() {
                return Err(Error::input(format!("`{word}` names no key")));
            }
            terms.push((if key.contains(':') { key.to_string() } else { format!("kind:{key}") }, weight));
        }
        if search.this.is_empty() && search.near.is_empty() {
            return Err(Error::input(
                "a search needs at least one key, such as `cylinder`, `n:+z` or `near:plane`",
            ));
        }
        Ok(search)
    }
}

/// The index of keys and relations of a body.
pub fn index(cx: &Context, body: &Arc<Body>) -> Attempt<Arc<dyn Index>> {
    cx.call::<Indexing>(&Id::new(KEYS), &IndexRequest { body: Arc::clone(body) })
}

/// The `count` best elements for a search with their scores: the highest score first,
/// equal scores in the order of the elements, and nothing that scores zero or less.
pub fn best(index: &dyn Index, search: &Search, count: usize) -> Vec<(usize, f64)> {
    let mut ranked: Vec<(usize, f64)> =
        index.score(search).into_iter().enumerate().filter(|(_, score)| *score > 0.0).collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked.truncate(count);
    ranked
}

#[cfg(test)]
mod tests {
    use super::Search;
    use krly3d_core::ErrorKind;

    fn terms(terms: &[(&str, f64)]) -> Vec<(String, f64)> {
        terms.iter().map(|(key, weight)| (key.to_string(), *weight)).collect()
    }

    #[test]
    fn a_bare_word_is_a_kind_and_a_word_with_a_colon_is_a_key() {
        let search = Search::parse("cylinder r:3").unwrap();

        assert_eq!(search.this, terms(&[("kind:cylinder", 1.0), ("r:3", 1.0)]));
        assert!(search.near.is_empty());
    }

    #[test]
    fn near_in_front_of_a_word_makes_it_a_key_of_the_neighbours() {
        let search = Search::parse("cylinder near:plane near:n:+z").unwrap();

        assert_eq!(search.this, terms(&[("kind:cylinder", 1.0)]));
        assert_eq!(search.near, terms(&[("kind:plane", 1.0), ("n:+z", 1.0)]));
    }

    #[test]
    fn a_number_after_a_caret_is_the_weight_of_the_key() {
        let search = Search::parse("plane^2 near:r:3^0.5").unwrap();

        assert_eq!(search.this, terms(&[("kind:plane", 2.0)]));
        assert_eq!(search.near, terms(&[("r:3", 0.5)]));
    }

    #[test]
    fn a_weight_that_is_no_number_is_refused_with_the_word_it_stands_in() {
        let error = Search::parse("cylinder plane^heavy").unwrap_err();

        assert_eq!(error.kind, ErrorKind::Input);
        assert_eq!(error.message, "`plane^heavy`: what follows `^` is a weight, a number");
    }

    #[test]
    fn a_search_without_any_key_is_refused() {
        let (empty, hollow) = (Search::parse("  ").unwrap_err(), Search::parse("near:").unwrap_err());

        assert_eq!((empty.kind, hollow.kind), (ErrorKind::Input, ErrorKind::Input));
        assert_eq!(hollow.message, "`near:` names no key");
    }
}
