//! What nodes were made into, by a key of what they were made from.

use super::op::Op;
use crate::schema::Value;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, PoisonError};

/// Identity of a built node: a hash of its tool, its parameters and the identities of
/// its inputs. Two nodes with the same key are the same product.
pub type Key = u128;

/// Products by key. Shared by every document built in a session, so a node is computed
/// once however many documents contain it.
pub struct Cache<T> {
    made: Mutex<BTreeMap<Key, T>>,
}

impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self { made: Mutex::new(BTreeMap::new()) }
    }
}

impl<T: Clone> Cache<T> {
    pub fn get(&self, key: Key) -> Option<T> {
        self.made.lock().unwrap_or_else(PoisonError::into_inner).get(&key).cloned()
    }

    pub fn put(&self, key: Key, made: T) {
        self.made.lock().unwrap_or_else(PoisonError::into_inner).insert(key, made);
    }

    pub fn len(&self) -> usize {
        self.made.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The key of a node made by `op` from inputs with the keys `inputs`.
pub fn key_of(op: &Op, inputs: &[Key]) -> Key {
    // Two differently seeded 64-bit hashes of the same content make one 128-bit key, so
    // that two different nodes sharing a key is out of the question in practice.
    let half = |seed: u8| {
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        op.tool.hash(&mut hasher);
        for (name, value) in op.params.iter() {
            name.hash(&mut hasher);
            hash_value(value, &mut hasher);
        }
        inputs.hash(&mut hasher);
        hasher.finish()
    };
    (u128::from(half(0)) << 64) | u128::from(half(1))
}

fn hash_value(value: &Value, hasher: &mut DefaultHasher) {
    match value {
        Value::Bool(flag) => (3_u8, flag).hash(hasher),
        Value::Number(number) => (0_u8, number.to_bits()).hash(hasher),
        Value::Text(text) => (1_u8, text).hash(hasher),
        Value::List(items) => {
            (2_u8, items.len()).hash(hasher);
            items.iter().for_each(|item| hash_value(item, hasher));
        }
        Value::Map(entries) => {
            (4_u8, entries.len()).hash(hasher);
            for (name, value) in entries {
                name.hash(hasher);
                hash_value(value, hasher);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{key_of, Cache};
    use crate::document::Statement;
    use crate::schema::Params;

    #[test]
    fn the_same_op_on_the_same_inputs_has_the_same_key_and_any_change_another() {
        let op = |r: f64| Statement::new("a", "sphere", Params::new().with("r", r), &[]).unwrap().op;

        assert_eq!(key_of(&op(2.0), &[7]), key_of(&op(2.0), &[7]));
        assert_ne!(key_of(&op(2.0), &[7]), key_of(&op(3.0), &[7]));
        assert_ne!(key_of(&op(2.0), &[7]), key_of(&op(2.0), &[8]));
    }

    #[test]
    fn a_cache_gives_back_what_was_put_under_a_key() {
        let cache = Cache::default();

        cache.put(5, "made");

        assert_eq!((cache.get(5), cache.get(6), cache.len()), (Some("made"), None, 1));
    }
}
