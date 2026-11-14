//! Contracts and orchestration of krlyvis, the image reader of krly3d.
//!
//! No capability is implemented here. This crate defines what a plugin is, the port of
//! each family and how an analysis travels through the stages; the implementations are
//! registered from outside (see the `krlyvis-std` crate). It is cast in the same mould
//! as the krly3d kernel and shares its vocabulary (`krly3d-core`).

pub mod builder;
pub mod context;
pub mod datum;
pub mod frame;
pub mod image;
pub mod model;
pub mod plugin;
pub mod ports;
pub mod report;
pub mod scene;
pub mod vision;

pub use builder::VisionBuilder;
pub use context::Context;
pub use datum::Datum;
pub use frame::{Frame, View};
pub use image::{Gray, Pixel};
pub use plugin::{Family, Plugin, Registries, Registry};
pub use scene::{Ask, Axis, Known, Scene};
pub use vision::Vision;
