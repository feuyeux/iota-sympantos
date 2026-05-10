mod acp;
mod cli;
mod config;
mod context;
mod daemon;
mod engine;
mod mcp;
mod native;
mod runtime_event;
mod skill;
mod store;
mod telemetry;
mod tui;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
