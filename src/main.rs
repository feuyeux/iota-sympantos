mod acp;
mod acp_permission;
mod acp_session;
mod acp_wire;
mod agent;
mod approval;
mod cli;
mod config;
mod context;
mod context_mcp;
mod engine;
mod event_store;
mod fun_mcp;
mod mcp_client;
mod mcp_router;
mod memory;
mod native_materializer;
mod runtime_event;
mod session_ledger;
mod skill_registry_cache;
mod skill_runner;
mod skills;
mod tui;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
