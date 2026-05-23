//! MCP protocol layer.
//!
//! - [`client`] — spawns and communicates with stdio MCP sidecar processes
//! - [`server`] — stdio JSON-RPC MCP server (`iota mcp context`)
//! - [`router`] — intercepts `iota_*` tool calls in the ACP stream
//! - [`tool_dispatch`] — shared tool execution logic used by both server and router

pub mod client;
pub mod router;
pub mod server;
pub(crate) mod tool_dispatch;
