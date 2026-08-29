use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        atomic::{AtomicU64, Ordering},
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

mod daemons;
mod shaping;

use daemons::{
    connect_reader_to_providers, listener_socket, start_daemon, wait_for_peer_absence, DaemonSpec,
    RunningDaemon,
};
use shaping::{snapshot_links, wait_for_link_quiescence, ShapedLink};
pub use shaping::{NetworkBytes, NetworkProfile};

const POSTS_PREFIX: &str = "/spoke/posts/";
const RESULT_VERSION: u32 = 4;
const STATUS_SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
const VISIBILITY_POLL_DEADLINE: Duration = Duration::from_secs(75);
const VISIBILITY_POLL_INTERVAL: Duration = Duration::from_millis(100);

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

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiskReport {
    pub files: u64,
    pub bytes: u64,
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
    pub reader_disk_before: DiskReport,
    pub reader_disk_after: DiskReport,
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

#[derive(Debug, Default, Eq, PartialEq)]
struct ReadOutcome {
    resolves: u64,
    fetches: u64,
    refresh: Option<MaterializedRecordRefreshOutcome>,
}

#[derive(Debug, Eq, PartialEq)]
struct ReadFailure {
    resolves: u64,
    fetches: u64,
    reason: String,
}

impl ReadFailure {
    fn new(reason: impl ToString) -> Self {
        Self {
            resolves: 0,
            fetches: 0,
            reason: reason.to_string(),
        }
    }

    fn after_activity(resolves: u64, fetches: u64, reason: impl ToString) -> Self {
        Self {
            resolves,
            fetches,
            reason: reason.to_string(),
        }
    }
}

type ReadResult = Result<ReadOutcome, ReadFailure>;

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
                DaemonSpec {
                    domain: "provider",
                    index,
                    provider_record_capacity: config.provider_record_capacity,
                    cache_max_bytes: CacheConfig::default().max_size_bytes,
                    listen_port: 0,
                },
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
    let reader = start_daemon(
        workdir,
        config.workload.seed,
        DaemonSpec {
            domain: "reader",
            index: plan.provider_count,
            provider_record_capacity: config.provider_record_capacity,
            cache_max_bytes: config.reader_cache_max_bytes,
            listen_port: 0,
        },
    )
    .await?;
    connect_reader_to_providers(&reader.handle, &providers, &links).await?;

    let mut author_states = seed_authors(&plan, &providers, workdir, config.timeline_path).await?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    let setup_activity = ActivityReport {
        provider_announcements: plan.authors.len() as u64,
        content_announcements: plan.record_count() as u64,
        ..ActivityReport::default()
    };

    let mut system = System::new();
    let (mut phases, propagation) = {
        let mut measurement = MeasurementContext {
            reader: &reader.handle,
            plan: &plan,
            workdir,
            timeline_path: config.timeline_path,
            concurrency: config.concurrency,
            links: &links,
            system: &mut system,
        };
        let mut phases = Vec::new();
        phases.push(
            measurement
                .measure_named(
                    "cold",
                    config.workload.records_per_identity,
                    TimelineOperation::cold(config.timeline_path),
                )
                .await?,
        );
        wait_for_link_quiescence(&links).await;
        phases.push(
            measurement
                .measure_named(
                    "warm_no_change",
                    config.workload.records_per_identity,
                    TimelineOperation::warm(config.timeline_path),
                )
                .await?,
        );

        let (active, propagation) = measurement
            .measure_active_records(
                &mut author_states,
                &providers,
                config.publish_rate_per_second,
            )
            .await?;
        phases.push(active);

        if !plan.churned_providers.is_empty() && config.timeline_path == TimelinePath::CacheFirst {
            phases.extend(run_churn_phases(&mut measurement, &mut providers, &config).await?);
        }
        (phases, propagation)
    };

    let restart = run_restart_phase(
        RestartPhaseSpec {
            workdir,
            config: &config,
            plan: &plan,
            providers: &providers,
            links: &links,
        },
        reader,
        &mut system,
    )
    .await?;
    phases.push(restart.phase);
    let reader = restart.reader;
    let restart_startup_micros = restart.startup_micros;

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
        "Active visibility polling calls the daemon refresh command directly and bypasses the App API's refresh cooldown, so identity sync request counts represent a deliberately aggressive daemon workload rather than normal Spoke polling.".to_string(),
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

struct RestartPhaseSpec<'a> {
    workdir: &'a Path,
    config: &'a RunConfig,
    plan: &'a WorkloadPlan,
    providers: &'a [RunningDaemon],
    links: &'a [ShapedLink],
}

