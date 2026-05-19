mod backend;
mod client;
mod message;
mod parser;
pub mod permission;
pub mod session;
mod stream_reader;
mod types;
mod util;
pub mod wire;

pub use backend::{ALL_BACKENDS, AcpBackend};
#[allow(unused_imports)]
pub use parser::print_acp_help;
pub use parser::{AcpRunOptions, parse_acp_args};
pub use types::{
    AcpClient, AcpClientStartOptions, AcpPromptOutput, AcpPromptTiming, AcpStartupTiming,
};

pub use message::extract_text;
#[cfg(test)]
use message::{
    acp_tool_call_parts, extract_final_text, is_terminal_result, permission_request_id,
    text_from_session_update,
};
#[cfg(test)]
use wire::AcpWireMessage;

pub const DEFAULT_TIMEOUT_MS: u64 = 60_000;

#[cfg(test)]
#[path = "acp_tests.rs"]
mod acp_tests;
