use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use jolt_network::MaterializedRecordInfo;
use serde::Serialize;
use tokio::sync::{Mutex, Notify};

use crate::session_store::DataSubscriptionRefresh;

const DEFAULT_CHANGE_JOURNAL_CAPACITY: usize = 64;
const DEFAULT_CHANGE_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const CHANGE_LONG_POLL_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DataChangeEvent {
    Snapshot {
        cursor: String,
        identity: String,
        records: Vec<MaterializedRecordInfo>,
        state: DataSubscriptionRefresh,
    },
    Changed {
        cursor: String,
        identity: String,
        records: Vec<MaterializedRecordInfo>,
        removed: Vec<String>,
    },
    State {
        cursor: String,
        state: DataSubscriptionRefresh,
    },
    Timeout {
        cursor: String,
    },
    ResyncRequired,
    Cancelled,
    Revoked,
}

#[derive(Debug, Clone)]
pub struct DataChangeStreams {
    generation: Arc<str>,
    capacity: usize,
    idle_timeout: Duration,
    registry: Arc<Mutex<StreamRegistry>>,
}

#[derive(Debug, Default)]
struct StreamRegistry {
    active: HashMap<String, StreamState>,
    terminal: HashMap<String, TerminalTombstone>,
}

#[derive(Debug)]
struct StreamState {
    session_id: String,
    identity: String,
    records: BTreeMap<String, MaterializedRecordInfo>,
    refresh: DataSubscriptionRefresh,
    sequence: u64,
    journal: VecDeque<SequencedEvent>,
    notify: Arc<Notify>,
    last_polled_at: Instant,
}

#[derive(Debug)]
struct TerminalTombstone {
    session_id: String,
    event: DataChangeEvent,
    expires_at: Instant,
}

#[derive(Debug)]
struct SequencedEvent {
    sequence: u64,
    event: DataChangeEvent,
}

impl DataChangeStreams {
    pub fn new() -> Self {
        let generation = format!("stream_{}", hex::encode(rand::random::<[u8; 16]>()));
        Self::with_generation_and_capacity(generation, DEFAULT_CHANGE_JOURNAL_CAPACITY)
    }

    pub fn for_refresh_interval(refresh_interval: Duration) -> Self {
        let refresh_interval = refresh_interval.max(Duration::from_secs(1));
        let generation = format!("stream_{}", hex::encode(rand::random::<[u8; 16]>()));
        Self::with_generation_capacity_and_idle_timeout(
            generation,
            DEFAULT_CHANGE_JOURNAL_CAPACITY,
            refresh_interval.saturating_mul(3),
        )
    }

    fn with_generation_and_capacity(generation: impl Into<String>, capacity: usize) -> Self {
        Self::with_generation_capacity_and_idle_timeout(
            generation,
            capacity,
            DEFAULT_CHANGE_STREAM_IDLE_TIMEOUT,
        )
    }

