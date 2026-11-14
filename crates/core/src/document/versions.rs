//! Every version of every node, and the one way a document is built into them.

use super::cache::{key_of, Cache, Key};
use super::op::{NodeRef, Statement};
use crate::Error;
use std::collections::BTreeMap;

/// A built node that knows its key, so that the nodes reading it can be keyed in turn.
pub trait Keyed {
    fn key(&self) -> Key;
}

/// Every version of every node of a document, in the order they were defined.
#[derive(Clone, Debug)]
pub struct Versions<B> {
    nodes: BTreeMap<String, Vec<B>>,
}

impl<B> Default for Versions<B> {
    fn default() -> Self {
        Self { nodes: BTreeMap::new() }
    }
}

impl<B> Versions<B> {
    /// The node at the version named, or at its latest.
    pub fn get(&self, node: &NodeRef) -> Option<&B> {
        let versions = self.nodes.get(&node.name)?;
        match node.version {
            Some(version) => versions.get(version.checked_sub(1)? as usize),
            None => versions.last(),
        }
    }

    /// Node names with the number of versions each has, in name order.
    pub fn nodes(&self) -> impl Iterator<Item = (&str, usize)> {
        self.nodes.iter().map(|(name, versions)| (name.as_str(), versions.len()))
    }

    fn push(&mut self, name: &str, built: B) {
        self.nodes.entry(name.to_string()).or_default().push(built);
    }
}

/// Builds the statements in order. A statement reads the latest version defined before it,
/// so a name may be redefined in terms of itself. `make` runs a statement whose key the
/// cache does not know, from the nodes it reads; `wrap` turns what was made, its key and
/// those nodes into the built node.
pub fn build<'a, B, M, Make, Wrap>(
    statements: impl Iterator<Item = &'a Statement>,
    cache: &Cache<M>,
    mut make: Make,
    mut wrap: Wrap,
) -> Result<Versions<B>, Error>
where
    B: Keyed,
    M: Clone,
    Make: FnMut(&Statement, &[&B]) -> Result<M, Error>,
    Wrap: FnMut(M, Key, &[&B]) -> B,
{
    let mut versions: Versions<B> = Versions::default();
    for statement in statements {
        let named = |error: Error| Error::new(error.kind, format!("{}: {}", statement.name, error.message));
        let mut inputs = Vec::with_capacity(statement.op.inputs.len());
        for reference in &statement.op.inputs {
            let built = versions.get(reference).ok_or_else(|| {
                named(Error::not_found(format!("`{reference}` is not defined before it is used")))
            })?;
            inputs.push(built);
        }
        let keys: Vec<Key> = inputs.iter().map(|built| built.key()).collect();
        let key = key_of(&statement.op, &keys);
        let made = match cache.get(key) {
            Some(made) => made,
            None => {
                let made = make(statement, &inputs).map_err(named)?;
                cache.put(key, made.clone());
                made
            }
        };
        let built = wrap(made, key, &inputs);
        versions.push(&statement.name, built);
    }
    Ok(versions)
}

#[cfg(test)]
mod tests {
    use super::{build, Keyed, Versions};
    use crate::document::{Cache, Key, NodeRef, Statement};
    use crate::schema::Params;

    #[derive(Clone, Debug, PartialEq)]
    struct Count {
        value: usize,
        key: Key,
    }

    impl Keyed for Count {
        fn key(&self) -> Key {
            self.key
        }
    }

    fn statements() -> Vec<Statement> {
        vec![
            Statement::new("a", "one", Params::new(), &[]).unwrap(),
            Statement::new("b", "sum", Params::new(), &["a", "a"]).unwrap(),
            Statement::new("a", "sum", Params::new(), &["a", "b"]).unwrap(),
        ]
    }

    fn built(cache: &Cache<usize>, runs: &mut usize) -> Versions<Count> {
        let make = |statement: &Statement, inputs: &[&Count]| {
            *runs += 1;
            Ok(if statement.op.tool.as_str() == "one" {
                1
            } else {
                inputs.iter().map(|count| count.value).sum()
            })
        };
        build(statements().iter(), cache, make, |value, key, _| Count { value, key }).unwrap()
    }

    #[test]
    fn a_statement_reads_the_versions_defined_before_it() {
        let cache = Cache::default();

        let versions = built(&cache, &mut 0);

        let value = |text: &str| versions.get(&NodeRef::parse(text).unwrap()).map(|count| count.value);
        assert_eq!((value("a@1"), value("b"), value("a")), (Some(1), Some(2), Some(3)));
    }

    #[test]
    fn building_again_with_the_same_cache_runs_nothing() {
        let (cache, mut runs) = (Cache::default(), 0);
        built(&cache, &mut runs);

        let mut again = 0;
        built(&cache, &mut again);

        assert_eq!((runs, again), (3, 0));
    }

    #[test]
    fn a_node_read_before_it_is_defined_is_named_in_the_error() {
        let statements = [Statement::new("b", "sum", Params::new(), &["a"]).unwrap()];

        let refused = build(
            statements.iter(),
            &Cache::<usize>::default(),
            |_, _| Ok(0),
            |value, key, _| Count { value, key },
        );

        assert!(refused.unwrap_err().message.contains("`a` is not defined"));
    }
}