struct RestartPhaseResult {
    reader: RunningDaemon,
    phase: PhaseReport,
    startup_micros: u64,
}

async fn run_restart_phase(
    spec: RestartPhaseSpec<'_>,
    reader: RunningDaemon,
    system: &mut System,
) -> anyhow::Result<RestartPhaseResult> {
    reader.shutdown().await;
    let started = Instant::now();
    let reader = start_daemon(
        spec.workdir,
        spec.config.workload.seed,
        DaemonSpec {
            domain: "reader",
            index: spec.plan.provider_count,
            provider_record_capacity: spec.config.provider_record_capacity,
            cache_max_bytes: spec.config.reader_cache_max_bytes,
            listen_port: 0,
        },
    )
    .await?;
    let startup_micros = started.elapsed().as_micros() as u64;
    connect_reader_to_providers(&reader.handle, spec.providers, spec.links).await?;
    wait_for_link_quiescence(spec.links).await;
    let phase = MeasurementContext {
        reader: &reader.handle,
        plan: spec.plan,
        workdir: spec.workdir,
        timeline_path: spec.config.timeline_path,
        concurrency: spec.config.concurrency,
        links: spec.links,
        system,
    }
    .measure_named(
        "restart_warm",
        spec.config.workload.records_per_identity + 1,
        TimelineOperation::warm(spec.config.timeline_path),
    )
    .await?;

    Ok(RestartPhaseResult {
        reader,
        phase,
        startup_micros,
    })
}

async fn run_churn_phases(
    measurement: &mut MeasurementContext<'_>,
    providers: &mut [RunningDaemon],
    config: &RunConfig,
) -> anyhow::Result<Vec<PhaseReport>> {
    let stopped_providers = stop_churned_providers(measurement, providers).await?;
    tokio::time::sleep(Duration::from_millis(config.churn_duration_ms)).await;
    wait_for_link_quiescence(measurement.links).await;

    let expected_records = config.workload.records_per_identity + 1;
    let mut offline_warm = measurement
        .measure_named(
            "offline_warm",
            expected_records,
            TimelineOperation::CacheReadAll,
        )
        .await?;
    offline_warm.activity.churn_events = measurement.plan.churned_providers.len() as u64;
    let offline_refresh = measurement
        .measure_named(
            "offline_refresh",
            expected_records,
            TimelineOperation::CacheRefreshAll,
        )
        .await?;

    restart_churned_providers(measurement, providers, stopped_providers, config).await?;
    let recovered = measurement
        .measure_named(
            "churn_recovery",
            expected_records,
            TimelineOperation::CacheRefreshAll,
        )
        .await?;

    Ok(vec![offline_warm, offline_refresh, recovered])
}

async fn stop_churned_providers(
    measurement: &MeasurementContext<'_>,
    providers: &mut [RunningDaemon],
) -> anyhow::Result<Vec<(usize, u16, String)>> {
    let mut stopped = Vec::with_capacity(measurement.plan.churned_providers.len());
    for index in &measurement.plan.churned_providers {
        measurement.links[*index].set_offline(true);
        let port = listener_socket(&providers[*index].listen_addr)?.port();
        let peer_id = providers[*index].peer_id.clone();
        providers[*index].stop().await;
        stopped.push((*index, port, peer_id));
    }
    wait_for_peer_absence(
        measurement.reader,
        &stopped
            .iter()
            .map(|(_, _, peer_id)| peer_id.clone())
            .collect::<Vec<_>>(),
    )
    .await?;
    Ok(stopped)
}

