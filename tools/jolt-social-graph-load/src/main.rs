use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use jolt_social_graph_load::{runtime, WorkloadConfig};

#[derive(Debug, Parser)]
#[command(
    name = "jolt-social-graph-load",
    about = "Deterministic Spoke-shaped Jolt follower-scale baseline"
)]
struct Args {
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(long, default_value_t = 3)]
    daemons: usize,
    #[arg(long, default_value_t = 100)]
    identities: usize,
    #[arg(long, default_value_t = 100)]
    follows: usize,
    #[arg(long, default_value_t = 1)]
    records_per_identity: usize,
    #[arg(long, default_value_t = 0)]
    publish_rate_per_second: u64,
    #[arg(long, default_value_t = 32)]
    concurrency: usize,
    #[arg(long, default_value_t = 0)]
    one_way_latency_ms: u64,
    #[arg(long, default_value_t = 0)]
    bandwidth_kbps: u64,
    #[arg(long, default_value_t = 0)]
    loss_percent: u8,
    #[arg(long, default_value_t = 0)]
    churn_percent: u8,
    #[arg(long, default_value_t = 250)]
    churn_duration_ms: u64,
    #[arg(long, default_value_t = 0)]
    provider_record_capacity: usize,
    #[arg(long)]
    workdir: Option<PathBuf>,
    #[arg(long)]
    keep_workdir: bool,
    #[arg(long)]
    json_output: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_target(false)
        .try_init()
        .ok();

    let args = Args::parse();
    let workload = WorkloadConfig {
        seed: args.seed,
        daemon_count: args.daemons,
        identity_count: args.identities,
        follow_count: args.follows,
        records_per_identity: args.records_per_identity,
        churn_percent: args.churn_percent,
    };
    let config = runtime::RunConfig {
        workload,
        publish_rate_per_second: args.publish_rate_per_second,
        concurrency: args.concurrency,
        churn_duration_ms: args.churn_duration_ms,
        provider_record_capacity: args.provider_record_capacity,
        network: runtime::NetworkProfile {
            one_way_latency_ms: args.one_way_latency_ms,
            bandwidth_kbps: args.bandwidth_kbps,
            loss_percent: args.loss_percent,
        },
    };

    let mut temporary = None;
    let workdir = match args.workdir {
        Some(path) => path,
        None => {
            let directory = tempfile::Builder::new()
                .prefix("jolt-social-graph-load-")
                .tempdir()
                .context("create benchmark workdir")?;
            let path = directory.path().to_path_buf();
            temporary = Some(directory);
            path
        }
    };
    std::fs::create_dir_all(&workdir).context("create benchmark workdir")?;

    let report = runtime::run(config, &workdir).await?;
    let json = serde_json::to_string_pretty(&report)?;
    if let Some(output) = args.json_output {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).context("create result directory")?;
        }
        std::fs::write(&output, format!("{json}\n"))
            .with_context(|| format!("write {}", output.display()))?;
        eprintln!("wrote {}", output.display());
    }
    println!("{json}");

    if args.keep_workdir {
        if let Some(directory) = temporary {
            let kept = directory.keep();
            eprintln!("kept workdir at {}", kept.display());
        }
    }
    Ok(())
}
