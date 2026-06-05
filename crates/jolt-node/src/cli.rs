use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "jolt", about = "Decentralized web platform", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the jolt daemon
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

        /// Disable mDNS LAN peer discovery. Useful for deterministic relay-path demos/tests.
        #[arg(long)]
        no_mdns: bool,

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

        /// Optional .jolt namespace path to bind to the published CID
        #[arg(long)]
        path: Option<String>,

        /// Pin the published content to the configured home relay
        #[arg(long)]
        pin_home_relay: bool,
    },

    /// Fetch content by ContentId or .jolt address
    Fetch {
        /// The ContentId or .jolt address to fetch
        target: String,

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

    /// Manage the configured home relay
    HomeRelay {
        #[command(subcommand)]
        command: HomeRelayCommands,
    },

    /// Inspect and diagnose this node as a relay operator
    Relay {
        #[command(subcommand)]
        command: RelayCommands,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum HomeRelayCapabilityArg {
    Unknown,
    DiscoveryOnly,
    Pinning,
}

#[derive(Subcommand)]
pub enum HomeRelayCommands {
    /// Show the configured home relay
    Show,

    /// Set the configured home relay multiaddr
    Set {
        /// Home relay multiaddr with /p2p/<peer_id>
        multiaddr: String,

        /// Known relay capability
        #[arg(long, value_enum, default_value_t = HomeRelayCapabilityArg::Pinning)]
        capability: HomeRelayCapabilityArg,

        /// HTTP API URL for relay pin requests
        #[arg(long)]
        api_url: Option<String>,
    },

    /// Pin locally published content to the configured home relay
    Pin {
        /// The locally published ContentId to pin
        content_id: String,
    },

    /// Remove the configured home relay
    Clear,
}

#[derive(Subcommand)]
pub enum RelayCommands {
    /// Show relay operator status
    Status {
        /// Emit the admin relay status JSON payload
        #[arg(long)]
        json: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_start_command() {
        let cli = Cli::parse_from(["jolt", "start"]);
        assert!(matches!(cli.command, Commands::Start { .. }));
    }

    #[test]
    fn parse_home_relay_set_command() {
        let cli = Cli::parse_from([
            "jolt",
            "home-relay",
            "set",
            "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWExample",
            "--capability",
            "discovery-only",
            "--api-url",
            "http://127.0.0.1:9863",
        ]);

        match cli.command {
            Commands::HomeRelay {
                command:
                    HomeRelayCommands::Set {
                        multiaddr,
                        capability,
                        api_url,
                    },
            } => {
                assert_eq!(multiaddr, "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWExample");
                assert_eq!(capability, HomeRelayCapabilityArg::DiscoveryOnly);
                assert_eq!(api_url.as_deref(), Some("http://127.0.0.1:9863"));
            }
            _ => panic!("expected HomeRelay Set command"),
        }
    }

    #[test]
    fn parse_publish_with_home_relay_pin() {
        let cli = Cli::parse_from(["jolt", "publish", "post.txt", "--pin-home-relay"]);

        match cli.command {
            Commands::Publish {
                file,
                path,
                pin_home_relay,
            } => {
                assert_eq!(file, PathBuf::from("post.txt"));
                assert_eq!(path, None);
                assert!(pin_home_relay);
            }
            _ => panic!("expected Publish command"),
        }
    }

    #[test]
    fn parse_home_relay_pin_command() {
        let cli = Cli::parse_from(["jolt", "home-relay", "pin", "cid123"]);

        match cli.command {
            Commands::HomeRelay {
                command: HomeRelayCommands::Pin { content_id },
            } => assert_eq!(content_id, "cid123"),
            _ => panic!("expected HomeRelay Pin command"),
        }
    }

    #[test]
    fn parse_start_with_api_port() {
        let cli = Cli::parse_from(["jolt", "start", "--api-port", "8080"]);
        match cli.command {
            Commands::Start { api_port, .. } => assert_eq!(api_port, 8080),
            _ => panic!("expected Start command"),
        }
    }

    #[test]
    fn parse_start_with_tcp_transport() {
        let cli = Cli::parse_from(["jolt", "start", "--transport", "tcp"]);
        match cli.command {
            Commands::Start { transport, .. } => assert_eq!(transport, TransportMode::Tcp),
            _ => panic!("expected Start command"),
        }
    }

    #[test]
    fn parse_start_with_no_mdns() {
        let cli = Cli::parse_from(["jolt", "start", "--no-mdns"]);
        match cli.command {
            Commands::Start { no_mdns, .. } => assert!(no_mdns),
            _ => panic!("expected Start command"),
        }
    }

    #[test]
    fn parse_stop_command() {
        let cli = Cli::parse_from(["jolt", "stop"]);
        assert!(matches!(cli.command, Commands::Stop));
    }

    #[test]
    fn parse_status_command() {
        let cli = Cli::parse_from(["jolt", "status"]);
        assert!(matches!(cli.command, Commands::Status));
    }

    #[test]
    fn parse_relay_status_commands() {
        let cli = Cli::parse_from(["jolt", "relay", "status"]);
        match cli.command {
            Commands::Relay {
                command: RelayCommands::Status { json },
            } => assert!(!json),
            _ => panic!("expected Relay Status command"),
        }

        let cli = Cli::parse_from(["jolt", "relay", "status", "--json"]);
        match cli.command {
            Commands::Relay {
                command: RelayCommands::Status { json },
            } => assert!(json),
            _ => panic!("expected Relay Status command"),
        }
    }

    #[test]
    fn parse_publish_command() {
        let cli = Cli::parse_from(["jolt", "publish", "/tmp/test.txt"]);
        match cli.command {
            Commands::Publish {
                file,
                path,
                pin_home_relay,
            } => {
                assert_eq!(file, PathBuf::from("/tmp/test.txt"));
                assert!(path.is_none());
                assert!(!pin_home_relay);
            }
            _ => panic!("expected Publish command"),
        }
    }

    #[test]
    fn parse_publish_command_with_jolt_path() {
        let cli = Cli::parse_from(["jolt", "publish", "/tmp/test.txt", "--path", "/hello"]);
        match cli.command {
            Commands::Publish {
                file,
                path,
                pin_home_relay,
            } => {
                assert_eq!(file, PathBuf::from("/tmp/test.txt"));
                assert_eq!(path.as_deref(), Some("/hello"));
                assert!(!pin_home_relay);
            }
            _ => panic!("expected Publish command"),
        }
    }

    #[test]
    fn parse_fetch_command() {
        let cli = Cli::parse_from(["jolt", "fetch", "bafk_test_id"]);
        match cli.command {
            Commands::Fetch { target, output, .. } => {
                assert_eq!(target, "bafk_test_id");
                assert!(output.is_none());
            }
            _ => panic!("expected Fetch command"),
        }
    }

    #[test]
    fn parse_fetch_command_with_jolt_address() {
        let cli = Cli::parse_from(["jolt", "fetch", "alice.jolt/profile"]);
        match cli.command {
            Commands::Fetch { target, output, .. } => {
                assert_eq!(target, "alice.jolt/profile");
                assert!(output.is_none());
            }
            _ => panic!("expected Fetch command"),
        }
    }

    #[test]
    fn parse_fetch_command_with_output() {
        let cli = Cli::parse_from(["jolt", "fetch", "bafk_test_id", "-o", "/tmp/out.bin"]);
        match cli.command {
            Commands::Fetch { target, output, .. } => {
                assert_eq!(target, "bafk_test_id");
                assert_eq!(output, Some(PathBuf::from("/tmp/out.bin")));
            }
            _ => panic!("expected Fetch command"),
        }
    }

    #[test]
    fn parse_resolve_command() {
        let cli = Cli::parse_from(["jolt", "resolve", "alice.jolt/profile"]);
        match cli.command {
            Commands::Resolve { address } => {
                assert_eq!(address, "alice.jolt/profile");
            }
            _ => panic!("expected Resolve command"),
        }
    }

    #[test]
    fn parse_cache_stats_command() {
        let cli = Cli::parse_from(["jolt", "cache", "stats"]);
        assert!(matches!(
            cli.command,
            Commands::Cache {
                command: CacheCommands::Stats
            }
        ));
    }

    #[test]
    fn parse_cache_list_command() {
        let cli = Cli::parse_from(["jolt", "cache", "list"]);
        assert!(matches!(
            cli.command,
            Commands::Cache {
                command: CacheCommands::List
            }
        ));
    }

    #[test]
    fn parse_cache_pin_command() {
        let cli = Cli::parse_from(["jolt", "cache", "pin", "bafk_test"]);
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
        let cli = Cli::parse_from(["jolt", "cache", "unpin", "bafk_test"]);
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
        let cli = Cli::parse_from(["jolt", "bootstrap", "list"]);
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
            "jolt",
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
            "jolt",
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
