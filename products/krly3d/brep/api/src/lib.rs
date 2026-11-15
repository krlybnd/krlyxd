//! Additive Rust metadata contract for B-rep snapshots, independent of the engine.
//!
//! Live, unforgeable snapshot handles belong to the engine. The separate [`planar`]
//! contract describes one bounded extrusion and its native document artifact.

pub mod planar;

pub use krlyxd::{Diagnostic, Id};

/// Version of the Rust snapshot metadata contract; not a serialized shape format.
pub const SNAPSHOT_FORMAT: &str = "krly3d.brep.snapshot/1";

/// Metadata describing one resolved node in one B-rep engine session.
///
/// Metadata can be copied freely; it cannot create or rebind a live snapshot handle.
/// Session and build identities are process-local provenance, not durable hashes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotIdentity {
    /// Session, resolved `node@version` and build key, joined with colons.
    pub source: Id,
    /// Unique identity of the engine that resolved this snapshot.
    pub session: Id,
    /// Node name normalized to an explicit `node@version` exactly once.
    pub node: String,
    /// Native build key written as exactly 32 lowercase hexadecimal digits.
    pub build_key: String,
}