    fn with_generation_capacity_and_idle_timeout(
        generation: impl Into<String>,
        capacity: usize,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            generation: Arc::from(generation.into()),
            capacity: capacity.max(1),
            idle_timeout,
            registry: Arc::new(Mutex::new(StreamRegistry::default())),
        }
    }

    pub fn start_idle_eviction(&self, sweep_interval: Duration) {
        let registry = Arc::downgrade(&self.registry);
        let idle_timeout = self.idle_timeout;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(sweep_interval).await;
                let Some(registry) = registry.upgrade() else {
                    return;
                };
                evict_idle_registry(&registry, idle_timeout).await;
            }
        });
    }

    pub async fn next(
        &self,
        session_id: &str,
        subscription_id: &str,
        identity: &str,
        records: Vec<MaterializedRecordInfo>,
        refresh: DataSubscriptionRefresh,
        cursor: Option<&str>,
    ) -> DataChangeEvent {
        self.next_with_timeout(
            session_id,
            subscription_id,
            identity,
            records,
            refresh,
            cursor,
            CHANGE_LONG_POLL_TIMEOUT,
        )
        .await
    }

    async fn next_with_timeout(
        &self,
        session_id: &str,
        subscription_id: &str,
        identity: &str,
        records: Vec<MaterializedRecordInfo>,
        refresh: DataSubscriptionRefresh,
        cursor: Option<&str>,
        timeout: Duration,
    ) -> DataChangeEvent {
        let requested_sequence = match cursor {
            None => None,
            Some(cursor) => match self.parse_cursor(cursor) {
                Some(sequence) => Some(sequence),
                None => return DataChangeEvent::ResyncRequired,
            },
        };
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);

        loop {
            let notified = {
                let mut registry = self.registry.lock().await;
                if let Some(terminal) = registry.terminal.get(subscription_id) {
                    return if terminal.session_id == session_id {
                        terminal.event.clone()
                    } else {
                        DataChangeEvent::ResyncRequired
                    };
                }
                let mut created = false;
                let stream = registry
                    .active
                    .entry(subscription_id.to_string())
                    .or_insert_with(|| {
                        created = true;
                        StreamState {
                            session_id: session_id.to_string(),
                            identity: identity.to_string(),
                            records: records
                                .iter()
                                .cloned()
                                .map(|record| (record.path.clone(), record))
                                .collect(),
                            refresh: refresh.clone(),
                            sequence: 0,
                            journal: VecDeque::new(),
                            notify: Arc::new(Notify::new()),
                            last_polled_at: Instant::now(),
                        }
                    });
                if stream.session_id != session_id || stream.identity != identity {
                    return DataChangeEvent::ResyncRequired;
                }
                stream.last_polled_at = Instant::now();
                if created || requested_sequence.is_none() {
                    return self.snapshot(stream);
                }
                let requested_sequence = requested_sequence.expect("checked above");
                if requested_sequence > stream.sequence {
                    return DataChangeEvent::ResyncRequired;
                }
                if let Some(first) = stream.journal.front() {
                    if requested_sequence < first.sequence.saturating_sub(1) {
                        return self.snapshot(stream);
                    }
                }
                if let Some(next) = stream
                    .journal
                    .iter()
                    .find(|event| event.sequence > requested_sequence)
                {
                    return next.event.clone();
                }
                stream.notify.clone().notified_owned()
            };
            tokio::select! {
                () = notified => {}
                () = &mut deadline => {
                    return DataChangeEvent::Timeout {
                        cursor: self.cursor(requested_sequence.expect("a waiting poll has a cursor")),
                    };
                }
            }
        }
    }

    pub async fn publish(
        &self,
        session_id: &str,
        subscription_id: &str,
        identity: &str,
        records: Vec<MaterializedRecordInfo>,
        refresh: DataSubscriptionRefresh,
    ) {
        let mut registry = self.registry.lock().await;
        if registry.terminal.contains_key(subscription_id) {
            return;
        }
        let Some(stream) = registry.active.get_mut(subscription_id) else {
            registry.active.insert(
                subscription_id.to_string(),
                StreamState {
                    session_id: session_id.to_string(),
                    identity: identity.to_string(),
                    records: records
                        .into_iter()
                        .map(|record| (record.path.clone(), record))
                        .collect(),
                    refresh,
                    sequence: 0,
                    journal: VecDeque::new(),
                    notify: Arc::new(Notify::new()),
                    last_polled_at: Instant::now(),
                },
            );
            return;
        };
        if stream.session_id != session_id || stream.identity != identity {
            return;
        }

        let next_records = records
            .into_iter()
            .map(|record| (record.path.clone(), record))
            .collect::<BTreeMap<_, _>>();
        let changed = next_records
            .iter()
            .filter(|(path, record)| stream.records.get(*path) != Some(*record))
            .map(|(_, record)| record.clone())
            .collect::<Vec<_>>();
        let removed = stream
            .records
            .keys()
            .filter(|path| !next_records.contains_key(*path))
            .cloned()
            .collect::<Vec<_>>();
        let refresh_changed = refresh_transition_changed(&stream.refresh, &refresh);
        let records_changed = !changed.is_empty() || !removed.is_empty();

        stream.records = next_records;
        stream.refresh = refresh.clone();
        if records_changed {
            let sequence = stream.sequence + 1;
            self.push_event(
                stream,
                DataChangeEvent::Changed {
                    cursor: self.cursor(sequence),
                    identity: identity.to_string(),
                    records: changed,
                    removed,
                },
            );
        }
        if refresh_changed {
            if matches!(
                stream.journal.back().map(|event| &event.event),
                Some(DataChangeEvent::State { .. })
            ) {
                stream.journal.pop_back();
            }
            let sequence = stream.sequence + 1;
            self.push_event(
                stream,
                DataChangeEvent::State {
                    cursor: self.cursor(sequence),
                    state: refresh,
                },
            );
        }
        if records_changed || refresh_changed {
            stream.notify.notify_waiters();
        }
    }

    pub async fn cancel_subscription(&self, session_id: &str, subscription_id: &str) {
        let mut registry = self.registry.lock().await;
        let Some(stream) = registry.active.remove(subscription_id) else {
            return;
        };
        if stream.session_id != session_id {
            registry.active.insert(subscription_id.to_string(), stream);
            return;
        }
        let notify = stream.notify;
        registry.terminal.insert(
            subscription_id.to_string(),
            TerminalTombstone {
                session_id: session_id.to_string(),
                event: DataChangeEvent::Cancelled,
                expires_at: Instant::now() + self.idle_timeout,
            },
        );
        notify.notify_waiters();
    }

    pub async fn revoke_session(&self, session_id: &str) {
        let mut registry = self.registry.lock().await;
        let subscription_ids = registry
            .active
            .iter()
            .filter(|(_, stream)| stream.session_id == session_id)
            .map(|(subscription_id, _)| subscription_id.clone())
            .collect::<Vec<_>>();
        for subscription_id in subscription_ids {
            let stream = registry
                .active
                .remove(&subscription_id)
                .expect("subscription id came from active registry");
            let notify = stream.notify;
            registry.terminal.insert(
                subscription_id,
                TerminalTombstone {
                    session_id: session_id.to_string(),
                    event: DataChangeEvent::Revoked,
                    expires_at: Instant::now() + self.idle_timeout,
                },
            );
            notify.notify_waiters();
        }
    }

    #[cfg(test)]
    async fn evict_idle(&self) {
        evict_idle_registry(&self.registry, self.idle_timeout).await;
    }

    #[cfg(test)]
    async fn active_stream_count(&self) -> usize {
        self.registry.lock().await.active.len()
    }

    #[cfg(test)]
    async fn terminal_tombstone_count(&self) -> usize {
        self.registry.lock().await.terminal.len()
    }

    fn push_event(&self, stream: &mut StreamState, event: DataChangeEvent) {
        stream.sequence += 1;
        stream.journal.push_back(SequencedEvent {
            sequence: stream.sequence,
            event,
        });
        while stream.journal.len() > self.capacity {
            stream.journal.pop_front();
        }
    }

    fn parse_cursor(&self, cursor: &str) -> Option<u64> {
        let (generation, sequence) = cursor.rsplit_once(':')?;
        (generation == self.generation.as_ref())
            .then(|| sequence.parse().ok())
            .flatten()
    }

    fn cursor(&self, sequence: u64) -> String {
        format!("{}:{sequence}", self.generation)
    }

    fn snapshot(&self, stream: &StreamState) -> DataChangeEvent {
        DataChangeEvent::Snapshot {
            cursor: self.cursor(stream.sequence),
            identity: stream.identity.clone(),
            records: stream.records.values().cloned().collect(),
            state: stream.refresh.clone(),
        }
    }
}

