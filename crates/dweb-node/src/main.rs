mod cli;
mod commands;
mod config;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Start => commands::start::run().await?,
        Commands::Publish { file } => commands::publish::run(&file).await?,
        Commands::Fetch { content_id, output } => {
            commands::fetch::run(&content_id, output).await?
        }
    }

    Ok(())
}
