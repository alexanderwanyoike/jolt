mod cli;
mod client;
mod commands;
mod config;
mod daemon;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use cli::{BootstrapCommands, CacheCommands, Cli, Commands, HomeRelayCommands, RelayCommands};

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
            no_mdns,
            p2p_port,
            transport,
        } => {
            commands::start::run(
                api_port,
                &api_bind,
                bootstrap,
                no_bootstrap,
                no_mdns,
                p2p_port,
                transport,
            )
            .await?
        }
        Commands::Stop => commands::stop::run().await?,
        Commands::Status => commands::status::run().await?,
        Commands::Publish {
            file,
            path,
            pin_home_relay,
        } => commands::publish::run(&file, path.as_deref(), pin_home_relay).await?,
        Commands::Fetch { target, output } => commands::fetch::run(&target, output).await?,
        Commands::Resolve { address } => commands::resolve::run(&address).await?,
        Commands::Cache { command } => match command {
            CacheCommands::Stats => commands::cache::stats().await?,
            CacheCommands::List => commands::cache::list().await?,
            CacheCommands::Pin { content_id } => commands::cache::pin(&content_id).await?,
            CacheCommands::Unpin { content_id } => commands::cache::unpin(&content_id).await?,
        },
        Commands::Bootstrap { command } => match command {
            BootstrapCommands::List => commands::bootstrap::list().await?,
            BootstrapCommands::Add { multiaddr } => commands::bootstrap::add(&multiaddr).await?,
            BootstrapCommands::Remove { multiaddr } => {
                commands::bootstrap::remove(&multiaddr).await?
            }
        },
        Commands::HomeRelay { command } => match command {
            HomeRelayCommands::Show => commands::home_relay::show().await?,
            HomeRelayCommands::Set {
                multiaddr,
                capability,
                api_url,
            } => commands::home_relay::set(&multiaddr, capability, api_url.as_deref()).await?,
            HomeRelayCommands::Pin { content_id } => commands::home_relay::pin(&content_id).await?,
            HomeRelayCommands::Clear => commands::home_relay::clear().await?,
        },
        Commands::Relay { command } => match command {
            RelayCommands::Status { json } => commands::relay::status(json).await?,
        },
    }

    Ok(())
}