async fn evict_idle_registry(registry: &Arc<Mutex<StreamRegistry>>, idle_timeout: Duration) {
    let now = Instant::now();
    let mut registry = registry.lock().await;
    registry.active.retain(|_, stream| {
        Arc::strong_count(&stream.notify) > 1
            || now.duration_since(stream.last_polled_at) < idle_timeout
    });
    registry
        .terminal
        .retain(|_, tombstone| tombstone.expires_at > now);
}

fn refresh_transition_changed(
    previous: &DataSubscriptionRefresh,
    next: &DataSubscriptionRefresh,
) -> bool {
    match (previous, next) {
        (DataSubscriptionRefresh::Loading, DataSubscriptionRefresh::Loading)
        | (DataSubscriptionRefresh::Updating { .. }, DataSubscriptionRefresh::Updating { .. }) => {
            false
        }
        (DataSubscriptionRefresh::Ready { .. }, DataSubscriptionRefresh::Ready { .. }) => false,
        (
            DataSubscriptionRefresh::Stale {
                reason: previous, ..
            },
            DataSubscriptionRefresh::Stale { reason: next, .. },
        )
        | (
            DataSubscriptionRefresh::Unavailable { reason: previous },
            DataSubscriptionRefresh::Unavailable { reason: next },
        ) => previous != next,
        _ => true,
    }
}