async fn restart_churned_providers(
    measurement: &MeasurementContext<'_>,
    providers: &mut [RunningDaemon],
    stopped: Vec<(usize, u16, String)>,
    config: &RunConfig,
) -> anyhow::Result<()> {
    for (index, port, _) in stopped {
        providers[index] = start_daemon(
            measurement.workdir,
            config.workload.seed,
            DaemonSpec {
                domain: "provider",
                index,
                provider_record_capacity: config.provider_record_capacity,
                cache_max_bytes: CacheConfig::default().max_size_bytes,
                listen_port: port,
            },
        )
        .await?;
        measurement.links[index].set_offline(false);
    }
    connect_reader_to_providers(measurement.reader, providers, measurement.links).await
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

struct ActiveRefreshContext<'a> {
    plan: &'a WorkloadPlan,
    providers: &'a [RunningDaemon],
    workdir: &'a Path,
    reader: &'a DaemonHandle,
    concurrency: usize,
    timeline_path: TimelinePath,
}

async fn publish_and_refresh_new_records(
    states: &mut [AuthorState],
    context: ActiveRefreshContext<'_>,
    rate: u64,
) -> anyhow::Result<TimelineOutcome> {
    let semaphore = Arc::new(Semaphore::new(context.concurrency));
    let mut tasks = JoinSet::new();
    let delay = (rate > 0).then(|| Duration::from_secs_f64(1.0 / rate as f64));
    for author_index in &context.plan.followed_authors {
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
                "seed": context.plan.seed,
                "text": format!("new deterministic post from author {}", state.plan.index),
            })
            .to_string(),
        };
        let provider = &context.providers[state.plan.provider_index];
        let content = publish_record(provider, context.workdir, state.plan.index, &record).await?;
        let owner = state.plan.identity_key()?;
        let operation = DeviceWriterOperation::set_path(
            record.path.clone(),
            content,
            writer_path_mode(context.timeline_path),
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
            context.reader.clone(),
            state.plan.clone(),
            state.plan.records.len(),
            Some(Instant::now()),
            TimelineOperation::active(context.timeline_path),
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

async fn sample_daemon_status(
    handle: DaemonHandle,
    mut stop: watch::Receiver<bool>,
) -> StatusSampling {
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
        if *stop.borrow() {
            return sampling;
        }
        tokio::select! {
            _ = tokio::time::sleep(STATUS_SAMPLE_INTERVAL) => {}
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    return sampling;
                }
            }
        }
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

struct PhaseCapture {
    network_before: NetworkBytes,
    cache_before: CacheReport,
    process_before: ProcessReport,
    reader_disk_before: DiskReport,
    sync_before: NodeStatus,
    stop_sampling: watch::Sender<bool>,
    status_task: JoinHandle<StatusSampling>,
    started: Instant,
}

impl PhaseCapture {
    async fn begin(context: &mut MeasurementContext<'_>) -> anyhow::Result<Self> {
        let network_before = snapshot_links(context.links);
        let cache_before = cache_report(context.reader).await?;
        let process_before = process_report(context.system)?;
        let reader_disk_before = disk_report(&reader_store_path(context.workdir, context.plan))?;
        let sync_before = context.reader.status().await?;
        let (stop_sampling, stop_signal) = watch::channel(false);
        let status_task = tokio::spawn(sample_daemon_status(context.reader.clone(), stop_signal));
        tokio::task::yield_now().await;

        Ok(Self {
            network_before,
            cache_before,
            process_before,
            reader_disk_before,
            sync_before,
            stop_sampling,
            status_task,
            started: Instant::now(),
        })
    }

