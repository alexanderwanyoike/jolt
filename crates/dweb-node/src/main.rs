mod cli;
mod commands;
mod config;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use cli::{CacheCommands, Cli, Commands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Start {
            port,
            bootstrap,
            no_bootstrap,
        } => commands::start::run(port, bootstrap, no_bootstrap).await?,
        Commands::Publish { file } => commands::publish::run(&file).await?,
        Commands::Fetch {
            content_id,
            output,
            dial,
            bootstrap,
        } => commands::fetch::run(&content_id, output, dial, bootstrap).await?,
        Commands::Cache { command } => match command {
            CacheCommands::Stats => commands::cache::stats()?,
            CacheCommands::List => commands::cache::list()?,
            CacheCommands::Pin { content_id } => commands::cache::pin(&content_id)?,
            CacheCommands::Unpin { content_id } => commands::cache::unpin(&content_id)?,
        },
    }

    Ok(())
}