impl Default for DataChangeStreams {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use jolt_network::MaterializedRecordInfo;

    use crate::session_store::DataSubscriptionRefresh;

    use super::{DataChangeEvent, DataChangeStreams};

    fn record(path: &str, revision: &str) -> MaterializedRecordInfo {
        MaterializedRecordInfo {
            path: path.to_string(),
            content_id: format!("cid_{revision}"),
            revision: revision.to_string(),
            created_at: 1,
        }
    }

    #[tokio::test]
    async fn current_cursor_receives_one_ordered_record_delta() {
        let streams = DataChangeStreams::with_generation_and_capacity("boot_a", 4);
        let initial = streams
            .next(
                "session_a",
                "sub_a",
                "alice.jolt",
                vec![],
                DataSubscriptionRefresh::Loading,
                None,
            )
            .await;
        let DataChangeEvent::Snapshot { cursor, .. } = initial else {
            panic!("first poll must establish a snapshot boundary");
        };

        streams
            .publish(
                "session_a",
                "sub_a",
                "alice.jolt",
                vec![record("/chirp/posts/one", "revision_1")],
                DataSubscriptionRefresh::Loading,
            )
            .await;

        assert_eq!(
            streams
                .next(
                    "session_a",
                    "sub_a",
                    "alice.jolt",
                    vec![],
                    DataSubscriptionRefresh::Loading,
                    Some(&cursor),
                )
                .await,
            DataChangeEvent::Changed {
                cursor: "boot_a:1".to_string(),
                identity: "alice.jolt".to_string(),
                records: vec![record("/chirp/posts/one", "revision_1")],
                removed: vec![],
            }
        );
    }

    #[tokio::test]
    async fn lagging_cursor_coalesces_to_the_latest_snapshot() {
        let streams = DataChangeStreams::with_generation_and_capacity("boot_a", 2);
        let initial = streams
            .next(
                "session_a",
                "sub_a",
                "alice.jolt",
                vec![],
                DataSubscriptionRefresh::Loading,
                None,
            )
            .await;
        let DataChangeEvent::Snapshot { cursor, .. } = initial else {
            panic!("first poll must establish a snapshot boundary");
        };
        for revision in 1..=3 {
            streams
                .publish(
                    "session_a",
                    "sub_a",
                    "alice.jolt",
                    vec![record("/chirp/posts/one", &format!("revision_{revision}"))],
                    DataSubscriptionRefresh::Loading,
                )
                .await;
        }

        assert!(matches!(
            streams
                .next(
                    "session_a",
                    "sub_a",
                    "alice.jolt",
                    vec![],
                    DataSubscriptionRefresh::Loading,
                    Some(&cursor),
                )
                .await,
            DataChangeEvent::Snapshot {
                cursor,
                records,
                ..
            } if cursor == "boot_a:3"
                && records == vec![record("/chirp/posts/one", "revision_3")]
        ));
    }

