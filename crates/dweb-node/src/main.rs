mod cli;
mod client;
mod commands;
mod config;
mod daemon;

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
            api_port,
            api_bind,
            bootstrap,
            no_bootstrap,
        } => commands::start::run(api_port, &api_bind, bootstrap, no_bootstrap).await?,
        Commands::Stop => commands::stop::run().await?,
        Commands::Status => commands::status::run().await?,
        Commands::Publish { file } => commands::publish::run(&file).await?,
        Commands::Fetch {
            content_id,
            output,
        } => commands::fetch::run(&content_id, output).await?,
        Commands::Cache { command } => match command {
            CacheCommands::Stats => commands::cache::stats().await?,
            CacheCommands::List => commands::cache::list().await?,
            CacheCommands::Pin { content_id } => commands::cache::pin(&content_id).await?,
            CacheCommands::Unpin { content_id } => commands::cache::unpin(&content_id).await?,
        },
    }

    Ok(())
}