    async fn finish(
        self,
        context: &mut MeasurementContext<'_>,
        name: &str,
        outcome: TimelineOutcome,
    ) -> anyhow::Result<MeasuredPhase> {
        let wall_time_micros = self.started.elapsed().as_micros() as u64;
        self.stop_sampling.send_replace(true);
        let status_sampling = self
            .status_task
            .await
            .context("status sampler task failed")?;
        let sync_after = context.reader.status().await?;
        let process_after = process_report(context.system)?;
        let cache_after = cache_report(context.reader).await?;
        let reader_disk_after = disk_report(&reader_store_path(context.workdir, context.plan))?;
        let network_after = snapshot_links(context.links);
        let cpu_millis = process_after
            .accumulated_cpu_millis
            .saturating_sub(self.process_before.accumulated_cpu_millis);
        let rss_growth_bytes =
            process_after.rss_bytes as i128 - self.process_before.rss_bytes as i128;

        Ok(MeasuredPhase {
            report: PhaseReport {
                name: name.to_string(),
                wall_time_micros,
                timeline_latency: outcome.accounting.summarize(),
                network_bytes: network_after.difference(&self.network_before),
                activity: outcome.activity,
                daemon_api_latency: status_sampling.accounting.summarize(),
                sync_work: sync_work_report(&self.sync_before, &sync_after, &status_sampling),
                cache_before: self.cache_before,
                cache_after,
                process_before: self.process_before,
                process_after,
                reader_disk_before: self.reader_disk_before,
                reader_disk_after,
                cpu_millis,
                rss_growth_bytes: rss_growth_bytes.clamp(i64::MIN as i128, i64::MAX as i128) as i64,
            },
            visibility: outcome.visibility.summarize(),
            first_attempt_misses: outcome.first_attempt_misses,
        })
    }
}

struct MeasurementContext<'a> {
    reader: &'a DaemonHandle,
    plan: &'a WorkloadPlan,
    workdir: &'a Path,
    timeline_path: TimelinePath,
    concurrency: usize,
    links: &'a [ShapedLink],
    system: &'a mut System,
}

impl MeasurementContext<'_> {
    async fn measure_named(
        &mut self,
        name: &str,
        expected_records: usize,
        operation: TimelineOperation,
    ) -> anyhow::Result<PhaseReport> {
        let phase = self
            .measure(name, expected_records, operation)
            .await?
            .report;
        print_phase_progress(&phase);
        Ok(phase)
    }

    async fn measure_active_records(
        &mut self,
        states: &mut [AuthorState],
        providers: &[RunningDaemon],
        publish_rate_per_second: u64,
    ) -> anyhow::Result<(PhaseReport, PropagationReport)> {
        let measured = self
            .measure_new_records(states, providers, publish_rate_per_second)
            .await?;
        let mut phase = measured.report;
        phase.activity.content_announcements = self.plan.followed_authors.len() as u64;
        phase.activity.churn_events = self.plan.churned_providers.len() as u64;
        print_phase_progress(&phase);

        let published_records = self.plan.followed_authors.len() as u64;
        let propagation = PropagationReport {
            published_records,
            visible_records: measured.visibility.successes,
            failed_records: published_records.saturating_sub(measured.visibility.successes),
            first_attempt_misses: measured.first_attempt_misses,
            latency_micros: measured.visibility.latency_micros,
        };
        Ok((phase, propagation))
    }

    async fn measure(
        &mut self,
        name: &str,
        expected_records: usize,
        operation: TimelineOperation,
    ) -> anyhow::Result<MeasuredPhase> {
        let capture = PhaseCapture::begin(self).await?;
        let outcome = run_timeline(
            self.reader,
            self.plan,
            expected_records,
            self.concurrency,
            operation,
        )
        .await;
        capture.finish(self, name, outcome).await
    }

    async fn measure_new_records(
        &mut self,
        states: &mut [AuthorState],
        providers: &[RunningDaemon],
        publish_rate_per_second: u64,
    ) -> anyhow::Result<MeasuredPhase> {
        let capture = PhaseCapture::begin(self).await?;
        let outcome = publish_and_refresh_new_records(
            states,
            ActiveRefreshContext {
                plan: self.plan,
                providers,
                workdir: self.workdir,
                reader: self.reader,
                concurrency: self.concurrency,
                timeline_path: self.timeline_path,
            },
            publish_rate_per_second,
        )
        .await?;
        capture.finish(self, "new_record_refresh", outcome).await
    }
}

