//! Contracts and orchestration of the krly3d kernel.
//!
//! No capability is implemented here. This crate defines what a plugin is, the port of
//! each family, and how a request travels; the implementations are registered from
//! outside (see the `krly3d-std` crate).

pub mod api;
pub mod builder;
pub mod context;
pub mod kernel;
pub mod mesh;
pub mod model;
pub mod plugin;
pub mod ports;
pub mod predicate;
pub mod schema;
pub mod topo;

pub use api::{describe, Method, MethodKind, Reply, Session};
pub use builder::KernelBuilder;
pub use context::Context;
pub use kernel::{Checkpoint, Kernel};
pub use plugin::{Family, Plugin, Registries, Registry};
