use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context};
use clap::ValueEnum;
use jolt_core::{
    ContentId, DeviceAuthorizationOperation, DeviceAuthorizationRecord, DeviceWriterLogEntry,
    DeviceWriterOperation, DeviceWriterPathMode, JoltAddress, UpdateAction, UpdateLogEntry,
};
use jolt_identity::NodeIdentity;
use jolt_network::{
    DaemonCommand, DaemonHandle, MaterializedRecordInfo, MaterializedRecordRefreshOutcome,
    NetworkConfig, NetworkNode, NodeStatus,
};
use jolt_store::{CacheConfig, ContentStore};
use libp2p::multiaddr::Protocol;
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch, Semaphore},
    task::{JoinHandle, JoinSet},
};

use crate::{
    deterministic_bytes, AuthorPlan, PhaseAccounting, PhaseSummary, RecordPlan, WorkloadConfig,
    WorkloadPlan,
};

const POSTS_PREFIX: &str = "/spoke/posts/";
const RESULT_VERSION: u32 = 3;
const VISIBILITY_POLL_DEADLINE: Duration = Duration::from_secs(75);
const VISIBILITY_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkProfile {
    pub one_way_latency_ms: u64,
    pub bandwidth_kbps: u64,
    pub loss_percent: u8,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum TimelinePath {
    LegacyRefresh,
    #[default]
    CacheFirst,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunConfig {
    pub workload: WorkloadConfig,
    pub timeline_path: TimelinePath,
    pub publish_rate_per_second: u64,
    pub concurrency: usize,
    pub churn_duration_ms: u64,
    /// Zero preserves libp2p's production default.
    pub provider_record_capacity: usize,
    pub reader_cache_max_bytes: u64,
    pub network: NetworkProfile,
}

impl RunConfig {
    fn validate(&self) -> anyhow::Result<()> {
        self.workload.validate()?;
        if self.concurrency == 0 {
            bail!("concurrency must be at least one");
        }
        if self.network.loss_percent > 100 {
            bail!("loss_percent cannot exceed 100");
        }
        if self.reader_cache_max_bytes == 0 {
            bail!("reader_cache_max_bytes must be at least one");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentReport {
    pub os: String,
    pub architecture: String,
    pub logical_cpus: usize,
    pub total_memory_bytes: u64,
    pub jolt_version: String,
    pub transport: String,
    pub process_model: String,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkBytes {
    pub reader_to_providers: u64,
    pub providers_to_reader: u64,
    pub dropped: u64,
}

impl NetworkBytes {
    fn difference(&self, before: &Self) -> Self {
        Self {
            reader_to_providers: self
                .reader_to_providers
                .saturating_sub(before.reader_to_providers),
            providers_to_reader: self
                .providers_to_reader
                .saturating_sub(before.providers_to_reader),
            dropped: self.dropped.saturating_sub(before.dropped),
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActivityReport {
    pub identity_sync_requests: u64,
    pub resolves: u64,
    pub fetches: u64,
    pub provider_announcements: u64,
    pub content_announcements: u64,
    pub churn_events: u64,
    pub refresh_ready: u64,
    pub refresh_network_unavailable: u64,
    pub refresh_verification_failed: u64,
    pub refresh_overloaded: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct CacheReport {
    pub cached_items: usize,
    pub cached_bytes: u64,
    pub published_items: usize,
    pub published_bytes: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessReport {
    pub rss_bytes: u64,
    pub virtual_memory_bytes: u64,
    pub accumulated_cpu_millis: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PhaseReport {
    pub name: String,
    pub wall_time_micros: u64,
    pub timeline_latency: PhaseSummary,
    pub network_bytes: NetworkBytes,
    pub activity: ActivityReport,
    pub daemon_api_latency: PhaseSummary,
    pub sync_work: SyncWorkReport,
    pub cache_before: CacheReport,
    pub cache_after: CacheReport,
    pub process_before: ProcessReport,
    pub process_after: ProcessReport,
    pub cpu_millis: u64,
    pub rss_growth_bytes: i64,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncWorkReport {
    pub max_concurrency: usize,
    pub queue_capacity: usize,
    pub peak_active: usize,
    pub peak_queued: usize,
    pub verified: u64,
    pub verification_failed: u64,
    pub rejected: u64,
    pub timed_out: u64,
    pub full_responses: u64,
    pub delta_responses: u64,
    pub delta_continuations: u64,
    pub received_entries: u64,
    pub received_bytes: u64,
    pub full_recoveries: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PropagationReport {
    pub published_records: u64,
    pub visible_records: u64,
    pub failed_records: u64,
    pub first_attempt_misses: u64,
    pub latency_micros: crate::LatencyPercentiles,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub result_version: u32,
    pub generated_at_unix_secs: u64,
    pub config: RunConfig,
    pub plan_digest: String,
    pub environment: EnvironmentReport,
    pub setup_activity: ActivityReport,
    pub phases: Vec<PhaseReport>,
    pub propagation: PropagationReport,
    pub restart_startup_micros: u64,
    pub final_network_bytes: NetworkBytes,
    pub limitations: Vec<String>,
}

#[derive(Clone)]
struct LinkCounters {
    reader_to_provider: Arc<AtomicU64>,
    provider_to_reader: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    chunks: Arc<AtomicU64>,
}

impl LinkCounters {
    fn new() -> Self {
        Self {
            reader_to_provider: Arc::new(AtomicU64::new(0)),
            provider_to_reader: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
            chunks: Arc::new(AtomicU64::new(0)),
        }
    }
}

struct ShapedLink {
    port: u16,
    counters: LinkCounters,
    offline: watch::Sender<bool>,
    accept_task: JoinHandle<()>,
}

impl ShapedLink {
    async fn start(upstream: SocketAddr, profile: NetworkProfile) -> anyhow::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        let counters = LinkCounters::new();
        let (offline, offline_state) = watch::channel(false);
        let task_counters = counters.clone();
        let accept_task = tokio::spawn(async move {
            while let Ok((downstream, _)) = listener.accept().await {
                let counters = task_counters.clone();
                let offline = offline_state.clone();
                let profile = profile.clone();
                tokio::spawn(async move {
                    if *offline.borrow() {
                        return;
                    }
                    let Ok(upstream_stream) = TcpStream::connect(upstream).await else {
                        return;
                    };
                    let (downstream_read, downstream_write) = downstream.into_split();
                    let (upstream_read, upstream_write) = upstream_stream.into_split();
                    let left = pump(
                        downstream_read,
                        upstream_write,
                        counters.reader_to_provider.clone(),
                        counters.clone(),
                        offline.clone(),
                        profile.clone(),
                    );
                    let right = pump(
                        upstream_read,
                        downstream_write,
                        counters.provider_to_reader.clone(),
                        counters,
                        offline,
                        profile,
                    );
                    let _ = tokio::join!(left, right);
                });
            }
        });
        Ok(Self {
            port,
            counters,
            offline,
            accept_task,
        })
    }

    fn set_offline(&self, offline: bool) {
        self.offline.send_replace(offline);
    }

    fn snapshot(&self) -> NetworkBytes {
        NetworkBytes {
            reader_to_providers: self.counters.reader_to_provider.load(Ordering::Relaxed),
            providers_to_reader: self.counters.provider_to_reader.load(Ordering::Relaxed),
            dropped: self.counters.dropped.load(Ordering::Relaxed),
        }
    }
}

impl Drop for ShapedLink {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

async fn pump<R, W>(
    mut reader: R,
    mut writer: W,
    forwarded: Arc<AtomicU64>,
    counters: LinkCounters,
    mut offline: watch::Receiver<bool>,
    profile: NetworkProfile,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        if *offline.borrow() {
            break;
        }
        let read = tokio::select! {
            result = reader.read(&mut buffer) => match result {
                Ok(read) => read,
                Err(_) => break,
            },
            changed = offline.changed() => {
                if changed.is_err() || *offline.borrow() {
                    break;
                }
                continue;
            }
        };
        if read == 0 {
            break;
        }
        if profile.one_way_latency_ms > 0 {
            tokio::time::sleep(Duration::from_millis(profile.one_way_latency_ms)).await;
        }
        let chunk = counters.chunks.fetch_add(1, Ordering::Relaxed);
        if profile.loss_percent > 0 && chunk % 100 < u64::from(profile.loss_percent) {
            counters.dropped.fetch_add(read as u64, Ordering::Relaxed);
            continue;
        }
        if profile.bandwidth_kbps > 0 {
            let seconds = (read as f64 * 8.0) / (profile.bandwidth_kbps as f64 * 1_000.0);
            tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
        }
        if writer.write_all(&buffer[..read]).await.is_err() {
            break;
        }
        if writer.flush().await.is_err() {
            break;
        }
        forwarded.fetch_add(read as u64, Ordering::Relaxed);
    }
    let _ = writer.shutdown().await;
}

struct RunningDaemon {
    handle: DaemonHandle,
    peer_id: String,
    listen_addr: String,
    task: JoinHandle<()>,
}

impl RunningDaemon {
    async fn stop(&mut self) {
        let _ = self.handle.shutdown().await;
        let _ = (&mut self.task).await;
    }

    async fn shutdown(mut self) {
        self.stop().await;
    }
}

struct AuthorState {
    plan: AuthorPlan,
    authority: Vec<DeviceAuthorizationRecord>,
    writer_log: Vec<DeviceWriterLogEntry>,
}

#[derive(Default)]
struct TimelineOutcome {
    accounting: PhaseAccounting,
    activity: ActivityReport,
    visibility: PhaseAccounting,
    first_attempt_misses: u64,
}

struct AuthorOutcome {
    published_at: Option<Instant>,
    completed_at: Instant,
    latency_micros: u64,
    resolves: u64,
    fetches: u64,
    sync_requests: u64,
    refresh: Option<MaterializedRecordRefreshOutcome>,
    first_attempt_missed: bool,
    error: Option<String>,
}

#[derive(Clone, Copy)]
enum TimelineOperation {
    LegacyRefresh,
    CacheRefreshAll,
    CacheReadAll,
    CacheRefreshLatest,
}

#[derive(Clone, Copy)]
enum RecordFetch {
    All,
    Latest,
}

impl TimelineOperation {
    fn cold(path: TimelinePath) -> Self {
        match path {
            TimelinePath::LegacyRefresh => Self::LegacyRefresh,
            TimelinePath::CacheFirst => Self::CacheRefreshAll,
        }
    }

    fn warm(path: TimelinePath) -> Self {
        match path {
            TimelinePath::LegacyRefresh => Self::LegacyRefresh,
            TimelinePath::CacheFirst => Self::CacheReadAll,
        }
    }

    fn active(path: TimelinePath) -> Self {
        match path {
            TimelinePath::LegacyRefresh => Self::LegacyRefresh,
            TimelinePath::CacheFirst => Self::CacheRefreshLatest,
        }
    }

    fn sync_requests(self, attempts: u64) -> u64 {
        match self {
            Self::CacheReadAll => 0,
            Self::LegacyRefresh | Self::CacheRefreshAll | Self::CacheRefreshLatest => attempts,
        }
    }
}

fn writer_path_mode(path: TimelinePath) -> DeviceWriterPathMode {
    match path {
        TimelinePath::LegacyRefresh => DeviceWriterPathMode::Append,
        TimelinePath::CacheFirst => DeviceWriterPathMode::Singleton,
    }
}

pub async fn run(config: RunConfig, workdir: &Path) -> anyhow::Result<BenchmarkReport> {
    config.validate()?;
    if workdir.read_dir()?.next().is_some() {
        bail!(
            "workdir {} must be empty so the cold phase cannot reuse cached state",
            workdir.display()
        );
    }
    let plan = WorkloadPlan::generate(&config.workload)?;
    let plan_json = serde_json::to_vec(&plan)?;
    let plan_digest = blake3::hash(&plan_json).to_hex().to_string();

    let mut providers = Vec::with_capacity(plan.provider_count);
    for index in 0..plan.provider_count {
        providers.push(
            start_daemon(
                workdir,
                config.workload.seed,
                "provider",
                index,
                config.provider_record_capacity,
                CacheConfig::default().max_size_bytes,
                0,
            )
            .await?,
        );
    }
    let mut links = Vec::with_capacity(plan.provider_count);
    for provider in &providers {
        links.push(
            ShapedLink::start(
                listener_socket(&provider.listen_addr)?,
                config.network.clone(),
            )
            .await?,
        );
    }
    let mut reader = start_daemon(
        workdir,
        config.workload.seed,
        "reader",
        plan.provider_count,
        config.provider_record_capacity,
        config.reader_cache_max_bytes,
        0,
    )
    .await?;
    connect_reader_to_providers(&reader, &providers, &links).await?;

    let mut author_states = seed_authors(&plan, &providers, workdir, config.timeline_path).await?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    let setup_activity = ActivityReport {
        provider_announcements: plan.authors.len() as u64,
        content_announcements: plan.record_count() as u64,
        ..ActivityReport::default()
    };

    let mut system = System::new();
    let mut measurement = MeasurementContext {
        reader: &reader.handle,
        plan: &plan,
        timeline_path: config.timeline_path,
        concurrency: config.concurrency,
        links: &links,
        system: &mut system,
    };
    let mut phases = Vec::new();
    let cold = measurement
        .measure(
            "cold",
            config.workload.records_per_identity,
            TimelineOperation::cold(config.timeline_path),
        )
        .await?
        .report;
    print_phase_progress(&cold);
    phases.push(cold);
    wait_for_link_quiescence(&links).await;
    let warm = measurement
        .measure(
            "warm_no_change",
            config.workload.records_per_identity,
            TimelineOperation::warm(config.timeline_path),
        )
        .await?
        .report;
    print_phase_progress(&warm);
    phases.push(warm);

    let measured_new_phase = measurement
        .measure_new_records(
            &mut author_states,
            &providers,
            workdir,
            config.publish_rate_per_second,
        )
        .await?;
    let mut new_phase = measured_new_phase.report;
    new_phase.activity.content_announcements = plan.followed_authors.len() as u64;
    new_phase.activity.churn_events = plan.churned_providers.len() as u64;
    print_phase_progress(&new_phase);
    let visible = measured_new_phase.visibility.successes;
    let published_records = plan.followed_authors.len() as u64;
    let propagation = PropagationReport {
        published_records,
        visible_records: visible,
        failed_records: published_records.saturating_sub(visible),
        first_attempt_misses: measured_new_phase.first_attempt_misses,
        latency_micros: measured_new_phase.visibility.latency_micros,
    };
    phases.push(new_phase);

    if !plan.churned_providers.is_empty() && config.timeline_path == TimelinePath::CacheFirst {
        let mut stopped_providers = Vec::with_capacity(plan.churned_providers.len());
        for index in &plan.churned_providers {
            links[*index].set_offline(true);
            let port = listener_socket(&providers[*index].listen_addr)?.port();
            let peer_id = providers[*index].peer_id.clone();
            providers[*index].stop().await;
            stopped_providers.push((*index, port, peer_id));
        }
        wait_for_peer_absence(
            &reader.handle,
            &stopped_providers
                .iter()
                .map(|(_, _, peer_id)| peer_id.clone())
                .collect::<Vec<_>>(),
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(config.churn_duration_ms)).await;
        wait_for_link_quiescence(&links).await;

        let mut offline_warm = measurement
            .measure(
                "offline_warm",
                config.workload.records_per_identity + 1,
                TimelineOperation::CacheReadAll,
            )
            .await?
            .report;
        offline_warm.activity.churn_events = plan.churned_providers.len() as u64;
        print_phase_progress(&offline_warm);
        phases.push(offline_warm);

        let offline_refresh = measurement
            .measure(
                "offline_refresh",
                config.workload.records_per_identity + 1,
                TimelineOperation::CacheRefreshAll,
            )
            .await?
            .report;
        print_phase_progress(&offline_refresh);
        phases.push(offline_refresh);

        for (index, port, _) in stopped_providers {
            providers[index] = start_daemon(
                workdir,
                config.workload.seed,
                "provider",
                index,
                config.provider_record_capacity,
                CacheConfig::default().max_size_bytes,
                port,
            )
            .await?;
            links[index].set_offline(false);
        }
        connect_reader_to_providers(&reader, &providers, &links).await?;

        let recovered = measurement
            .measure(
                "churn_recovery",
                config.workload.records_per_identity + 1,
                TimelineOperation::CacheRefreshAll,
            )
            .await?
            .report;
        print_phase_progress(&recovered);
        phases.push(recovered);
    }

    drop(measurement);
    reader.shutdown().await;
    let restart_started = Instant::now();
    reader = start_daemon(
        workdir,
        config.workload.seed,
        "reader",
        plan.provider_count,
        config.provider_record_capacity,
        config.reader_cache_max_bytes,
        0,
    )
    .await?;
    let restart_startup_micros = restart_started.elapsed().as_micros() as u64;
    connect_reader_to_providers(&reader, &providers, &links).await?;
    wait_for_link_quiescence(&links).await;
    let mut restart_measurement = MeasurementContext {
        reader: &reader.handle,
        plan: &plan,
        timeline_path: config.timeline_path,
        concurrency: config.concurrency,
        links: &links,
        system: &mut system,
    };
    let restarted = restart_measurement
        .measure(
            "restart_warm",
            config.workload.records_per_identity + 1,
            TimelineOperation::warm(config.timeline_path),
        )
        .await?
        .report;
    print_phase_progress(&restarted);
    phases.push(restarted);

    let final_network_bytes = snapshot_links(&links);
    let environment = environment_report();

    reader.shutdown().await;
    for provider in providers {
        provider.shutdown().await;
    }

    let mut limitations = vec![
        "Daemons are real NetworkNode daemon loops in one OS process, so CPU and RSS are aggregate rather than per daemon.".to_string(),
        "The workload uses localhost TCP; the optional shaper adds delay and bandwidth per TCP chunk, and loss deliberately drops encrypted stream chunks rather than modelling kernel-level packet retransmission.".to_string(),
        "Provider activity counts harness requests and announcements; Jolt does not yet expose internal Kademlia packet counters.".to_string(),
        "This local benchmark does not exercise iroh hole punching, NATS coordination, relays, or Internet path diversity.".to_string(),
        "Results are comparative engineering evidence, not an Internet-wide capacity or marketing claim.".to_string(),
    ];
    if config.provider_record_capacity > 0 {
        limitations.push(format!(
            "The load harness raised each daemon's local DHT provided-key capacity to {}; normal nodes retain libp2p's default. A separate default-capacity run must record the unmodified ceiling.",
            config.provider_record_capacity
        ));
    }

    Ok(BenchmarkReport {
        result_version: RESULT_VERSION,
        generated_at_unix_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        config,
        plan_digest,
        environment,
        setup_activity,
        phases,
        propagation,
        restart_startup_micros,
        final_network_bytes,
        limitations,
    })
}

async fn connect_reader_to_providers(
    reader: &RunningDaemon,
    providers: &[RunningDaemon],
    links: &[ShapedLink],
) -> anyhow::Result<()> {
    for (provider, link) in providers.iter().zip(links) {
        reader
            .handle
            .connect_peer(format!(
                "/ip4/127.0.0.1/tcp/{}/p2p/{}",
                link.port, provider.peer_id
            ))
            .await
            .with_context(|| format!("connect reader to provider {}", provider.peer_id))?;
    }
    wait_for_peer_count(&reader.handle, providers.len()).await
}

fn print_phase_progress(phase: &PhaseReport) {
    let failures: u64 = phase.timeline_latency.failures.values().sum();
    eprintln!(
        "phase {} complete in {:.3}s: {} succeeded, {} failed",
        phase.name,
        phase.wall_time_micros as f64 / 1_000_000.0,
        phase.timeline_latency.successes,
        failures
    );
}

async fn start_daemon(
    workdir: &Path,
    seed: u64,
    domain: &str,
    index: usize,
    provider_record_capacity: usize,
    cache_max_bytes: u64,
    listen_port: u16,
) -> anyhow::Result<RunningDaemon> {
    let root = workdir.join(format!("{domain}-{index}"));
    std::fs::create_dir_all(&root)?;
    let identity = NodeIdentity::from_signing_key_bytes(&deterministic_bytes(seed, domain, index))
        .map_err(|error| anyhow::anyhow!("create {domain} identity {index}: {error}"))?;
    let identity_id = identity.identity_id().to_string();
    let store = ContentStore::open(
        &root,
        CacheConfig {
            max_size_bytes: cache_max_bytes,
        },
    )?;
    let mut node = NetworkNode::new_tcp(
        identity,
        store,
        NetworkConfig {
            enable_mdns: false,
            provider_record_capacity: (provider_record_capacity > 0)
                .then_some(provider_record_capacity),
            ..NetworkConfig::test_config()
        },
    )?;
    let listen_address = format!("/ip4/127.0.0.1/tcp/{listen_port}");
    node.listen_on(&listen_address)?;
    node = wait_for_listener(node).await?;
    let peer_id = node.local_peer_id().to_string();
    let listen_addr = node
        .listeners()
        .first()
        .context("daemon has no listener")?
        .to_string();
    let (tx, rx) = mpsc::channel::<DaemonCommand>(4_096);
    let handle = DaemonHandle::new_with_local_identity(tx, identity_id);
    let task = tokio::spawn(async move { node.run_daemon_loop(rx).await });
    Ok(RunningDaemon {
        handle,
        peer_id,
        listen_addr,
        task,
    })
}

async fn wait_for_listener(mut node: NetworkNode) -> anyhow::Result<NetworkNode> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = node.next_event().await;
            node.handle_swarm_event(event);
            if !node.listeners().is_empty() {
                return node;
            }
        }
    })
    .await
    .context("timed out waiting for daemon listener")
}

fn listener_socket(address: &str) -> anyhow::Result<SocketAddr> {
    let multiaddr: libp2p::Multiaddr = address.parse()?;
    let mut ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let mut port = None;
    for protocol in multiaddr.iter() {
        match protocol {
            Protocol::Ip4(value) => ip = IpAddr::V4(value),
            Protocol::Ip6(value) => ip = IpAddr::V6(value),
            Protocol::Tcp(value) => port = Some(value),
            _ => {}
        }
    }
    Ok(SocketAddr::new(
        ip,
        port.context("listener has no TCP port")?,
    ))
}

async fn wait_for_peer_count(handle: &DaemonHandle, expected: usize) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if handle
                .status()
                .await
                .is_ok_and(|status| status.connected_peers >= expected)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("timed out connecting benchmark daemons")
}

async fn wait_for_peer_absence(
    handle: &DaemonHandle,
    absent_peer_ids: &[String],
) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if handle.peers().await.is_ok_and(|peers| {
                peers
                    .iter()
                    .all(|peer| !absent_peer_ids.contains(&peer.peer_id))
            }) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("timed out disconnecting benchmark providers")
}

async fn seed_authors(
    plan: &WorkloadPlan,
    providers: &[RunningDaemon],
    workdir: &Path,
    timeline_path: TimelinePath,
) -> anyhow::Result<Vec<AuthorState>> {
    let mut states = Vec::with_capacity(plan.authors.len());
    for author in &plan.authors {
        let owner = author.identity_key()?;
        let identity = owner.identity_id();
        let device_id = format!("dev_load_{:05}", author.index);
        let issued_at = 1_700_000_000 + author.index as u64 * 1_000;
        let authority = vec![DeviceAuthorizationRecord::genesis(
            owner.public_key_bytes(),
            identity.clone(),
            DeviceAuthorizationOperation::authorize_device(
                device_id.clone(),
                owner.public_key_bytes(),
                vec!["identity:write".to_string()],
                Some("Card 110 deterministic load identity".to_string()),
                issued_at,
            ),
            issued_at,
            |bytes| owner.sign(bytes),
        )?];
        let provider = &providers[author.provider_index];
        let mut writer_log: Vec<DeviceWriterLogEntry> = Vec::with_capacity(author.records.len());
        let mut first_content = None;
        for record in &author.records {
            let content = publish_record(provider, workdir, author.index, record).await?;
            first_content.get_or_insert_with(|| content.clone());
            let operation = DeviceWriterOperation::set_path(
                record.path.clone(),
                content,
                writer_path_mode(timeline_path),
            );
            let created_at = issued_at + record.index as u64 + 1;
            let entry = match writer_log.last() {
                Some(previous) => {
                    previous.append(operation, created_at, |bytes| owner.sign(bytes))?
                }
                None => DeviceWriterLogEntry::genesis(
                    identity.clone(),
                    device_id.clone(),
                    operation,
                    created_at,
                    |bytes| owner.sign(bytes),
                )?,
            };
            writer_log.push(entry);
        }
        provider
            .handle
            .store_device_writer_logs(
                identity.clone(),
                authority.clone(),
                vec![writer_log.clone()],
            )
            .await?;
        let update_log = vec![UpdateLogEntry::genesis(
            owner.public_key_bytes(),
            UpdateAction::SetPath {
                path: "/profile".to_string(),
                content_id: first_content.context("author must have one content record")?,
            },
            |bytes| owner.sign(bytes),
        )?];
        provider
            .handle
            .store_update_log(identity, update_log)
            .await?;
        states.push(AuthorState {
            plan: author.clone(),
            authority,
            writer_log,
        });
        if states.len() % 100 == 0 || states.len() == plan.authors.len() {
            eprintln!("seeded {}/{} identities", states.len(), plan.authors.len());
        }
    }
    Ok(states)
}

async fn publish_record(
    provider: &RunningDaemon,
    workdir: &Path,
    author_index: usize,
    record: &RecordPlan,
) -> anyhow::Result<ContentId> {
    let path = workdir.join(format!(
        "content-author-{author_index:05}-record-{:05}.json",
        record.index
    ));
    std::fs::write(&path, record.body.as_bytes())?;
    let published = provider.handle.publish(path, None).await?;
    ContentId::from_str(&published.content_id)
        .map_err(|error| anyhow::anyhow!("daemon returned invalid content id: {error}"))
}

async fn publish_and_refresh_new_records(
    states: &mut [AuthorState],
    plan: &WorkloadPlan,
    providers: &[RunningDaemon],
    workdir: &Path,
    rate: u64,
    reader: &DaemonHandle,
    concurrency: usize,
    timeline_path: TimelinePath,
) -> anyhow::Result<TimelineOutcome> {
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut tasks = JoinSet::new();
    let delay = (rate > 0).then(|| Duration::from_secs_f64(1.0 / rate as f64));
    for author_index in &plan.followed_authors {
        let state = &mut states[*author_index];
        let record_index = state.plan.records.len();
        let record = RecordPlan {
            index: record_index,
            path: format!(
                "/spoke/posts/author-{:05}-record-{record_index:05}",
                state.plan.index
            ),
            body: serde_json::json!({
                "authorIndex": state.plan.index,
                "recordIndex": record_index,
                "seed": plan.seed,
                "text": format!("new deterministic post from author {}", state.plan.index),
            })
            .to_string(),
        };
        let provider = &providers[state.plan.provider_index];
        let content = publish_record(provider, workdir, state.plan.index, &record).await?;
        let owner = state.plan.identity_key()?;
        let operation = DeviceWriterOperation::set_path(
            record.path.clone(),
            content,
            writer_path_mode(timeline_path),
        );
        let created_at = 1_800_000_000 + state.plan.index as u64 * 1_000 + record_index as u64;
        let entry = state
            .writer_log
            .last()
            .context("seeded writer log is empty")?
            .append(operation, created_at, |bytes| owner.sign(bytes))?;
        state.writer_log.push(entry);
        provider
            .handle
            .store_device_writer_logs(
                owner.identity_id(),
                state.authority.clone(),
                vec![state.writer_log.clone()],
            )
            .await?;
        state.plan.records.push(record);
        spawn_author_refresh(
            &mut tasks,
            semaphore.clone(),
            reader.clone(),
            state.plan.clone(),
            state.plan.records.len(),
            Some(Instant::now()),
            TimelineOperation::active(timeline_path),
        );
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
    }
    Ok(collect_author_outcomes(tasks).await)
}

struct MeasuredPhase {
    report: PhaseReport,
    visibility: PhaseSummary,
    first_attempt_misses: u64,
}

#[derive(Default)]
struct StatusSampling {
    accounting: PhaseAccounting,
    peak_active: usize,
    peak_queued: usize,
}

impl StatusSampling {
    fn observe(&mut self, status: &NodeStatus) {
        self.peak_active = self.peak_active.max(status.device_writer_sync_work.active);
        self.peak_queued = self.peak_queued.max(status.device_writer_sync_work.queued);
    }
}

async fn sample_daemon_status(handle: DaemonHandle, stop: Arc<AtomicBool>) -> StatusSampling {
    let mut sampling = StatusSampling::default();
    loop {
        let started = Instant::now();
        match handle.status().await {
            Ok(status) => {
                sampling
                    .accounting
                    .record_success(started.elapsed().as_micros() as u64);
                sampling.observe(&status);
            }
            Err(error) => sampling.accounting.record_failure(
                started.elapsed().as_micros() as u64,
                classify_error(&error.to_string()),
            ),
        }
        if stop.load(Ordering::Relaxed) {
            return sampling;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn sync_work_report(
    before: &NodeStatus,
    after: &NodeStatus,
    sampling: &StatusSampling,
) -> SyncWorkReport {
    let before = &before.device_writer_sync_work;
    let after = &after.device_writer_sync_work;
    SyncWorkReport {
        max_concurrency: after.max_concurrency,
        queue_capacity: after.queue_capacity,
        peak_active: sampling.peak_active.max(before.active).max(after.active),
        peak_queued: sampling.peak_queued.max(before.queued).max(after.queued),
        verified: after.verified.saturating_sub(before.verified),
        verification_failed: after
            .verification_failed
            .saturating_sub(before.verification_failed),
        rejected: after.rejected.saturating_sub(before.rejected),
        timed_out: after.timed_out.saturating_sub(before.timed_out),
        full_responses: after.full_responses.saturating_sub(before.full_responses),
        delta_responses: after.delta_responses.saturating_sub(before.delta_responses),
        delta_continuations: after
            .delta_continuations
            .saturating_sub(before.delta_continuations),
        received_entries: after
            .received_entries
            .saturating_sub(before.received_entries),
        received_bytes: after.received_bytes.saturating_sub(before.received_bytes),
        full_recoveries: after.full_recoveries.saturating_sub(before.full_recoveries),
    }
}

struct MeasurementContext<'a> {
    reader: &'a DaemonHandle,
    plan: &'a WorkloadPlan,
    timeline_path: TimelinePath,
    concurrency: usize,
    links: &'a [ShapedLink],
    system: &'a mut System,
}

impl MeasurementContext<'_> {
    async fn measure(
        &mut self,
        name: &str,
        expected_records: usize,
        operation: TimelineOperation,
    ) -> anyhow::Result<MeasuredPhase> {
        let network_before = snapshot_links(self.links);
        let cache_before = cache_report(self.reader).await?;
        let process_before = process_report(self.system)?;
        let sync_before = self.reader.status().await?;
        let stop_sampling = Arc::new(AtomicBool::new(false));
        let status_task = tokio::spawn(sample_daemon_status(
            self.reader.clone(),
            stop_sampling.clone(),
        ));
        tokio::task::yield_now().await;
        let started = Instant::now();
        let outcome = run_timeline(
            self.reader,
            self.plan,
            expected_records,
            self.concurrency,
            operation,
        )
        .await;
        let wall_time_micros = started.elapsed().as_micros() as u64;
        stop_sampling.store(true, Ordering::Relaxed);
        let status_sampling = status_task.await.context("status sampler task failed")?;
        let sync_after = self.reader.status().await?;
        let process_after = process_report(self.system)?;
        let cache_after = cache_report(self.reader).await?;
        let network_after = snapshot_links(self.links);
        let cpu_millis = process_after
            .accumulated_cpu_millis
            .saturating_sub(process_before.accumulated_cpu_millis);
        let rss_growth_bytes = process_after.rss_bytes as i128 - process_before.rss_bytes as i128;
        Ok(MeasuredPhase {
            report: PhaseReport {
                name: name.to_string(),
                wall_time_micros,
                timeline_latency: outcome.accounting.summarize(),
                network_bytes: network_after.difference(&network_before),
                activity: outcome.activity,
                daemon_api_latency: status_sampling.accounting.summarize(),
                sync_work: sync_work_report(&sync_before, &sync_after, &status_sampling),
                cache_before,
                cache_after,
                process_before,
                process_after,
                cpu_millis,
                rss_growth_bytes: rss_growth_bytes.clamp(i64::MIN as i128, i64::MAX as i128) as i64,
            },
            visibility: outcome.visibility.summarize(),
            first_attempt_misses: outcome.first_attempt_misses,
        })
    }

    async fn measure_new_records(
        &mut self,
        states: &mut [AuthorState],
        providers: &[RunningDaemon],
        workdir: &Path,
        publish_rate_per_second: u64,
    ) -> anyhow::Result<MeasuredPhase> {
        let network_before = snapshot_links(self.links);
        let cache_before = cache_report(self.reader).await?;
        let process_before = process_report(self.system)?;
        let sync_before = self.reader.status().await?;
        let stop_sampling = Arc::new(AtomicBool::new(false));
        let status_task = tokio::spawn(sample_daemon_status(
            self.reader.clone(),
            stop_sampling.clone(),
        ));
        tokio::task::yield_now().await;
        let started = Instant::now();
        let outcome = publish_and_refresh_new_records(
            states,
            self.plan,
            providers,
            workdir,
            publish_rate_per_second,
            self.reader,
            self.concurrency,
            self.timeline_path,
        )
        .await?;
        let wall_time_micros = started.elapsed().as_micros() as u64;
        stop_sampling.store(true, Ordering::Relaxed);
        let status_sampling = status_task.await.context("status sampler task failed")?;
        let sync_after = self.reader.status().await?;
        let process_after = process_report(self.system)?;
        let cache_after = cache_report(self.reader).await?;
        let network_after = snapshot_links(self.links);
        let cpu_millis = process_after
            .accumulated_cpu_millis
            .saturating_sub(process_before.accumulated_cpu_millis);
        let rss_growth_bytes = process_after.rss_bytes as i128 - process_before.rss_bytes as i128;
        Ok(MeasuredPhase {
            report: PhaseReport {
                name: "new_record_refresh".to_string(),
                wall_time_micros,
                timeline_latency: outcome.accounting.summarize(),
                network_bytes: network_after.difference(&network_before),
                activity: outcome.activity,
                daemon_api_latency: status_sampling.accounting.summarize(),
                sync_work: sync_work_report(&sync_before, &sync_after, &status_sampling),
                cache_before,
                cache_after,
                process_before,
                process_after,
                cpu_millis,
                rss_growth_bytes: rss_growth_bytes.clamp(i64::MIN as i128, i64::MAX as i128) as i64,
            },
            visibility: outcome.visibility.summarize(),
            first_attempt_misses: outcome.first_attempt_misses,
        })
    }
}

async fn run_timeline(
    reader: &DaemonHandle,
    plan: &WorkloadPlan,
    expected_records: usize,
    concurrency: usize,
    operation: TimelineOperation,
) -> TimelineOutcome {
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut tasks = JoinSet::new();
    for author_index in &plan.followed_authors {
        let author = plan.authors[*author_index].clone();
        spawn_author_refresh(
            &mut tasks,
            semaphore.clone(),
            reader.clone(),
            author,
            expected_records,
            None,
            operation,
        );
    }

    collect_author_outcomes(tasks).await
}

fn spawn_author_refresh(
    tasks: &mut JoinSet<AuthorOutcome>,
    semaphore: Arc<Semaphore>,
    reader: DaemonHandle,
    author: AuthorPlan,
    expected_records: usize,
    published_at: Option<Instant>,
    operation: TimelineOperation,
) {
    tasks.spawn(async move {
        let permit = semaphore.acquire_owned().await;
        let started = Instant::now();
        let poll = match permit {
            Ok(_permit) if published_at.is_some() => {
                poll_until_visible(VISIBILITY_POLL_DEADLINE, VISIBILITY_POLL_INTERVAL, || {
                    read_author(&reader, &author, expected_records, operation)
                })
                .await
            }
            Ok(_permit) => VisibilityPollOutcome {
                result: read_author(&reader, &author, expected_records, operation).await,
                attempts: 1,
                first_attempt_missed: false,
            },
            Err(error) => VisibilityPollOutcome {
                result: Err((0, 0, error.to_string())),
                attempts: 0,
                first_attempt_missed: false,
            },
        };
        match poll.result {
            Ok((resolves, fetches, refresh)) => AuthorOutcome {
                published_at,
                completed_at: Instant::now(),
                latency_micros: started.elapsed().as_micros() as u64,
                resolves,
                fetches,
                sync_requests: operation.sync_requests(poll.attempts),
                refresh,
                first_attempt_missed: poll.first_attempt_missed,
                error: None,
            },
            Err((resolves, fetches, error)) => AuthorOutcome {
                published_at,
                completed_at: Instant::now(),
                latency_micros: started.elapsed().as_micros() as u64,
                resolves,
                fetches,
                sync_requests: operation.sync_requests(poll.attempts),
                refresh: None,
                first_attempt_missed: poll.first_attempt_missed,
                error: Some(error),
            },
        }
    });
}

async fn collect_author_outcomes(mut tasks: JoinSet<AuthorOutcome>) -> TimelineOutcome {
    let mut outcome = TimelineOutcome::default();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(author) => {
                outcome.activity.identity_sync_requests += author.sync_requests;
                outcome.first_attempt_misses += u64::from(author.first_attempt_missed);
                if let Some(published) = author.published_at {
                    let latency = author
                        .completed_at
                        .saturating_duration_since(published)
                        .as_micros() as u64;
                    if author.error.is_some() {
                        outcome.visibility.record_failure(latency, "not_visible");
                    } else {
                        outcome.visibility.record_success(latency);
                    }
                }
                outcome.activity.resolves += author.resolves;
                outcome.activity.fetches += author.fetches;
                if let Some(refresh) = author.refresh {
                    match refresh {
                        MaterializedRecordRefreshOutcome::Ready => {
                            outcome.activity.refresh_ready += 1;
                        }
                        MaterializedRecordRefreshOutcome::NetworkUnavailable => {
                            outcome.activity.refresh_network_unavailable += 1;
                        }
                        MaterializedRecordRefreshOutcome::VerificationFailed => {
                            outcome.activity.refresh_verification_failed += 1;
                        }
                        MaterializedRecordRefreshOutcome::Overloaded => {
                            outcome.activity.refresh_overloaded += 1;
                        }
                    }
                }
                match author.error {
                    Some(error) => outcome
                        .accounting
                        .record_failure(author.latency_micros, classify_error(&error)),
                    None => outcome.accounting.record_success(author.latency_micros),
                }
            }
            Err(error) => {
                outcome.activity.identity_sync_requests += 1;
                outcome
                    .accounting
                    .record_failure(0, format!("task:{error}"));
            }
        }
    }
    outcome
}

struct VisibilityPollOutcome<T, E> {
    result: Result<T, E>,
    attempts: u64,
    first_attempt_missed: bool,
}

async fn poll_until_visible<T, F, Fut>(
    max_wait: Duration,
    interval: Duration,
    mut refresh: F,
) -> VisibilityPollOutcome<T, (u64, u64, String)>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, (u64, u64, String)>>,
{
    let deadline = Instant::now() + max_wait;
    let mut attempts = 0;
    let mut first_attempt_missed = false;
    loop {
        attempts += 1;
        match refresh().await {
            Ok(value) => {
                return VisibilityPollOutcome {
                    result: Ok(value),
                    attempts,
                    first_attempt_missed,
                };
            }
            Err(error) => {
                let old_count = error.2.starts_with("record_count:");
                if attempts == 1 && old_count {
                    first_attempt_missed = true;
                }
                let now = Instant::now();
                if !old_count || now >= deadline {
                    return VisibilityPollOutcome {
                        result: Err(error),
                        attempts,
                        first_attempt_missed,
                    };
                }
                tokio::time::sleep(interval.min(deadline.saturating_duration_since(now))).await;
            }
        }
    }
}

async fn read_author(
    reader: &DaemonHandle,
    author: &AuthorPlan,
    expected_records: usize,
    operation: TimelineOperation,
) -> Result<(u64, u64, Option<MaterializedRecordRefreshOutcome>), (u64, u64, String)> {
    match operation {
        TimelineOperation::LegacyRefresh => refresh_legacy_author(reader, author, expected_records)
            .await
            .map(|(resolves, fetches)| (resolves, fetches, None)),
        TimelineOperation::CacheRefreshAll => {
            refresh_cache_first_author(reader, author, expected_records, RecordFetch::All).await
        }
        TimelineOperation::CacheReadAll => {
            read_cached_author(reader, author, expected_records).await
        }
        TimelineOperation::CacheRefreshLatest => {
            refresh_cache_first_author(reader, author, expected_records, RecordFetch::Latest).await
        }
    }
}

async fn refresh_legacy_author(
    reader: &DaemonHandle,
    author: &AuthorPlan,
    expected_records: usize,
) -> Result<(u64, u64), (u64, u64, String)> {
    let identity = author
        .identity_key()
        .map_err(|error| (0, 0, error.to_string()))?
        .identity_id();
    let records = reader
        .enumerate_append_records(identity.clone(), POSTS_PREFIX.to_string())
        .await
        .map_err(|error| (0, 0, error.to_string()))?;
    if records.len() != expected_records {
        return Err((
            0,
            0,
            format!("record_count:{}_expected:{expected_records}", records.len()),
        ));
    }
    let profile = JoltAddress::new(identity.clone(), "/profile")
        .map_err(|error| (0, 0, error.to_string()))?;
    let resolved_profile = reader
        .resolve(profile.to_string())
        .await
        .map_err(|error| (0, 0, error.to_string()))?;
    let resolves = 1;
    let mut fetches = 0;
    reader
        .fetch(resolved_profile.content_id)
        .await
        .map_err(|error| (resolves, fetches, error.to_string()))?;
    fetches += 1;
    for record in records {
        let fetched = reader
            .fetch(record.content_id.clone())
            .await
            .map_err(|error| (resolves, fetches, error.to_string()))?;
        fetches += 1;
        if fetched.content_id != record.content_id {
            return Err((resolves, fetches, "fetch_content_mismatch".to_string()));
        }
    }
    Ok((resolves, fetches))
}

async fn refresh_cache_first_author(
    reader: &DaemonHandle,
    author: &AuthorPlan,
    expected_records: usize,
    fetch: RecordFetch,
) -> Result<(u64, u64, Option<MaterializedRecordRefreshOutcome>), (u64, u64, String)> {
    let identity = author
        .identity_key()
        .map_err(|error| (0, 0, error.to_string()))?
        .identity_id();
    let view = reader
        .refresh_materialized_record_view(identity, POSTS_PREFIX.to_string())
        .await
        .map_err(|error| (0, 0, error.to_string()))?;
    validate_record_count(&view.records, expected_records, Some(view.refresh))?;
    let records = match fetch {
        RecordFetch::All => view.records.iter().collect(),
        RecordFetch::Latest => {
            let expected_path = author
                .records
                .get(expected_records.saturating_sub(1))
                .map(|record| &record.path)
                .ok_or_else(|| (0, 0, "newest_planned_record_missing".to_string()))?;
            let latest = view
                .records
                .iter()
                .find(|record| &record.path == expected_path)
                .ok_or_else(|| (0, 0, "newest_materialized_record_missing".to_string()))?;
            vec![latest]
        }
    };
    fetch_materialized_records(reader, records)
        .await
        .map(|(resolves, fetches)| (resolves, fetches, Some(view.refresh)))
}

async fn read_cached_author(
    reader: &DaemonHandle,
    author: &AuthorPlan,
    expected_records: usize,
) -> Result<(u64, u64, Option<MaterializedRecordRefreshOutcome>), (u64, u64, String)> {
    let identity = author
        .identity_key()
        .map_err(|error| (0, 0, error.to_string()))?
        .identity_id();
    let snapshot = reader
        .read_materialized_record_snapshot(identity, POSTS_PREFIX.to_string())
        .await
        .map_err(|error| (0, 0, error.to_string()))?;
    validate_record_count(&snapshot.records, expected_records, None)?;
    fetch_materialized_records(reader, snapshot.records.iter().collect())
        .await
        .map(|(resolves, fetches)| (resolves, fetches, None))
}

fn validate_record_count(
    records: &[MaterializedRecordInfo],
    expected_records: usize,
    refresh: Option<MaterializedRecordRefreshOutcome>,
) -> Result<(), (u64, u64, String)> {
    if records.len() == expected_records {
        return Ok(());
    }
    let refresh = refresh
        .map(|outcome| format!("_refresh:{outcome:?}"))
        .unwrap_or_default();
    Err((
        0,
        0,
        format!(
            "record_count:{}_expected:{expected_records}{refresh}",
            records.len()
        ),
    ))
}

async fn fetch_materialized_records(
    reader: &DaemonHandle,
    records: Vec<&MaterializedRecordInfo>,
) -> Result<(u64, u64), (u64, u64, String)> {
    let mut fetches = 0;
    for record in records {
        let fetched = reader
            .fetch(record.content_id.clone())
            .await
            .map_err(|error| (0, fetches, error.to_string()))?;
        fetches += 1;
        if fetched.content_id != record.content_id {
            return Err((0, fetches, "fetch_content_mismatch".to_string()));
        }
    }
    Ok((0, fetches))
}

fn classify_error(error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    if lower.contains("timeout") {
        "timeout".to_string()
    } else if lower.contains("record_count") {
        error.to_string()
    } else if lower.contains("no peer") || lower.contains("not connected") {
        "unreachable".to_string()
    } else {
        error.chars().take(160).collect()
    }
}

fn snapshot_links(links: &[ShapedLink]) -> NetworkBytes {
    links
        .iter()
        .fold(NetworkBytes::default(), |mut total, link| {
            let snapshot = link.snapshot();
            total.reader_to_providers += snapshot.reader_to_providers;
            total.providers_to_reader += snapshot.providers_to_reader;
            total.dropped += snapshot.dropped;
            total
        })
}

async fn wait_for_link_quiescence(links: &[ShapedLink]) {
    const QUIET_FOR: Duration = Duration::from_millis(50);
    const GIVE_UP_AFTER: Duration = Duration::from_secs(2);
    const SAMPLE_EVERY: Duration = Duration::from_millis(10);

    let deadline = Instant::now() + GIVE_UP_AFTER;
    let mut last_activity = Instant::now();
    let mut previous = snapshot_links(links);
    loop {
        tokio::time::sleep(SAMPLE_EVERY).await;
        let current = snapshot_links(links);
        if current != previous {
            previous = current;
            last_activity = Instant::now();
        }
        if last_activity.elapsed() >= QUIET_FOR || Instant::now() >= deadline {
            return;
        }
    }
}

async fn cache_report(handle: &DaemonHandle) -> anyhow::Result<CacheReport> {
    let stats = handle.cache_stats().await?;
    Ok(CacheReport {
        cached_items: stats.cached_items,
        cached_bytes: stats.total_cached,
        published_items: stats.published_items,
        published_bytes: stats.total_published,
    })
}

fn process_report(system: &mut System) -> anyhow::Result<ProcessReport> {
    let pid = Pid::from_u32(std::process::id());
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing()
            .with_memory()
            .with_cpu()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .without_tasks(),
    );
    let process = system
        .process(pid)
        .context("benchmark process disappeared")?;
    Ok(ProcessReport {
        rss_bytes: process.memory(),
        virtual_memory_bytes: process.virtual_memory(),
        accumulated_cpu_millis: process.accumulated_cpu_time(),
    })
}

fn environment_report() -> EnvironmentReport {
    let system = System::new_all();
    EnvironmentReport {
        os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.to_string()),
        architecture: std::env::consts::ARCH.to_string(),
        logical_cpus: system.cpus().len(),
        total_memory_bytes: system.total_memory(),
        jolt_version: env!("CARGO_PKG_VERSION").to_string(),
        transport: "libp2p TCP through local shaped links".to_string(),
        process_model: "multiple real NetworkNode daemon loops in one process".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, time::Duration};

    use tempfile::tempdir;

    use super::{poll_until_visible, run, NetworkProfile, RunConfig, TimelinePath};
    use crate::WorkloadConfig;

    #[tokio::test]
    async fn cache_first_warm_open_is_a_local_read() {
        let directory = tempdir().unwrap();
        let report = run(
            RunConfig {
                workload: WorkloadConfig {
                    seed: 7,
                    daemon_count: 2,
                    identity_count: 1,
                    follow_count: 1,
                    records_per_identity: 1,
                    churn_percent: 0,
                },
                timeline_path: TimelinePath::CacheFirst,
                publish_rate_per_second: 0,
                concurrency: 1,
                churn_duration_ms: 0,
                provider_record_capacity: 100,
                reader_cache_max_bytes: 2 * 1024 * 1024 * 1024,
                network: NetworkProfile {
                    one_way_latency_ms: 0,
                    bandwidth_kbps: 0,
                    loss_percent: 0,
                },
            },
            directory.path(),
        )
        .await
        .unwrap();

        let cold = &report.phases[0];
        assert_eq!(cold.timeline_latency.successes, 1);
        assert_eq!(cold.activity.identity_sync_requests, 1);

        let warm = &report.phases[1];
        assert_eq!(warm.name, "warm_no_change");
        assert_eq!(warm.timeline_latency.successes, 1);
        assert_eq!(warm.activity.identity_sync_requests, 0);
        assert_eq!(warm.network_bytes, Default::default());

        let active = &report.phases[2];
        assert!(active.daemon_api_latency.successes > 0);
        assert_eq!(active.sync_work.queue_capacity, 64);
        assert!(active.sync_work.delta_responses >= 1);

        assert_eq!(report.propagation.visible_records, 1);
        assert!(report.restart_startup_micros > 0);
        let restarted = &report.phases[3];
        assert_eq!(restarted.name, "restart_warm");
        assert_eq!(restarted.timeline_latency.successes, 1);
        assert_eq!(restarted.activity.identity_sync_requests, 0);
        assert_eq!(restarted.network_bytes, Default::default());
    }

    #[tokio::test]
    async fn cache_first_keeps_cached_posts_readable_during_provider_churn() {
        let directory = tempdir().unwrap();
        let report = run(
            RunConfig {
                workload: WorkloadConfig {
                    seed: 11,
                    daemon_count: 3,
                    identity_count: 2,
                    follow_count: 2,
                    records_per_identity: 1,
                    churn_percent: 50,
                },
                timeline_path: TimelinePath::CacheFirst,
                publish_rate_per_second: 0,
                concurrency: 2,
                churn_duration_ms: 50,
                provider_record_capacity: 100,
                reader_cache_max_bytes: 2 * 1024 * 1024 * 1024,
                network: NetworkProfile {
                    one_way_latency_ms: 0,
                    bandwidth_kbps: 0,
                    loss_percent: 0,
                },
            },
            directory.path(),
        )
        .await
        .unwrap();

        let offline_warm = report
            .phases
            .iter()
            .find(|phase| phase.name == "offline_warm")
            .unwrap();
        assert_eq!(offline_warm.timeline_latency.successes, 2);
        assert_eq!(offline_warm.activity.identity_sync_requests, 0);
        assert_eq!(offline_warm.network_bytes, Default::default());

        let offline_refresh = report
            .phases
            .iter()
            .find(|phase| phase.name == "offline_refresh")
            .unwrap();
        assert!(offline_refresh.activity.refresh_network_unavailable >= 1);

        let recovered = report
            .phases
            .iter()
            .find(|phase| phase.name == "churn_recovery")
            .unwrap();
        assert_eq!(recovered.activity.refresh_ready, 2);
    }

    #[tokio::test]
    async fn cache_pressure_is_visible_as_warm_network_work_without_losing_the_view() {
        let directory = tempdir().unwrap();
        let report = run(
            RunConfig {
                workload: WorkloadConfig {
                    seed: 13,
                    daemon_count: 2,
                    identity_count: 1,
                    follow_count: 1,
                    records_per_identity: 1,
                    churn_percent: 0,
                },
                timeline_path: TimelinePath::CacheFirst,
                publish_rate_per_second: 0,
                concurrency: 1,
                churn_duration_ms: 0,
                provider_record_capacity: 100,
                reader_cache_max_bytes: 1,
                network: NetworkProfile {
                    one_way_latency_ms: 0,
                    bandwidth_kbps: 0,
                    loss_percent: 0,
                },
            },
            directory.path(),
        )
        .await
        .unwrap();

        let warm = &report.phases[1];
        assert_eq!(warm.timeline_latency.successes, 1);
        assert_eq!(warm.activity.identity_sync_requests, 0);
        assert!(warm.network_bytes.providers_to_reader > 0);
        assert_eq!(warm.cache_after.cached_bytes, 0);
    }

    #[tokio::test]
    async fn legacy_refresh_mode_remains_available_for_baseline_reproduction() {
        let directory = tempdir().unwrap();
        let report = run(
            RunConfig {
                workload: WorkloadConfig {
                    seed: 17,
                    daemon_count: 2,
                    identity_count: 1,
                    follow_count: 1,
                    records_per_identity: 1,
                    churn_percent: 0,
                },
                timeline_path: TimelinePath::LegacyRefresh,
                publish_rate_per_second: 0,
                concurrency: 1,
                churn_duration_ms: 0,
                provider_record_capacity: 100,
                reader_cache_max_bytes: 2 * 1024 * 1024 * 1024,
                network: NetworkProfile {
                    one_way_latency_ms: 0,
                    bandwidth_kbps: 0,
                    loss_percent: 0,
                },
            },
            directory.path(),
        )
        .await
        .unwrap();

        assert_eq!(report.config.timeline_path, TimelinePath::LegacyRefresh);
        let warm = &report.phases[1];
        assert_eq!(warm.activity.identity_sync_requests, 1);
        assert_eq!(warm.activity.resolves, 1);
        assert_eq!(warm.activity.fetches, 2);
        assert!(warm.network_bytes.reader_to_providers > 0);
    }

    #[tokio::test]
    async fn visibility_poll_retries_old_counts_until_the_record_is_visible() {
        let attempts = Cell::new(0_u64);

        let outcome = poll_until_visible(Duration::from_millis(50), Duration::ZERO, || {
            let attempt = attempts.get() + 1;
            attempts.set(attempt);
            async move {
                if attempt < 3 {
                    Err((0, 0, "record_count:1_expected:2".to_string()))
                } else {
                    Ok((1, 2))
                }
            }
        })
        .await;

        assert_eq!(outcome.result, Ok((1, 2)));
        assert_eq!(outcome.attempts, 3);
        assert!(outcome.first_attempt_missed);
    }

    #[tokio::test]
    async fn visibility_poll_stops_at_its_deadline() {
        let attempts = Cell::new(0_u64);

        let outcome =
            poll_until_visible(Duration::from_millis(5), Duration::from_millis(1), || {
                attempts.set(attempts.get() + 1);
                async { Err::<(), _>((0, 0, "record_count:1_expected:2".to_string())) }
            })
            .await;

        assert!(outcome.result.is_err());
        assert!(outcome.attempts >= 1);
        assert!(outcome.first_attempt_missed);
    }
}
