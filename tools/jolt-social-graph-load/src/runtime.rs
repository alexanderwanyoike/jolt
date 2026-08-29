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
use jolt_core::{
    ContentId, DeviceAuthorizationOperation, DeviceAuthorizationRecord, DeviceWriterLogEntry,
    DeviceWriterOperation, DeviceWriterPathMode, JoltAddress, UpdateAction, UpdateLogEntry,
};
use jolt_identity::NodeIdentity;
use jolt_network::{DaemonCommand, DaemonHandle, NetworkConfig, NetworkNode};
use jolt_store::{CacheConfig, ContentStore};
use libp2p::multiaddr::Protocol;
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, Semaphore},
    task::{JoinHandle, JoinSet},
};

use crate::{
    deterministic_bytes, AuthorPlan, PhaseAccounting, PhaseSummary, RecordPlan, WorkloadConfig,
    WorkloadPlan,
};

const POSTS_PREFIX: &str = "/spoke/posts/";
const RESULT_VERSION: u32 = 1;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkProfile {
    pub one_way_latency_ms: u64,
    pub bandwidth_kbps: u64,
    pub loss_percent: u8,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunConfig {
    pub workload: WorkloadConfig,
    pub publish_rate_per_second: u64,
    pub concurrency: usize,
    pub churn_duration_ms: u64,
    /// Zero preserves libp2p's production default.
    pub provider_record_capacity: usize,
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
    pub cache_before: CacheReport,
    pub cache_after: CacheReport,
    pub process_before: ProcessReport,
    pub process_after: ProcessReport,
    pub cpu_millis: u64,
    pub rss_growth_bytes: i64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PropagationReport {
    pub published_records: u64,
    pub visible_records: u64,
    pub failed_records: u64,
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
    offline: Arc<AtomicBool>,
    accept_task: JoinHandle<()>,
}

impl ShapedLink {
    async fn start(upstream: SocketAddr, profile: NetworkProfile) -> anyhow::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        let counters = LinkCounters::new();
        let offline = Arc::new(AtomicBool::new(false));
        let task_counters = counters.clone();
        let task_offline = offline.clone();
        let accept_task = tokio::spawn(async move {
            while let Ok((downstream, _)) = listener.accept().await {
                let counters = task_counters.clone();
                let offline = task_offline.clone();
                let profile = profile.clone();
                tokio::spawn(async move {
                    if offline.load(Ordering::Relaxed) {
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
        self.offline.store(offline, Ordering::Relaxed);
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
    offline: Arc<AtomicBool>,
    profile: NetworkProfile,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        if offline.load(Ordering::Relaxed) {
            break;
        }
        let Ok(read) = reader.read(&mut buffer).await else {
            break;
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
    async fn shutdown(self) {
        let _ = self.handle.shutdown().await;
        let _ = self.task.await;
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
}

struct AuthorOutcome {
    published_at: Option<Instant>,
    completed_at: Instant,
    latency_micros: u64,
    resolves: u64,
    fetches: u64,
    error: Option<String>,
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
        "reader",
        plan.provider_count,
        config.provider_record_capacity,
    )
    .await?;
    for (provider, link) in providers.iter().zip(&links) {
        reader
            .handle
            .connect_peer(format!(
                "/ip4/127.0.0.1/tcp/{}/p2p/{}",
                link.port, provider.peer_id
            ))
            .await
            .with_context(|| format!("connect reader to provider {}", provider.peer_id))?;
    }
    wait_for_peer_count(&reader.handle, providers.len()).await?;

    let mut author_states = seed_authors(&plan, &providers, workdir).await?;
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
        concurrency: config.concurrency,
        links: &links,
        system: &mut system,
    };
    let mut phases = Vec::new();
    phases.push(
        measurement
            .measure("cold", config.workload.records_per_identity)
            .await?
            .report,
    );
    phases.push(
        measurement
            .measure("warm_no_change", config.workload.records_per_identity)
            .await?
            .report,
    );

    for index in &plan.churned_providers {
        links[*index].set_offline(true);
    }
    if !plan.churned_providers.is_empty() {
        tokio::time::sleep(Duration::from_millis(config.churn_duration_ms)).await;
        for index in &plan.churned_providers {
            links[*index].set_offline(false);
            let provider = &providers[*index];
            let _ = reader
                .handle
                .connect_peer(format!(
                    "/ip4/127.0.0.1/tcp/{}/p2p/{}",
                    links[*index].port, provider.peer_id
                ))
                .await;
        }
        wait_for_peer_count(&reader.handle, providers.len()).await?;
    }
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
    let visible = measured_new_phase.visibility.successes;
    let published_records = plan.followed_authors.len() as u64;
    let propagation = PropagationReport {
        published_records,
        visible_records: visible,
        failed_records: published_records.saturating_sub(visible),
        latency_micros: measured_new_phase.visibility.latency_micros,
    };
    phases.push(new_phase);

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
        final_network_bytes,
        limitations,
    })
}

async fn start_daemon(
    workdir: &Path,
    seed: u64,
    domain: &str,
    index: usize,
    provider_record_capacity: usize,
) -> anyhow::Result<RunningDaemon> {
    let root = workdir.join(format!("{domain}-{index}"));
    std::fs::create_dir_all(&root)?;
    let identity = NodeIdentity::from_signing_key_bytes(&deterministic_bytes(seed, domain, index))
        .map_err(|error| anyhow::anyhow!("create {domain} identity {index}: {error}"))?;
    let identity_id = identity.identity_id().to_string();
    let store = ContentStore::open(&root, CacheConfig::default())?;
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
    node.listen_on("/ip4/127.0.0.1/tcp/0")?;
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

async fn seed_authors(
    plan: &WorkloadPlan,
    providers: &[RunningDaemon],
    workdir: &Path,
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
                DeviceWriterPathMode::Append,
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
            DeviceWriterPathMode::Append,
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
}

struct MeasurementContext<'a> {
    reader: &'a DaemonHandle,
    plan: &'a WorkloadPlan,
    concurrency: usize,
    links: &'a [ShapedLink],
    system: &'a mut System,
}

impl MeasurementContext<'_> {
    async fn measure(
        &mut self,
        name: &str,
        expected_records: usize,
    ) -> anyhow::Result<MeasuredPhase> {
        let network_before = snapshot_links(self.links);
        let cache_before = cache_report(self.reader).await?;
        let process_before = process_report(self.system)?;
        let started = Instant::now();
        let outcome =
            run_timeline(self.reader, self.plan, expected_records, self.concurrency).await;
        let wall_time_micros = started.elapsed().as_micros() as u64;
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
                cache_before,
                cache_after,
                process_before,
                process_after,
                cpu_millis,
                rss_growth_bytes: rss_growth_bytes.clamp(i64::MIN as i128, i64::MAX as i128) as i64,
            },
            visibility: outcome.visibility.summarize(),
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
        let started = Instant::now();
        let outcome = publish_and_refresh_new_records(
            states,
            self.plan,
            providers,
            workdir,
            publish_rate_per_second,
            self.reader,
            self.concurrency,
        )
        .await?;
        let wall_time_micros = started.elapsed().as_micros() as u64;
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
                cache_before,
                cache_after,
                process_before,
                process_after,
                cpu_millis,
                rss_growth_bytes: rss_growth_bytes.clamp(i64::MIN as i128, i64::MAX as i128) as i64,
            },
            visibility: outcome.visibility.summarize(),
        })
    }
}

async fn run_timeline(
    reader: &DaemonHandle,
    plan: &WorkloadPlan,
    expected_records: usize,
    concurrency: usize,
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
) {
    tasks.spawn(async move {
        let permit = semaphore.acquire_owned().await;
        let started = Instant::now();
        let result = match permit {
            Ok(_permit) => refresh_author(&reader, &author, expected_records).await,
            Err(error) => Err((0, 0, error.to_string())),
        };
        match result {
            Ok((resolves, fetches)) => AuthorOutcome {
                published_at,
                completed_at: Instant::now(),
                latency_micros: started.elapsed().as_micros() as u64,
                resolves,
                fetches,
                error: None,
            },
            Err((resolves, fetches, error)) => AuthorOutcome {
                published_at,
                completed_at: Instant::now(),
                latency_micros: started.elapsed().as_micros() as u64,
                resolves,
                fetches,
                error: Some(error),
            },
        }
    });
}

async fn collect_author_outcomes(mut tasks: JoinSet<AuthorOutcome>) -> TimelineOutcome {
    let mut outcome = TimelineOutcome::default();
    while let Some(result) = tasks.join_next().await {
        outcome.activity.identity_sync_requests += 1;
        match result {
            Ok(author) => {
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
                match author.error {
                    Some(error) => outcome
                        .accounting
                        .record_failure(author.latency_micros, classify_error(&error)),
                    None => outcome.accounting.record_success(author.latency_micros),
                }
            }
            Err(error) => outcome
                .accounting
                .record_failure(0, format!("task:{error}")),
        }
    }
    outcome
}

async fn refresh_author(
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
