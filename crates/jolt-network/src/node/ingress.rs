use jolt_store::PersistedIngressRecord;

use crate::command::{IngressRecord, IngressStatus};
use crate::error::NetworkError;

/// Persisted queues are capped at the newest records by arrival time so a
/// flooded or long-lived queue cannot grow the file without bound.
const MAX_PERSISTED_INGRESS_RECORDS: usize = 1024;

#[derive(Default)]
pub(super) struct IngressQueue {
    records: Vec<IngressRecord>,
}

impl IngressQueue {
    /// Rebuild the queue from disk, dropping records whose envelope has
    /// expired. Decided records are kept so accept/reject cannot be replayed
    /// after a restart.
    pub(super) fn from_persisted(persisted: Vec<PersistedIngressRecord>, now: u64) -> Self {
        let records = persisted
            .into_iter()
            .filter(|record| record.expires_at.is_none_or(|expires_at| expires_at > now))
            .map(|record| IngressRecord {
                ingress_id: record.ingress_id,
                receiver_id: record.receiver_id,
                sender_identity: record.sender_identity,
                recipient_identity: record.recipient_identity,
                schema_hint: record.schema_hint,
                status: match record.status.as_str() {
                    "accepted" => IngressStatus::Accepted,
                    "rejected" => IngressStatus::Rejected,
                    _ => IngressStatus::Pending,
                },
                received_at: record.received_at,
                expires_at: record.expires_at,
                size: record.encrypted_object.len() as u64,
                encrypted_object: record.encrypted_object,
                accepted_at: record.accepted_at,
                rejected_at: record.rejected_at,
            })
            .collect();
        Self { records }
    }

    pub(super) fn to_persisted(&self) -> Vec<PersistedIngressRecord> {
        let mut records: Vec<&IngressRecord> = self.records.iter().collect();
        records.sort_by_key(|record| std::cmp::Reverse(record.received_at));
        records.truncate(MAX_PERSISTED_INGRESS_RECORDS);
        records
            .into_iter()
            .map(|record| PersistedIngressRecord {
                ingress_id: record.ingress_id.clone(),
                receiver_id: record.receiver_id.clone(),
                sender_identity: record.sender_identity.clone(),
                recipient_identity: record.recipient_identity.clone(),
                schema_hint: record.schema_hint.clone(),
                status: match record.status {
                    IngressStatus::Pending => "pending",
                    IngressStatus::Accepted => "accepted",
                    IngressStatus::Rejected => "rejected",
                }
                .to_string(),
                received_at: record.received_at,
                expires_at: record.expires_at,
                encrypted_object: record.encrypted_object.clone(),
                accepted_at: record.accepted_at,
                rejected_at: record.rejected_at,
            })
            .collect()
    }

    pub(super) fn push(&mut self, record: IngressRecord) {
        self.records.push(record);
    }

    pub(super) fn list_pending(&self) -> Vec<IngressRecord> {
        self.records
            .iter()
            .filter(|record| record.status == IngressStatus::Pending)
            .cloned()
            .collect()
    }

    pub(super) fn encrypted_object(&self, ingress_id: &str) -> Result<&[u8], NetworkError> {
        let record = self.find(ingress_id)?;
        Ok(&record.encrypted_object)
    }

    pub(super) fn accept(
        &mut self,
        ingress_id: &str,
        now: u64,
    ) -> Result<IngressRecord, NetworkError> {
        self.decide(ingress_id, IngressStatus::Accepted, now)
    }

    pub(super) fn reject(
        &mut self,
        ingress_id: &str,
        now: u64,
    ) -> Result<IngressRecord, NetworkError> {
        self.decide(ingress_id, IngressStatus::Rejected, now)
    }

    fn decide(
        &mut self,
        ingress_id: &str,
        status: IngressStatus,
        now: u64,
    ) -> Result<IngressRecord, NetworkError> {
        let record = self.find_mut(ingress_id)?;
        if record.status == status {
            return Ok(record.clone());
        }
        if record.status != IngressStatus::Pending {
            return Err(NetworkError::InvalidInput(format!(
                "ingress envelope is not pending: {ingress_id}"
            )));
        }
        record.status = status;
        match record.status {
            IngressStatus::Accepted => record.accepted_at = Some(now),
            IngressStatus::Rejected => record.rejected_at = Some(now),
            IngressStatus::Pending => {}
        }
        Ok(record.clone())
    }

    fn find(&self, ingress_id: &str) -> Result<&IngressRecord, NetworkError> {
        self.records
            .iter()
            .find(|record| record.ingress_id == ingress_id)
            .ok_or_else(|| {
                NetworkError::InvalidInput(format!("ingress envelope not found: {ingress_id}"))
            })
    }

    fn find_mut(&mut self, ingress_id: &str) -> Result<&mut IngressRecord, NetworkError> {
        self.records
            .iter_mut()
            .find(|record| record.ingress_id == ingress_id)
            .ok_or_else(|| {
                NetworkError::InvalidInput(format!("ingress envelope not found: {ingress_id}"))
            })
    }
}