    #[tokio::test]
    async fn cursor_from_another_daemon_generation_requires_resync() {
        let streams = DataChangeStreams::with_generation_and_capacity("boot_b", 4);

        assert_eq!(
            streams
                .next(
                    "session_a",
                    "sub_a",
                    "alice.jolt",
                    vec![],
                    DataSubscriptionRefresh::Loading,
                    Some("boot_a:7"),
                )
                .await,
            DataChangeEvent::ResyncRequired,
        );
    }

    #[tokio::test]
    async fn lag_coalescing_never_hides_a_terminal_event() {
        let streams = DataChangeStreams::with_generation_and_capacity("boot_a", 2);
        let initial = streams
            .next(
                "session_a",
                "sub_a",
                "alice.jolt",
                vec![],
                DataSubscriptionRefresh::Loading,
                None,
            )
            .await;
        let DataChangeEvent::Snapshot { cursor, .. } = initial else {
            panic!("first poll must establish a snapshot boundary");
        };
        for revision in 1..=3 {
            streams
                .publish(
                    "session_a",
                    "sub_a",
                    "alice.jolt",
                    vec![record("/chirp/posts/one", &format!("revision_{revision}"))],
                    DataSubscriptionRefresh::Loading,
                )
                .await;
        }
        streams.cancel_subscription("session_a", "sub_a").await;

        assert_eq!(
            streams
                .next(
                    "session_a",
                    "sub_a",
                    "alice.jolt",
                    vec![],
                    DataSubscriptionRefresh::Loading,
                    Some(&cursor),
                )
                .await,
            DataChangeEvent::Cancelled,
        );
    }

