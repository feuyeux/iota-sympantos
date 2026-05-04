mod acp;
mod agent;
mod cli;
mod config;
mod engine;
mod tui;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