fn disk_report(root: &Path) -> anyhow::Result<DiskReport> {
    let mut report = DiskReport::default();
    let mut directories = vec![PathBuf::from(root)];

    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory)
            .with_context(|| format!("read benchmark directory {}", directory.display()))?
        {
            let entry = entry.with_context(|| {
                format!("read entry in benchmark directory {}", directory.display())
            })?;
            let file_type = entry
                .file_type()
                .with_context(|| format!("inspect benchmark path {}", entry.path().display()))?;
            if file_type.is_dir() {
                directories.push(entry.path());
            } else if file_type.is_file() {
                report.files = report.files.saturating_add(1);
                report.bytes = report.bytes.saturating_add(
                    entry
                        .metadata()
                        .with_context(|| {
                            format!("inspect benchmark file {}", entry.path().display())
                        })?
                        .len(),
                );
            }
        }
    }

    Ok(report)
}

fn reader_store_path(workdir: &Path, plan: &WorkloadPlan) -> PathBuf {
    workdir.join(format!("reader-{}", plan.provider_count))
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
                result: Err(ReadFailure::new(error)),
                attempts: 0,
                first_attempt_missed: false,
            },
        };
        match poll.result {
            Ok(read) => AuthorOutcome {
                published_at,
                completed_at: Instant::now(),
                latency_micros: started.elapsed().as_micros() as u64,
                resolves: read.resolves,
                fetches: read.fetches,
                sync_requests: operation.sync_requests(poll.attempts),
                refresh: read.refresh,
                first_attempt_missed: poll.first_attempt_missed,
                error: None,
            },
            Err(failure) => AuthorOutcome {
                published_at,
                completed_at: Instant::now(),
                latency_micros: started.elapsed().as_micros() as u64,
                resolves: failure.resolves,
                fetches: failure.fetches,
                sync_requests: operation.sync_requests(poll.attempts),
                refresh: None,
                first_attempt_missed: poll.first_attempt_missed,
                error: Some(failure.reason),
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

async fn poll_until_visible<F, Fut>(
    max_wait: Duration,
    interval: Duration,
    mut refresh: F,
) -> VisibilityPollOutcome<ReadOutcome, ReadFailure>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ReadResult>,
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
                let old_count = error.reason.starts_with("record_count:");
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
) -> ReadResult {
    match operation {
        TimelineOperation::LegacyRefresh => {
            refresh_legacy_author(reader, author, expected_records).await
        }
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
) -> ReadResult {
    let identity = author
        .identity_key()
        .map_err(ReadFailure::new)?
        .identity_id();
    let records = reader
        .enumerate_append_records(identity.clone(), POSTS_PREFIX.to_string())
        .await
        .map_err(ReadFailure::new)?;
    if records.len() != expected_records {
        return Err(ReadFailure::new(format!(
            "record_count:{}_expected:{expected_records}",
            records.len()
        )));
    }
    let profile = JoltAddress::new(identity.clone(), "/profile").map_err(ReadFailure::new)?;
    let resolved_profile = reader
        .resolve(profile.to_string())
        .await
        .map_err(ReadFailure::new)?;
    let resolves = 1;
    let mut fetches = 0;
    reader
        .fetch(resolved_profile.content_id)
        .await
        .map_err(|error| ReadFailure::after_activity(resolves, fetches, error))?;
    fetches += 1;
    for record in records {
        let fetched = reader
            .fetch(record.content_id.clone())
            .await
            .map_err(|error| ReadFailure::after_activity(resolves, fetches, error))?;
        fetches += 1;
        if fetched.content_id != record.content_id {
            return Err(ReadFailure::after_activity(
                resolves,
                fetches,
                "fetch_content_mismatch",
            ));
        }
    }
    Ok(ReadOutcome {
        resolves,
        fetches,
        refresh: None,
    })
}

async fn refresh_cache_first_author(
    reader: &DaemonHandle,
    author: &AuthorPlan,
    expected_records: usize,
    fetch: RecordFetch,
) -> ReadResult {
    let identity = author
        .identity_key()
        .map_err(ReadFailure::new)?
        .identity_id();
    let view = reader
        .refresh_materialized_record_view(identity, POSTS_PREFIX.to_string())
        .await
        .map_err(ReadFailure::new)?;
    validate_record_count(&view.records, expected_records, Some(view.refresh))?;
    let records = match fetch {
        RecordFetch::All => view.records.iter().collect(),
        RecordFetch::Latest => {
            let expected_path = author
                .records
                .get(expected_records.saturating_sub(1))
                .map(|record| &record.path)
                .ok_or_else(|| ReadFailure::new("newest_planned_record_missing"))?;
            let latest = view
                .records
                .iter()
                .find(|record| &record.path == expected_path)
                .ok_or_else(|| ReadFailure::new("newest_materialized_record_missing"))?;
            vec![latest]
        }
    };
    let mut outcome = fetch_materialized_records(reader, records).await?;
    outcome.refresh = Some(view.refresh);
    Ok(outcome)
}

async fn read_cached_author(
    reader: &DaemonHandle,
    author: &AuthorPlan,
    expected_records: usize,
) -> ReadResult {
    let identity = author
        .identity_key()
        .map_err(ReadFailure::new)?
        .identity_id();
    let snapshot = reader
        .read_materialized_record_snapshot(identity, POSTS_PREFIX.to_string())
        .await
        .map_err(ReadFailure::new)?;
    validate_record_count(&snapshot.records, expected_records, None)?;
    fetch_materialized_records(reader, snapshot.records.iter().collect()).await
}

fn validate_record_count(
    records: &[MaterializedRecordInfo],
    expected_records: usize,
    refresh: Option<MaterializedRecordRefreshOutcome>,
) -> Result<(), ReadFailure> {
    if records.len() == expected_records {
        return Ok(());
    }
    let refresh = refresh
        .map(|outcome| format!("_refresh:{outcome:?}"))
        .unwrap_or_default();
    Err(ReadFailure::new(format!(
        "record_count:{}_expected:{expected_records}{refresh}",
        records.len()
    )))
}

async fn fetch_materialized_records(
    reader: &DaemonHandle,
    records: Vec<&MaterializedRecordInfo>,
) -> ReadResult {
    let mut fetches = 0;
    for record in records {
        let fetched = reader
            .fetch(record.content_id.clone())
            .await
            .map_err(|error| ReadFailure::after_activity(0, fetches, error))?;
        fetches += 1;
        if fetched.content_id != record.content_id {
            return Err(ReadFailure::after_activity(
                0,
                fetches,
                "fetch_content_mismatch",
            ));
        }
    }
    Ok(ReadOutcome {
        resolves: 0,
        fetches,
        refresh: None,
    })
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

    use super::{
        poll_until_visible, run, NetworkProfile, ReadFailure, ReadOutcome, RunConfig, TimelinePath,
    };
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
        assert!(cold.reader_disk_after.bytes > cold.reader_disk_before.bytes);

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
                    Err(ReadFailure::new("record_count:1_expected:2"))
                } else {
                    Ok(ReadOutcome {
                        resolves: 1,
                        fetches: 2,
                        refresh: None,
                    })
                }
            }
        })
        .await;

        assert_eq!(
            outcome.result,
            Ok(ReadOutcome {
                resolves: 1,
                fetches: 2,
                refresh: None,
            })
        );
        assert_eq!(outcome.attempts, 3);
        assert!(outcome.first_attempt_missed);
    }

    #[tokio::test]
    async fn visibility_poll_stops_at_its_deadline() {
        let attempts = Cell::new(0_u64);

        let outcome =
            poll_until_visible(Duration::from_millis(5), Duration::from_millis(1), || {
                attempts.set(attempts.get() + 1);
                async { Err(ReadFailure::new("record_count:1_expected:2")) }
            })
            .await;

        assert!(outcome.result.is_err());
        assert!(outcome.attempts >= 1);
        assert!(outcome.first_attempt_missed);
    }
}
