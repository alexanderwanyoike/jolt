use super::*;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkProfile {
    pub one_way_latency_ms: u64,
    pub bandwidth_kbps: u64,
    pub loss_percent: u8,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkBytes {
    pub reader_to_providers: u64,
    pub providers_to_reader: u64,
    pub dropped: u64,
}

impl NetworkBytes {
    pub(super) fn difference(&self, before: &Self) -> Self {
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

pub(super) struct ShapedLink {
    pub(super) port: u16,
    counters: LinkCounters,
    offline: watch::Sender<bool>,
    accept_task: JoinHandle<()>,
}

impl ShapedLink {
    pub(super) async fn start(
        upstream: SocketAddr,
        profile: NetworkProfile,
    ) -> anyhow::Result<Self> {
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

    pub(super) fn set_offline(&self, offline: bool) {
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

pub(super) fn snapshot_links(links: &[ShapedLink]) -> NetworkBytes {
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

pub(super) async fn wait_for_link_quiescence(links: &[ShapedLink]) {
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
