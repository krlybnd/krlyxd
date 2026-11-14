//! Stable text identifiers.

use std::borrow::Borrow;
use std::fmt;
use std::sync::Arc;

/// Identifier of a plugin, of a kind of curve or surface, or of a rule.
///
/// Ids are text rather than enum variants so that a crate outside the kernel can add a
/// new kind without the kernel listing it anywhere.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(Arc<str>);

impl Id {
    pub fn new(text: &str) -> Self {
        Self(Arc::from(text))
    }

    /// Id of a rule that relates two kinds, written `first|second`.
    pub fn pair(first: &Id, second: &Id) -> Self {
        Self::new(&format!("{first}|{second}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Id {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl Borrow<str> for Id {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Id;
    use std::collections::BTreeMap;

    #[test]
    fn pair_joins_the_two_kinds_with_a_bar() {
        let pair = Id::pair(&Id::new("plane"), &Id::new("sphere"));

        assert_eq!(pair.as_str(), "plane|sphere");
    }

    #[test]
    fn ids_order_as_their_text() {
        let mut ids = [Id::new("torus"), Id::new("cone"), Id::new("plane")];

        ids.sort();

        assert_eq!(ids.map(|id| id.to_string()), ["cone", "plane", "torus"]);
    }

    #[test]
    fn a_map_keyed_by_id_is_searchable_by_plain_text() {
        let map = BTreeMap::from([(Id::new("extrude"), 1)]);

        assert_eq!(map.get("extrude"), Some(&1));
    }
}
