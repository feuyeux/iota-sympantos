//! MCP tool interception and stdio client wrapper.
//!
//! - [`router`] — intercepts `iota_*` tool calls in the ACP stream
//! - [`client`] — spawns and communicates with stdio MCP sidecar processes

pub mod client;
pub mod router;
