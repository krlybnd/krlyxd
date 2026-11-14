//! The krlyvis command line. The transports are krly3d's (`krly3d-cli`): krlyvis is one
//! more endpoint for them, so the shell form, plain JSON-RPC and MCP work the same way,
//! and krly3d can start krlyvis as a peer and answer its methods as `vis.…`.

pub mod args;
pub mod session;
