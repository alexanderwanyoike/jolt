use std::collections::{HashMap, HashSet};
use std::time::Instant;

use jolt_store::RelayPinRecord;

use crate::command::{RelayPinItem, RelayPinRequestItems};
use crate::error::NetworkError;

use super::{NetworkNode, RelayPinReservation};

impl NetworkNode {
    #[cfg(test)]
    pub(super) fn reserve_relay_pin(
        &mut self,
        owner: String,
        items: Vec<RelayPinItem>,
    ) -> Result<u64, NetworkError> {
        self.reserve_relay_pin_request(owner, RelayPinRequestItems::Declared(items))
    }

    pub(super) fn reserve_relay_pin_request(
        &mut self,
        owner: String,
        request: RelayPinRequestItems,
    ) -> Result<u64, NetworkError> {
        self.prune_expired_relay_pin_reservations();
        if !self.relay_pin_policy.is_allowed(&owner) {
            return Err(denied("identity is not allowlisted"));
        }
        validate_request(&request)?;
        self.validate_declared_size_conflicts(None, &request)?;

        let (owner_reserved_bytes, total_reserved_bytes) =
            self.reservation_bytes(&owner, &request, None)?;
        let reservation_id = self.next_relay_pin_reservation_id;
        self.next_relay_pin_reservation_id = self.next_relay_pin_reservation_id.saturating_add(1);
        self.relay_pin_reservations.insert(
            reservation_id,
            RelayPinReservation {
                owner,
                request,
                owner_reserved_bytes,
                total_reserved_bytes,
                expires_at: Instant::now() + self.relay_pin_reservation_ttl,
            },
        );
        Ok(reservation_id)
    }

    pub(super) fn validate_relay_pin(
        &mut self,
        reservation_id: u64,
        actual_items: &[RelayPinItem],
    ) -> Result<(), NetworkError> {
        self.prune_expired_relay_pin_reservations();
        validate_items(actual_items)?;
        let reservation = self
            .relay_pin_reservations
            .get(&reservation_id)
            .ok_or_else(|| denied("pin reservation is missing or expired"))?;

        match &reservation.request {
            RelayPinRequestItems::Declared(declared) if declared != actual_items => {
                return Err(denied(
                    "fetched content sizes do not match the signed request",
                ));
            }
            RelayPinRequestItems::Legacy(content_ids) => {
                let actual_ids = actual_items
                    .iter()
                    .map(|item| item.content_id.as_str())
                    .collect::<HashSet<_>>();
                let requested_ids = content_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<HashSet<_>>();
                if actual_ids != requested_ids {
                    return Err(denied("fetched content does not match the signed request"));
                }
            }
            RelayPinRequestItems::Declared(_) => {}
        }

        let owner = reservation.owner.clone();
        let actual_request = RelayPinRequestItems::Declared(actual_items.to_vec());
        self.validate_declared_size_conflicts(Some(reservation_id), &actual_request)?;
        let (owner_reserved_bytes, total_reserved_bytes) =
            self.reservation_bytes(&owner, &actual_request, Some(reservation_id))?;
        let reservation = self
            .relay_pin_reservations
            .get_mut(&reservation_id)
            .ok_or_else(|| denied("pin reservation is missing or expired"))?;
        reservation.request = actual_request;
        reservation.owner_reserved_bytes = owner_reserved_bytes;
        reservation.total_reserved_bytes = total_reserved_bytes;
        reservation.expires_at = Instant::now() + self.relay_pin_reservation_ttl;
        Ok(())
    }