    #[tokio::test]
    async fn terminal_events_wake_only_their_owned_waiters() {
        let streams = DataChangeStreams::with_generation_and_capacity("boot_a", 4);
        let alice = streams
            .next(
                "session_a",
                "sub_a",
                "alice.jolt",
                vec![],
                DataSubscriptionRefresh::Loading,
                None,
            )
            .await;
        let DataChangeEvent::Snapshot {
            cursor: alice_cursor,
            ..
        } = alice
        else {
            panic!("first poll must establish a snapshot boundary");
        };
        let bob = streams
            .next(
                "session_b",
                "sub_b",
                "bob.jolt",
                vec![],
                DataSubscriptionRefresh::Loading,
                None,
            )
            .await;
        let DataChangeEvent::Snapshot {
            cursor: bob_cursor, ..
        } = bob
        else {
            panic!("first poll must establish a snapshot boundary");
        };
        let alice_waiter = tokio::spawn({
            let streams = streams.clone();
            async move {
                streams
                    .next(
                        "session_a",
                        "sub_a",
                        "alice.jolt",
                        vec![],
                        DataSubscriptionRefresh::Loading,
                        Some(&alice_cursor),
                    )
                    .await
            }
        });
        let bob_waiter = tokio::spawn({
            let streams = streams.clone();
            async move {
                streams
                    .next(
                        "session_b",
                        "sub_b",
                        "bob.jolt",
                        vec![],
                        DataSubscriptionRefresh::Loading,
                        Some(&bob_cursor),
                    )
                    .await
            }
        });
        tokio::task::yield_now().await;

        streams.revoke_session("session_a").await;
        assert_eq!(alice_waiter.await.unwrap(), DataChangeEvent::Revoked);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), bob_waiter)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn terminal_delivery_releases_the_materialized_view() {
        let streams = DataChangeStreams::with_generation_and_capacity("boot_a", 4);
        let initial = streams
            .next(
                "session_a",
                "sub_a",
                "alice.jolt",
                vec![record("/chirp/posts/one", "revision_1")],
                DataSubscriptionRefresh::Loading,
                None,
            )
            .await;
        let DataChangeEvent::Snapshot { cursor, .. } = initial else {
            panic!("first poll must establish a snapshot boundary");
        };

        streams.cancel_subscription("session_a", "sub_a").await;

        assert_eq!(streams.active_stream_count().await, 0);
        assert_eq!(streams.terminal_tombstone_count().await, 1);
        assert_eq!(
            streams
                .next(
                    "session_a",
                    "sub_a",
                    "alice.jolt",
                    vec![],
                    DataSubscriptionRefresh::Loading,
                    Some(&cursor),
                )
                .await,
            DataChangeEvent::Cancelled,
        );
    }

    #[tokio::test]
    async fn idle_stream_is_evicted_and_rebuilt_from_a_snapshot() {
        let streams = DataChangeStreams::with_generation_capacity_and_idle_timeout(
            "boot_a",
            4,
            Duration::from_millis(10),
        );
        let initial = streams
            .next(
                "session_a",
                "sub_a",
                "alice.jolt",
                vec![record("/chirp/posts/one", "revision_1")],
                DataSubscriptionRefresh::Loading,
                None,
            )
            .await;
        let DataChangeEvent::Snapshot { cursor, .. } = initial else {
            panic!("first poll must establish a snapshot boundary");
        };

        tokio::time::sleep(Duration::from_millis(20)).await;
        streams.evict_idle().await;
        assert_eq!(streams.active_stream_count().await, 0);

        assert!(matches!(
            streams
                .next(
                    "session_a",
                    "sub_a",
                    "alice.jolt",
                    vec![record("/chirp/posts/one", "revision_1")],
                    DataSubscriptionRefresh::Loading,
                    Some(&cursor),
                )
                .await,
            DataChangeEvent::Snapshot { records, .. }
                if records == vec![record("/chirp/posts/one", "revision_1")]
        ));
    }

    #[tokio::test]
    async fn idle_eviction_preserves_a_live_waiter() {
        let streams = DataChangeStreams::with_generation_capacity_and_idle_timeout(
            "boot_a",
            4,
            Duration::from_millis(10),
        );
        let initial = streams
            .next(
                "session_a",
                "sub_a",
                "alice.jolt",
                vec![],
                DataSubscriptionRefresh::Loading,
                None,
            )
            .await;
        let DataChangeEvent::Snapshot { cursor, .. } = initial else {
            panic!("first poll must establish a snapshot boundary");
        };
        let waiter = tokio::spawn({
            let streams = streams.clone();
            async move {
                streams
                    .next_with_timeout(
                        "session_a",
                        "sub_a",
                        "alice.jolt",
                        vec![],
                        DataSubscriptionRefresh::Loading,
                        Some(&cursor),
                        Duration::from_secs(1),
                    )
                    .await
            }
        });
        tokio::task::yield_now().await;

        tokio::time::sleep(Duration::from_millis(20)).await;
        streams.evict_idle().await;
        assert_eq!(streams.active_stream_count().await, 1);
        streams
            .publish(
                "session_a",
                "sub_a",
                "alice.jolt",
                vec![record("/chirp/posts/one", "revision_1")],
                DataSubscriptionRefresh::Loading,
            )
            .await;

        assert!(matches!(
            waiter.await.unwrap(),
            DataChangeEvent::Changed { records, .. }
                if records == vec![record("/chirp/posts/one", "revision_1")]
        ));
    }

    #[tokio::test]
    async fn idle_long_poll_returns_a_cursor_preserving_timeout() {
        let streams = DataChangeStreams::with_generation_and_capacity("boot_a", 4);
        let initial = streams
            .next(
                "session_a",
                "sub_a",
                "alice.jolt",
                vec![],
                DataSubscriptionRefresh::Loading,
                None,
            )
            .await;
        let DataChangeEvent::Snapshot { cursor, .. } = initial else {
            panic!("first poll must establish a snapshot boundary");
        };

        assert_eq!(
            streams
                .next_with_timeout(
                    "session_a",
                    "sub_a",
                    "alice.jolt",
                    vec![],
                    DataSubscriptionRefresh::Loading,
                    Some(&cursor),
                    Duration::from_millis(10),
                )
                .await,
            DataChangeEvent::Timeout { cursor },
        );
    }
}
