//! Transports over the one interface of the kernel.
//!
//! A server is reached through [`endpoint::Endpoint`] alone: its `methods` and `call`.
//! Nothing here names a tool or a query: a plugin added to the kernel is callable from the
//! shell, over JSON-RPC and as an MCP tool without a change to this crate, and so is any
//! other server that is an endpoint, such as krlyvis.
//!
//! * [`endpoint`] — what a transport serves: the krly3d session, or another server;
//! * [`peer`] — another server reached over JSON-RPC, and [`peer::Joined`], a server whose
//!   peers answer the methods under their prefix;
//! * [`json`] — values and schemas of the kernel as JSON;
//! * [`rpc`] — the JSON-RPC 2.0 envelope;
//! * [`dialect`] — what is spoken inside that envelope, one file per dialect, and the
//!   loop that serves one over any reader and writer;
//! * [`shell`] — a call written as `key=value` words;
//! * [`args`] — the command line of the `krly3d` program.

pub mod args;
pub mod dialect;
pub mod endpoint;
pub mod json;
pub mod listen;
pub mod peer;
pub mod rpc;
pub mod shell;
