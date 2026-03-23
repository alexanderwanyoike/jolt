use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "dweb", about = "Decentralized web platform", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the dweb node
    Start,

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

    /// Manage the content cache
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
    },
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_start_command() {
        let cli = Cli::parse_from(["dweb", "start"]);
        assert!(matches!(cli.command, Commands::Start));
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
            Commands::Fetch { content_id, output } => {
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
            Commands::Fetch { content_id, output } => {
                assert_eq!(content_id, "bafk_test_id");
                assert_eq!(output, Some(PathBuf::from("/tmp/out.bin")));
            }
            _ => panic!("expected Fetch command"),
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
}
