//! The document: layers of statements, weakest first.
//!
//! A layer either holds statements, which a kernel builds, or lines kept as written.
//! Which layers hold statements is the kernel's choice (`geo` for krly3d, `vis` for
//! krlyvis); the document itself knows no layer by name.

use super::op::Statement;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layer {
    pub name: String,
    /// The operations of a layer of statements.
    pub statements: Vec<Statement>,
    /// Lines of a layer whose content is kept as written.
    pub lines: Vec<String>,
}

/// An immutable value: editing makes a new document and leaves the old one as it was.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub layers: Vec<Layer>,
}

impl Document {
    pub fn statements(&self) -> impl Iterator<Item = &Statement> {
        self.layers.iter().flat_map(|layer| &layer.statements)
    }

    /// This document with one more statement at the end of the layer `name`, which is
    /// made, first of all layers, when there is none.
    pub fn with_in(&self, name: &str, statement: Statement) -> Self {
        let mut next = self.clone();
        match next.layers.iter_mut().find(|layer| layer.name == name) {
            Some(layer) => layer.statements.push(statement),
            None => {
                let layer = Layer { name: name.to_string(), statements: vec![statement], lines: Vec::new() };
                next.layers.insert(0, layer);
            }
        }
        next
    }
}
