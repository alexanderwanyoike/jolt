use std::collections::{HashMap, HashSet};

use jolt_store::RelayPinRecord;

use crate::command::RelayPinItem;
use crate::error::NetworkError;

use super::{NetworkNode, RelayPinReservation};

impl NetworkNode {
    pub(super) fn reserve_relay_pin(
        &mut self,
        owner: String,
        items: Vec<RelayPinItem>,
    ) -> Result<u64, NetworkError> {
        if !self.relay_pin_policy.is_allowed(&owner) {
            return Err(denied("identity is not allowlisted"));
        }
        if (self.relay_pin_policy.per_identity_quota_bytes.is_some()
            || self.relay_pin_policy.total_capacity_bytes.is_some())
            && items.is_empty()
        {
            return Err(denied(
                "signed content sizes are required when quotas are configured",
            ));
        }
        validate_items(&items)?;
        for item in &items {
            if self
                .relay_pin_records
                .iter()
                .any(|record| record.content_id == item.content_id && record.size != item.size)
                || self
                    .relay_pin_reservations
                    .values()
                    .flat_map(|reservation| reservation.items.iter())
                    .any(|pending| {
                        pending.content_id == item.content_id && pending.size != item.size
                    })
            {
                return Err(denied(
                    "signed content size conflicts with existing relay accounting",
                ));
            }
        }

        let owner_used = unique_owner_bytes(&self.relay_pin_records, &owner);
        let owner_pending = self
            .relay_pin_reservations
            .values()
            .filter(|reservation| reservation.owner == owner)
            .flat_map(|reservation| reservation.items.iter())
            .filter(|item| {
                !self
                    .relay_pin_records
                    .iter()
                    .any(|record| record.owner == owner && record.content_id == item.content_id)
            })
            .map(|item| item.size)
            .sum::<u64>();
        let owner_requested = items
            .iter()
            .filter(|item| {
                !self
                    .relay_pin_records
                    .iter()
                    .any(|record| record.owner == owner && record.content_id == item.content_id)
                    && !self.relay_pin_reservations.values().any(|reservation| {
                        reservation.owner == owner
                            && reservation
                                .items
                                .iter()
                                .any(|pending| pending.content_id == item.content_id)
                    })
            })
            .map(|item| item.size)
            .sum::<u64>();
        if let Some(limit) = self.relay_pin_policy.per_identity_quota_bytes {
            if owner_used
                .saturating_add(owner_pending)
                .saturating_add(owner_requested)
                > limit
            {
                return Err(denied(&format!(
                    "identity quota exceeded: used {owner_used}, pending {owner_pending}, requested {owner_requested}, limit {limit}"
                )));
            }
        }

        let total_used = self.store.stats().pinned_size;
        let pinned_ids: HashSet<String> = self
            .store
            .list_entries()
            .iter()
            .filter(|entry| entry.pinned)
            .map(|entry| entry.content_id.clone())
            .collect();
        let mut pending_sizes = HashMap::new();
        for item in self
            .relay_pin_reservations
            .values()
            .flat_map(|reservation| reservation.items.iter())
        {
            if !pinned_ids.contains(&item.content_id) {
                pending_sizes
                    .entry(item.content_id.as_str())
                    .or_insert(item.size);
            }
        }
        let total_pending = pending_sizes.values().copied().sum::<u64>();
        let requested = items
            .iter()
            .filter(|item| {
                !pinned_ids.contains(&item.content_id)
                    && !pending_sizes.contains_key(item.content_id.as_str())
            })
            .map(|item| item.size)
            .sum::<u64>();
        if let Some(limit) = self.relay_pin_policy.total_capacity_bytes {
            if total_used
                .saturating_add(total_pending)
                .saturating_add(requested)
                > limit
            {
                return Err(denied(&format!(
                    "relay capacity exceeded: used {total_used}, pending {total_pending}, requested {requested}, limit {limit}"
                )));
            }
        }

        let reservation_id = self.next_relay_pin_reservation_id;
        self.next_relay_pin_reservation_id = self.next_relay_pin_reservation_id.saturating_add(1);
        self.relay_pin_reservations
            .insert(reservation_id, RelayPinReservation { owner, items });
        Ok(reservation_id)
    }

    pub(super) fn commit_relay_pin(
        &mut self,
        reservation_id: u64,
        actual_items: Vec<RelayPinItem>,
    ) -> Result<(), NetworkError> {
        validate_items(&actual_items)?;
        let reservation = self
            .relay_pin_reservations
            .get(&reservation_id)
            .ok_or_else(|| denied("pin reservation is missing or expired"))?;
        if !reservation.items.is_empty() && reservation.items != actual_items {
            return Err(denied(
                "fetched content sizes do not match the signed request",
            ));
        }

        let mut records = self.relay_pin_records.clone();
        for item in actual_items {
            if let Some(record) = records.iter_mut().find(|record| {
                record.owner == reservation.owner && record.content_id == item.content_id
            }) {
                if record.size != item.size {
                    return Err(denied(
                        "stored content size conflicts with relay accounting",
                    ));
                }
                continue;
            }
            records.push(RelayPinRecord {
                owner: reservation.owner.clone(),
                content_id: item.content_id,
                size: item.size,
                pinned_at: super::unix_now(),
            });
        }
        self.store.save_relay_pin_records(&records).map_err(|e| {
            NetworkError::Protocol(format!("failed to persist relay pin accounting: {e}"))
        })?;
        self.relay_pin_records = records;
        self.relay_pin_reservations.remove(&reservation_id);
        Ok(())
    }
}

