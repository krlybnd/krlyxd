//! One operation: a tool, its parameters and the nodes it reads.

use crate::schema::Params;
use crate::{Error, Id};
use std::fmt;

/// A node by name, at its latest version or at a stated one (`part`, `part@2`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeRef {
    pub name: String,
    pub version: Option<u32>,
}

impl NodeRef {
    pub fn parse(text: &str) -> Result<Self, Error> {
        let (name, version) = match text.split_once('@') {
            Some((name, version)) => {
                let version = version.parse::<u32>().ok().filter(|version| *version >= 1);
                (
                    name,
                    Some(version.ok_or_else(|| {
                        Error::input(format!("`{text}`: a version is a whole number from 1"))
                    })?),
                )
            }
            None => (text, None),
        };
        checked_name(name)?;
        Ok(Self { name: name.to_string(), version })
    }
}

impl fmt::Display for NodeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.version {
            Some(version) => write!(f, "{}@{version}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub tool: Id,
    pub params: Params,
    pub inputs: Vec<NodeRef>,
}

/// One line of the geometry layer: `name = tool inputs… parameters…`.
///
/// Defining a name again makes a new version of the node; the tool may read the version
/// before it, which is how a body is changed (`part = fillet part …`).
#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub name: String,
    pub op: Op,
    /// Expectations stated with the statement, as written.
    pub expect: Vec<String>,
}

impl Statement {
    pub fn new(name: &str, tool: &str, params: Params, inputs: &[&str]) -> Result<Self, Error> {
        checked_name(name)?;
        let inputs = inputs.iter().map(|input| NodeRef::parse(input)).collect::<Result<_, _>>()?;
        Ok(Self {
            name: name.to_string(),
            op: Op { tool: Id::new(tool), params, inputs },
            expect: Vec::new(),
        })
    }
}

/// Node names are plain words, so that `@`, `#`, `|`, `:` and `=` keep their meaning in
/// references to versions, faces, edges and parameters.
pub fn checked_name(name: &str) -> Result<(), Error> {
    let plain = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
    if !name.is_empty()
        && name.chars().all(plain)
        && !name.starts_with(|c: char| c.is_ascii_digit() || c == '-')
    {
        Ok(())
    } else {
        Err(Error::input(format!(
            "`{name}` is not a node name: use letters, digits, `_`, `-` and `.`, not starting with a digit"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{NodeRef, Statement};
    use crate::schema::Params;

    #[test]
    fn a_reference_reads_back_as_it_was_written() {
        for text in ["part", "part@2", "shaft.profile"] {
            assert_eq!(NodeRef::parse(text).unwrap().to_string(), text);
        }
    }

    #[test]
    fn a_reference_with_a_face_or_a_bad_version_is_rejected() {
        for text in ["part#top", "part@0", "part@two", "", "2part"] {
            assert!(NodeRef::parse(text).is_err(), "`{text}` was accepted");
        }
    }

    #[test]
    fn a_statement_rejects_a_name_that_would_read_as_something_else() {
        assert!(Statement::new("a=b", "box", Params::new(), &[]).is_err());
        assert!(Statement::new("plate", "box", Params::new(), &[]).is_ok());
    }
}
