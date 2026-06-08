use crate::command::{IngressRecord, IngressStatus};
use crate::error::NetworkError;

#[derive(Default)]
pub(super) struct IngressQueue {
    records: Vec<IngressRecord>,
}

impl IngressQueue {
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