fn validate_items(items: &[RelayPinItem]) -> Result<(), NetworkError> {
    let mut seen = HashSet::new();
    for item in items {
        if !seen.insert(&item.content_id) {
            return Err(denied("pin request contains duplicate content ids"));
        }
    }
    Ok(())
}

fn unique_owner_bytes(records: &[RelayPinRecord], owner: &str) -> u64 {
    records
        .iter()
        .filter(|record| record.owner == owner)
        .map(|record| record.size)
        .sum()
}

fn denied(message: &str) -> NetworkError {
    NetworkError::InvalidInput(format!("relay pin denied: {message}"))
}

#[cfg(test)]
mod tests {
    use jolt_identity::NodeIdentity;
    use jolt_store::{CacheConfig, ContentStore};
    use tempfile::tempdir;

    use crate::{NetworkConfig, RelayPinPolicy};

    use super::*;

    fn node(
        path: &std::path::Path,
        allowed: &[&str],
        quota: Option<u64>,
        capacity: Option<u64>,
    ) -> NetworkNode {
        let store = ContentStore::open(path, CacheConfig::default()).unwrap();
        let mut config = NetworkConfig::test_config();
        config.bootstrap_relay = true;
        config.relay_pin_policy = RelayPinPolicy {
            allowed_identities: allowed.iter().map(|owner| owner.to_string()).collect(),
            per_identity_quota_bytes: quota,
            total_capacity_bytes: capacity,
        };
        NetworkNode::new_tcp(NodeIdentity::generate(), store, config).unwrap()
    }

    fn item(name: &str, size: u64) -> RelayPinItem {
        RelayPinItem {
            content_id: name.to_string(),
            size,
        }
    }

    #[tokio::test]
    async fn default_deny_rejects_pin_before_fetch() {
        let dir = tempdir().unwrap();
        let mut node = node(dir.path(), &[], None, None);

        let error = node
            .reserve_relay_pin("owner-a".to_string(), vec![item("cid-a", 1)])
            .unwrap_err();

        assert!(error.to_string().contains("identity is not allowlisted"));
    }

    #[tokio::test]
    async fn allowed_identity_can_reserve_and_cancel_capacity() {
        let dir = tempdir().unwrap();
        let mut node = node(dir.path(), &["owner-a"], Some(10), Some(10));
        let reservation = node
            .reserve_relay_pin("owner-a".to_string(), vec![item("cid-a", 10)])
            .unwrap();

        let error = node
            .reserve_relay_pin("owner-a".to_string(), vec![item("cid-b", 1)])
            .unwrap_err();
        assert!(error.to_string().contains("identity quota exceeded"));

        node.relay_pin_reservations.remove(&reservation);
        node.reserve_relay_pin("owner-a".to_string(), vec![item("cid-b", 1)])
            .unwrap();
    }

    #[tokio::test]
    async fn total_capacity_is_distinct_from_owner_quota() {
        let dir = tempdir().unwrap();
        let mut node = node(dir.path(), &["owner-a", "owner-b"], Some(20), Some(10));
        node.reserve_relay_pin("owner-a".to_string(), vec![item("cid-a", 10)])
            .unwrap();

        let error = node
            .reserve_relay_pin("owner-b".to_string(), vec![item("cid-b", 1)])
            .unwrap_err();
        assert!(error.to_string().contains("relay capacity exceeded"));
    }

    #[tokio::test]
    async fn total_capacity_includes_content_pinned_outside_relay_api() {
        let dir = tempdir().unwrap();
        let mut node = node(dir.path(), &["owner-a"], None, Some(10));
        let existing = b"1234567890";
        let content_id = jolt_core::ContentId::from_bytes(existing).to_string();
        node.store
            .cache_content(&content_id, existing, &[1], &[2])
            .unwrap();
        node.store.pin(&content_id).unwrap();

        let error = node
            .reserve_relay_pin("owner-a".to_string(), vec![item("cid-a", 1)])
            .unwrap_err();

        assert!(error.to_string().contains("relay capacity exceeded"));
    }

    #[tokio::test]
    async fn committed_owner_accounting_survives_restart() {
        let dir = tempdir().unwrap();
        let mut first = node(dir.path(), &["owner-a"], Some(10), Some(20));
        let items = vec![item("cid-a", 10)];
        let reservation = first
            .reserve_relay_pin("owner-a".to_string(), items.clone())
            .unwrap();
        first.commit_relay_pin(reservation, items).unwrap();
        drop(first);

        let mut reopened = node(dir.path(), &["owner-a"], Some(10), Some(20));
        let error = reopened
            .reserve_relay_pin("owner-a".to_string(), vec![item("cid-b", 1)])
            .unwrap_err();
        assert!(error.to_string().contains("identity quota exceeded"));
    }

    #[tokio::test]
    async fn commit_rejects_fetched_size_that_differs_from_signed_declaration() {
        let dir = tempdir().unwrap();
        let mut node = node(dir.path(), &["owner-a"], Some(10), Some(10));
        let reservation = node
            .reserve_relay_pin("owner-a".to_string(), vec![item("cid-a", 10)])
            .unwrap();

        let error = node
            .commit_relay_pin(reservation, vec![item("cid-a", 9)])
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("fetched content sizes do not match"));
        assert!(node.relay_pin_records.is_empty());
    }
}
