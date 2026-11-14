//! The model: a document of operations, and the bodies built from it.
//!
//! The document mould — statements, versions, the memo of what each node was made into,
//! the text format — is shared with every kernel and lives in `krly3d_core::document`;
//! this module adds what a node of krly3d is made into, and what an edit changed.

pub mod build;
pub mod delta;

pub use build::{build, Built, Cache, Made, Model};
pub use delta::{Delta, NodeDelta};
pub use krly3d_core::document::{Document, Key, Layer, NodeRef, Op, Statement};

/// Name of the layer that holds krly3d's statements.
pub const GEOMETRY: &str = "geo";
