use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use jolt_network::MaterializedRecordInfo;
use serde::Serialize;
use tokio::sync::{Mutex, Notify};

use crate::session_store::DataSubscriptionRefresh;

const DEFAULT_CHANGE_JOURNAL_CAPACITY: usize = 64;

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
    ResyncRequired,
    Cancelled,
    Revoked,
}

#[derive(Debug, Clone)]
pub struct DataChangeStreams {
    generation: Arc<str>,
    capacity: usize,
    streams: Arc<Mutex<HashMap<String, StreamState>>>,
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

    fn with_generation_and_capacity(generation: impl Into<String>, capacity: usize) -> Self {
        Self {
            generation: Arc::from(generation.into()),
            capacity: capacity.max(1),
            streams: Arc::new(Mutex::new(HashMap::new())),
        }
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
        let requested_sequence = match cursor {
            None => None,
            Some(cursor) => match self.parse_cursor(cursor) {
                Some(sequence) => Some(sequence),
                None => return DataChangeEvent::ResyncRequired,
            },
        };

        loop {
            let notified = {
                let mut streams = self.streams.lock().await;
                let stream = streams
                    .entry(subscription_id.to_string())
                    .or_insert_with(|| StreamState {
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
                    });
                if stream.session_id != session_id || stream.identity != identity {
                    return DataChangeEvent::ResyncRequired;
                }
                let Some(requested_sequence) = requested_sequence else {
                    return self.snapshot(stream);
                };
                if requested_sequence > stream.sequence {
                    return DataChangeEvent::ResyncRequired;
                }
                if let Some(terminal) = stream.journal.iter().find(|event| {
                    event.sequence > requested_sequence && event_is_terminal(&event.event)
                }) {
                    return terminal.event.clone();
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
            notified.await;
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
        let mut streams = self.streams.lock().await;
        let Some(stream) = streams.get_mut(subscription_id) else {
            streams.insert(
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
        let mut streams = self.streams.lock().await;
        let Some(stream) = streams.get_mut(subscription_id) else {
            return;
        };
        if stream.session_id != session_id || stream_is_terminal(stream) {
            return;
        }
        self.push_event(stream, DataChangeEvent::Cancelled);
        stream.notify.notify_waiters();
    }

    pub async fn revoke_session(&self, session_id: &str) {
        let mut streams = self.streams.lock().await;
        for stream in streams
            .values_mut()
            .filter(|stream| stream.session_id == session_id)
        {
            if stream_is_terminal(stream) {
                continue;
            }
            self.push_event(stream, DataChangeEvent::Revoked);
            stream.notify.notify_waiters();
        }
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

fn stream_is_terminal(stream: &StreamState) -> bool {
    stream
        .journal
        .back()
        .is_some_and(|event| event_is_terminal(&event.event))
}

fn event_is_terminal(event: &DataChangeEvent) -> bool {
    matches!(event, DataChangeEvent::Cancelled | DataChangeEvent::Revoked)
}

fn refresh_transition_changed(
    previous: &DataSubscriptionRefresh,
    next: &DataSubscriptionRefresh,
) -> bool {
    match (previous, next) {
        (DataSubscriptionRefresh::Loading, DataSubscriptionRefresh::Loading)
        | (DataSubscriptionRefresh::Updating { .. }, DataSubscriptionRefresh::Updating { .. })
        | (DataSubscriptionRefresh::Ready { .. }, DataSubscriptionRefresh::Ready { .. }) => false,
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
}
