use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "dweb", about = "Decentralized web platform", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the dweb daemon
    Start {
        /// HTTP API port (default: 9862)
        #[arg(long, default_value = "9862")]
        api_port: u16,

        /// HTTP API bind address (default: 127.0.0.1, use 0.0.0.0 for all interfaces)
        #[arg(long, default_value = "127.0.0.1")]
        api_bind: String,

        /// Bootstrap peer multiaddr (repeatable, e.g., /ip4/1.2.3.4/udp/4001/quic-v1/p2p/12D3KooW...)
        #[arg(short, long)]
        bootstrap: Vec<String>,

        /// Disable DHT bootstrapping (LAN-only mode)
        #[arg(long)]
        no_bootstrap: bool,

        /// Fixed P2P port (default: 0 = random). UDP for iroh, TCP for --transport tcp.
        #[arg(long, default_value = "0")]
        p2p_port: u16,

        /// P2P transport to use. iroh is the real-network default; tcp is for local demos/tests.
        #[arg(long, value_enum, default_value_t = TransportMode::Iroh)]
        transport: TransportMode,
    },

    /// Stop the running daemon
    Stop,

    /// Show daemon status
    Status,

    /// Publish a file to the network
    Publish {
        /// Path to the file to publish
        file: PathBuf,
    },

    /// Fetch content by its ContentId
    Fetch {
        /// The ContentId to fetch
        content_id: String,

        /// Output file path (defaults to content ID in current directory)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Resolve a .jolt address to its current content target
    Resolve {
        /// The .jolt address to resolve
        address: String,
    },

    /// Manage the content cache
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
    },

    /// Manage persistent bootstrap relays
    Bootstrap {
        #[command(subcommand)]
        command: BootstrapCommands,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum TransportMode {
    Iroh,
    Tcp,
}

#[derive(Subcommand)]
pub enum CacheCommands {
    /// Show cache statistics
    Stats,

    /// List cached content
    List,

    /// Pin content to prevent eviction
    Pin {
        /// The ContentId to pin
        content_id: String,
    },

    /// Unpin content to allow eviction
    Unpin {
        /// The ContentId to unpin
        content_id: String,
    },
}

#[derive(Subcommand)]
pub enum BootstrapCommands {
    /// List configured, built-in, and effective bootstrap relays
    List,

    /// Add a configured bootstrap relay multiaddr
    Add {
        /// Bootstrap relay multiaddr with /p2p/<peer_id>
        multiaddr: String,
    },

    /// Remove a configured bootstrap relay multiaddr
    Remove {
        /// Bootstrap relay multiaddr to remove
        multiaddr: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_start_command() {
        let cli = Cli::parse_from(["dweb", "start"]);
        assert!(matches!(cli.command, Commands::Start { .. }));
    }

    #[test]
    fn parse_start_with_api_port() {
        let cli = Cli::parse_from(["dweb", "start", "--api-port", "8080"]);
        match cli.command {
            Commands::Start { api_port, .. } => assert_eq!(api_port, 8080),
            _ => panic!("expected Start command"),
        }
    }

    #[test]
    fn parse_start_with_tcp_transport() {
        let cli = Cli::parse_from(["dweb", "start", "--transport", "tcp"]);
        match cli.command {
            Commands::Start { transport, .. } => assert_eq!(transport, TransportMode::Tcp),
            _ => panic!("expected Start command"),
        }
    }

    #[test]
    fn parse_stop_command() {
        let cli = Cli::parse_from(["dweb", "stop"]);
        assert!(matches!(cli.command, Commands::Stop));
    }

    #[test]
    fn parse_status_command() {
        let cli = Cli::parse_from(["dweb", "status"]);
        assert!(matches!(cli.command, Commands::Status));
    }

    #[test]
    fn parse_publish_command() {
        let cli = Cli::parse_from(["dweb", "publish", "/tmp/test.txt"]);
        match cli.command {
            Commands::Publish { file } => {
                assert_eq!(file, PathBuf::from("/tmp/test.txt"));
            }
            _ => panic!("expected Publish command"),
        }
    }

    #[test]
    fn parse_fetch_command() {
        let cli = Cli::parse_from(["dweb", "fetch", "bafk_test_id"]);
        match cli.command {
            Commands::Fetch {
                content_id, output, ..
            } => {
                assert_eq!(content_id, "bafk_test_id");
                assert!(output.is_none());
            }
            _ => panic!("expected Fetch command"),
        }
    }

    #[test]
    fn parse_fetch_command_with_output() {
        let cli = Cli::parse_from(["dweb", "fetch", "bafk_test_id", "-o", "/tmp/out.bin"]);
        match cli.command {
            Commands::Fetch {
                content_id, output, ..
            } => {
                assert_eq!(content_id, "bafk_test_id");
                assert_eq!(output, Some(PathBuf::from("/tmp/out.bin")));
            }
            _ => panic!("expected Fetch command"),
        }
    }

    #[test]
    fn parse_resolve_command() {
        let cli = Cli::parse_from(["dweb", "resolve", "alice.jolt/profile"]);
        match cli.command {
            Commands::Resolve { address } => {
                assert_eq!(address, "alice.jolt/profile");
            }
            _ => panic!("expected Resolve command"),
        }
    }

    #[test]
    fn parse_cache_stats_command() {
        let cli = Cli::parse_from(["dweb", "cache", "stats"]);
        assert!(matches!(
            cli.command,
            Commands::Cache {
                command: CacheCommands::Stats
            }
        ));
    }

    #[test]
    fn parse_cache_list_command() {
        let cli = Cli::parse_from(["dweb", "cache", "list"]);
        assert!(matches!(
            cli.command,
            Commands::Cache {
                command: CacheCommands::List
            }
        ));
    }

    #[test]
    fn parse_cache_pin_command() {
        let cli = Cli::parse_from(["dweb", "cache", "pin", "bafk_test"]);
        match cli.command {
            Commands::Cache {
                command: CacheCommands::Pin { content_id },
            } => {
                assert_eq!(content_id, "bafk_test");
            }
            _ => panic!("expected Cache Pin command"),
        }
    }

    #[test]
    fn parse_cache_unpin_command() {
        let cli = Cli::parse_from(["dweb", "cache", "unpin", "bafk_test"]);
        match cli.command {
            Commands::Cache {
                command: CacheCommands::Unpin { content_id },
            } => {
                assert_eq!(content_id, "bafk_test");
            }
            _ => panic!("expected Cache Unpin command"),
        }
    }

    #[test]
    fn parse_bootstrap_list_command() {
        let cli = Cli::parse_from(["dweb", "bootstrap", "list"]);
        assert!(matches!(
            cli.command,
            Commands::Bootstrap {
                command: BootstrapCommands::List
            }
        ));
    }

    #[test]
    fn parse_bootstrap_add_command() {
        let cli = Cli::parse_from([
            "dweb",
            "bootstrap",
            "add",
            "/ip4/127.0.0.1/tcp/4001/p2p/12D3Relay",
        ]);
        match cli.command {
            Commands::Bootstrap {
                command: BootstrapCommands::Add { multiaddr },
            } => assert_eq!(multiaddr, "/ip4/127.0.0.1/tcp/4001/p2p/12D3Relay"),
            _ => panic!("expected Bootstrap Add command"),
        }
    }

    #[test]
    fn parse_bootstrap_remove_command() {
        let cli = Cli::parse_from([
            "dweb",
            "bootstrap",
            "remove",
            "/ip4/127.0.0.1/tcp/4001/p2p/12D3Relay",
        ]);
        match cli.command {
            Commands::Bootstrap {
                command: BootstrapCommands::Remove { multiaddr },
            } => assert_eq!(multiaddr, "/ip4/127.0.0.1/tcp/4001/p2p/12D3Relay"),
            _ => panic!("expected Bootstrap Remove command"),
        }
    }
}
