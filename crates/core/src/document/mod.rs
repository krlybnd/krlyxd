//! The document mould every kernel shares: statements in layers, nodes with versions, a
//! memo of what each node was made into, and the native text format.
//!
//! The document is the stored truth; what its statements make is a cache. Building is a
//! pure function of the document, remembered per node by a key of what the node was made
//! from, so undoing, trying out and replaying an edit are all the same thing. What a
//! statement makes — a body, a picture, an outline — is the kernel's business.

pub mod cache;
pub mod layers;
pub mod op;
pub mod text;
pub mod versions;

pub use cache::{key_of, Cache, Key};
pub use layers::{Document, Layer};
pub use op::{checked_name, NodeRef, Op, Statement};
pub use versions::{build, Keyed, Versions};