    pub(super) fn commit_relay_pin(
        &mut self,
        reservation_id: u64,
        actual_items: Vec<RelayPinItem>,
    ) -> Result<(), NetworkError> {
        self.validate_relay_pin(reservation_id, &actual_items)?;
        for item in &actual_items {
            if !self.store.is_pinned(&item.content_id) {
                return Err(denied("content is not pinned"));
            }
            if self.store.content_size(&item.content_id) != Some(item.size) {
                return Err(denied(
                    "pinned content size does not match relay accounting",
                ));
            }
        }

        let owner = self
            .relay_pin_reservations
            .get(&reservation_id)
            .ok_or_else(|| denied("pin reservation is missing or expired"))?
            .owner
            .clone();
        let mut records = self.relay_pin_records.clone();
        for item in actual_items {
            if let Some(record) = records
                .iter_mut()
                .find(|record| record.owner == owner && record.content_id == item.content_id)
            {
                if record.size != item.size {
                    return Err(denied(
                        "stored content size conflicts with relay accounting",
                    ));
                }
                continue;
            }
            records.push(RelayPinRecord {
                owner: owner.clone(),
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

    pub(super) fn unpin_relay_content(&mut self, content_id: &str) -> Result<(), NetworkError> {
        self.store
            .unpin(content_id)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let mut records = self.relay_pin_records.clone();
        records.retain(|record| record.content_id != content_id);
        self.store.save_relay_pin_records(&records).map_err(|e| {
            NetworkError::Protocol(format!("failed to persist relay pin accounting: {e}"))
        })?;
        self.relay_pin_records = records;
        Ok(())
    }

    pub(super) fn reconcile_relay_pin_records(&mut self) -> Result<(), NetworkError> {
        let mut records = self.relay_pin_records.clone();
        records.retain(|record| {
            self.store.is_pinned(&record.content_id)
                && self.store.content_size(&record.content_id) == Some(record.size)
        });
        if records != self.relay_pin_records {
            self.store.save_relay_pin_records(&records).map_err(|e| {
                NetworkError::Protocol(format!("failed to reconcile relay pin accounting: {e}"))
            })?;
            self.relay_pin_records = records;
        }
        Ok(())
    }

    pub(super) fn prune_expired_relay_pin_reservations(&mut self) {
        let now = Instant::now();
        self.relay_pin_reservations
            .retain(|_, reservation| reservation.expires_at > now);
    }

    fn reservation_bytes(
        &self,
        owner: &str,
        request: &RelayPinRequestItems,
        excluded_reservation: Option<u64>,
    ) -> Result<(u64, u64), NetworkError> {
        let owner_used = unique_owner_bytes(&self.relay_pin_records, owner);
        let owner_pending = self
            .relay_pin_reservations
            .iter()
            .filter(|(id, reservation)| {
                Some(**id) != excluded_reservation && reservation.owner == owner
            })
            .map(|(_, reservation)| reservation.owner_reserved_bytes)
            .sum::<u64>();
        let total_used = self.store.pinned_size();
        let total_pending = self
            .relay_pin_reservations
            .iter()
            .filter(|(id, _)| Some(**id) != excluded_reservation)
            .map(|(_, reservation)| reservation.total_reserved_bytes)
            .sum::<u64>();

        let owner_covered = self.covered_content_ids(Some(owner), excluded_reservation);
        let total_covered = self.covered_content_ids(None, excluded_reservation);
        let (owner_requested, total_requested) = match request {
            RelayPinRequestItems::Declared(items) => (
                items
                    .iter()
                    .filter(|item| !owner_covered.contains(item.content_id.as_str()))
                    .map(|item| item.size)
                    .sum(),
                items
                    .iter()
                    .filter(|item| {
                        !self.store.is_pinned(&item.content_id)
                            && !total_covered.contains(item.content_id.as_str())
                    })
                    .map(|item| item.size)
                    .sum(),
            ),
            RelayPinRequestItems::Legacy(content_ids) => {
                let owner_has_unknown = content_ids
                    .iter()
                    .any(|content_id| !owner_covered.contains(content_id.as_str()));
                let total_has_unknown = content_ids.iter().any(|content_id| {
                    !self.store.is_pinned(content_id)
                        && !total_covered.contains(content_id.as_str())
                });
                (
                    if owner_has_unknown {
                        self.relay_pin_policy
                            .per_identity_quota_bytes
                            .map(|limit| {
                                limit.saturating_sub(owner_used.saturating_add(owner_pending))
                            })
                            .unwrap_or(0)
                    } else {
                        0
                    },
                    if total_has_unknown {
                        self.relay_pin_policy
                            .total_capacity_bytes
                            .map(|limit| {
                                limit.saturating_sub(total_used.saturating_add(total_pending))
                            })
                            .unwrap_or(0)
                    } else {
                        0
                    },
                )
            }
        };

        if let Some(limit) = self.relay_pin_policy.per_identity_quota_bytes {
            if owner_used
                .saturating_add(owner_pending)
                .saturating_add(owner_requested)
                > limit
                || (!request.content_ids().is_empty()
                    && owner_requested == 0
                    && owner_used.saturating_add(owner_pending) >= limit
                    && request
                        .content_ids()
                        .iter()
                        .any(|content_id| !owner_covered.contains(*content_id)))
            {
                return Err(denied("identity quota exceeded"));
            }
        }
        if let Some(limit) = self.relay_pin_policy.total_capacity_bytes {
            if total_used
                .saturating_add(total_pending)
                .saturating_add(total_requested)
                > limit
                || (!request.content_ids().is_empty()
                    && total_requested == 0
                    && total_used.saturating_add(total_pending) >= limit
                    && request.content_ids().iter().any(|content_id| {
                        !self.store.is_pinned(content_id) && !total_covered.contains(*content_id)
                    }))
            {
                return Err(denied("relay capacity exceeded"));
            }
        }
        Ok((owner_requested, total_requested))
    }

    fn covered_content_ids<'a>(
        &'a self,
        owner: Option<&str>,
        excluded_reservation: Option<u64>,
    ) -> HashSet<&'a str> {
        let mut covered = self
            .relay_pin_records
            .iter()
            .filter(|record| owner.is_none_or(|owner| record.owner == owner))
            .map(|record| record.content_id.as_str())
            .collect::<HashSet<_>>();
        for (_, reservation) in self
            .relay_pin_reservations
            .iter()
            .filter(|(id, reservation)| {
                Some(**id) != excluded_reservation
                    && owner.is_none_or(|owner| reservation.owner == owner)
            })
        {
            covered.extend(reservation.request.content_ids());
        }
        covered
    }

    fn validate_declared_size_conflicts(
        &self,
        excluded_reservation: Option<u64>,
        request: &RelayPinRequestItems,
    ) -> Result<(), NetworkError> {
        let RelayPinRequestItems::Declared(items) = request else {
            return Ok(());
        };
        let mut known_sizes = HashMap::<&str, u64>::new();
        for record in &self.relay_pin_records {
            known_sizes.insert(record.content_id.as_str(), record.size);
        }
        for (_, reservation) in self
            .relay_pin_reservations
            .iter()
            .filter(|(id, _)| Some(**id) != excluded_reservation)
        {
            if let RelayPinRequestItems::Declared(pending) = &reservation.request {
                for item in pending {
                    known_sizes.insert(item.content_id.as_str(), item.size);
                }
            }
        }
        if items.iter().any(|item| {
            known_sizes
                .get(item.content_id.as_str())
                .is_some_and(|size| *size != item.size)
        }) {
            return Err(denied(
                "signed content size conflicts with existing relay accounting",
            ));
        }
        Ok(())
    }
}

fn validate_request(request: &RelayPinRequestItems) -> Result<(), NetworkError> {
    match request {
        RelayPinRequestItems::Declared(items) => validate_items(items),
        RelayPinRequestItems::Legacy(content_ids) => {
            if content_ids.is_empty() {
                return Err(denied("pin request contains no content ids"));
            }
            let mut seen = HashSet::new();
            if content_ids
                .iter()
                .any(|content_id| !seen.insert(content_id))
            {
                return Err(denied("pin request contains duplicate content ids"));
            }
            Ok(())
        }
    }
}

fn validate_items(items: &[RelayPinItem]) -> Result<(), NetworkError> {
    if items.is_empty() {
        return Err(denied("pin request contains no content ids"));
    }
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

    fn pin(node: &mut NetworkNode, data: &[u8]) -> RelayPinItem {
        let content_id = jolt_core::ContentId::from_bytes(data).to_string();
        node.store
            .cache_content(&content_id, data, &[1], &[2])
            .unwrap();
        node.store.pin(&content_id).unwrap();
        item(&content_id, data.len() as u64)
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
        pin(&mut node, b"1234567890");

        let error = node
            .reserve_relay_pin("owner-a".to_string(), vec![item("cid-a", 1)])
            .unwrap_err();

        assert!(error.to_string().contains("relay capacity exceeded"));
    }

    #[tokio::test]
    async fn committed_owner_accounting_survives_restart() {
        let dir = tempdir().unwrap();
        let mut first = node(dir.path(), &["owner-a"], Some(10), Some(20));
        let items = vec![pin(&mut first, b"1234567890")];
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

    #[tokio::test]
    async fn legacy_request_is_admitted_conservatively_when_quota_is_configured() {
        let dir = tempdir().unwrap();
        let mut node = node(dir.path(), &["owner-a"], Some(10), Some(20));

        let reservation = node.reserve_relay_pin_request(
            "owner-a".to_string(),
            RelayPinRequestItems::Legacy(vec!["cid-a".to_string()]),
        );

        assert!(
            reservation.is_ok(),
            "allowlisted pre-size clients must remain usable during rollout"
        );
    }

    #[tokio::test]
    async fn repeated_pending_content_is_counted_once_per_owner() {
        let dir = tempdir().unwrap();
        let mut node = node(dir.path(), &["owner-a"], Some(10), Some(20));

        for _ in 0..3 {
            node.reserve_relay_pin("owner-a".to_string(), vec![item("cid-a", 10)])
                .expect("the same owner and content consumes quota once");
        }
    }

    #[tokio::test]
    async fn quota_denial_does_not_disclose_internal_byte_totals() {
        let dir = tempdir().unwrap();
        let mut node = node(dir.path(), &["owner-a"], Some(10), Some(20));
        node.reserve_relay_pin("owner-a".to_string(), vec![item("cid-a", 10)])
            .unwrap();

        let error = node
            .reserve_relay_pin("owner-a".to_string(), vec![item("cid-b", 7)])
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "Invalid input: relay pin denied: identity quota exceeded"
        );
    }

    #[tokio::test]
    async fn accounting_cannot_commit_before_content_is_pinned() {
        let dir = tempdir().unwrap();
        let mut node = node(dir.path(), &["owner-a"], Some(10), Some(20));
        let items = vec![item("cid-a", 10)];
        let reservation = node
            .reserve_relay_pin("owner-a".to_string(), items.clone())
            .unwrap();

        let error = node.commit_relay_pin(reservation, items).unwrap_err();

        assert!(error.to_string().contains("content is not pinned"));
        assert!(node.relay_pin_records.is_empty());
    }

    #[tokio::test]
    async fn expired_reservation_releases_capacity_after_client_disconnect() {
        let dir = tempdir().unwrap();
        let mut node = node(dir.path(), &["owner-a"], Some(10), Some(10));
        let reservation = node
            .reserve_relay_pin("owner-a".to_string(), vec![item("cid-a", 10)])
            .unwrap();
        node.relay_pin_reservations
            .get_mut(&reservation)
            .unwrap()
            .expires_at = Instant::now();

        node.reserve_relay_pin("owner-a".to_string(), vec![item("cid-b", 10)])
            .expect("an abandoned request must stop consuming quota after its TTL");
    }

    #[tokio::test]
    async fn unpin_releases_persisted_owner_quota() {
        let dir = tempdir().unwrap();
        let mut node = node(dir.path(), &["owner-a"], Some(10), Some(20));
        let pinned = pin(&mut node, b"1234567890");
        let reservation = node
            .reserve_relay_pin("owner-a".to_string(), vec![pinned.clone()])
            .unwrap();
        node.commit_relay_pin(reservation, vec![pinned.clone()])
            .unwrap();

        node.unpin_relay_content(&pinned.content_id).unwrap();

        node.reserve_relay_pin("owner-a".to_string(), vec![item("cid-b", 10)])
            .expect("unpin must release the owner's quota as well as cache pin state");
        assert!(node.relay_pin_records.is_empty());
        assert!(node.store.load_relay_pin_records().unwrap().is_empty());
    }

    #[tokio::test]
    async fn restart_reconciles_accounting_when_pin_state_was_lost() {
        let dir = tempdir().unwrap();
        let mut first = node(dir.path(), &["owner-a"], Some(10), Some(20));
        let pinned = pin(&mut first, b"1234567890");
        let reservation = first
            .reserve_relay_pin("owner-a".to_string(), vec![pinned.clone()])
            .unwrap();
        first
            .commit_relay_pin(reservation, vec![pinned.clone()])
            .unwrap();
        first.store.unpin(&pinned.content_id).unwrap();
        drop(first);

        let mut reopened = node(dir.path(), &["owner-a"], Some(10), Some(20));

        assert!(reopened.relay_pin_records.is_empty());
        reopened
            .reserve_relay_pin("owner-a".to_string(), vec![item("cid-b", 10)])
            .expect("startup reconciliation must release stale accounting");
    }
}
