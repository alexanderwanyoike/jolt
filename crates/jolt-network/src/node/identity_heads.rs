use std::collections::{HashMap, HashSet};

use jolt_core::{IdentityHeadHint, IdentityId};

use crate::command::RelayDiagnoseIdentityHeadObservation;
use crate::error::NetworkError;
use crate::protocol::IdentityProviderCandidate;

use super::{unix_now, NetworkNode};

const IDENTITY_HEAD_HINT_MAX_GLOBAL: usize = 1024;
pub(super) const IDENTITY_HEAD_HINT_MAX_PER_IDENTITY: usize = 4;
pub(super) const IDENTITY_HEAD_HINT_EXCHANGE_MAX: usize = 32;

#[derive(Default)]
pub(super) struct IdentityHeadHintBook {
    hints: HashMap<IdentityId, Vec<IdentityHeadHint>>,
    gossip_cursor: usize,
}

impl IdentityHeadHintBook {
    pub(super) fn exchange_batch(&mut self, now: u64, limit: usize) -> Vec<IdentityHeadHint> {
        let mut hints_by_identity: Vec<_> = self
            .hints
            .iter()
            .filter_map(|(identity, hints)| {
                let best = hints
                    .iter()
                    .filter(|hint| hint.verify_at(now).is_ok())
                    .max_by(|a, b| {
                        a.body
                            .latest_sequence
                            .cmp(&b.body.latest_sequence)
                            .then_with(|| a.body.expires_at.cmp(&b.body.expires_at))
                            .then_with(|| a.body.provider_peer_id.cmp(&b.body.provider_peer_id))
                    })?
                    .clone();
                Some((identity.to_string(), best))
            })
            .collect();

        hints_by_identity.sort_by(|a, b| a.0.cmp(&b.0));
        if hints_by_identity.is_empty() {
            self.gossip_cursor = 0;
            return Vec::new();
        }

        let limit = limit
            .min(IDENTITY_HEAD_HINT_EXCHANGE_MAX)
            .min(hints_by_identity.len());
        let start = self.gossip_cursor % hints_by_identity.len();
        let mut selected = Vec::with_capacity(limit);
        for offset in 0..hints_by_identity.len() {
            if selected.len() >= limit {
                break;
            }
            let index = (start + offset) % hints_by_identity.len();
            selected.push(hints_by_identity[index].1.clone());
        }
        self.gossip_cursor = (start + selected.len()) % hints_by_identity.len();
        selected
    }

    pub(super) fn record_many(&mut self, hints: Vec<IdentityHeadHint>, now: u64) -> (u16, u16) {
        self.prune(now);
        let mut accepted = 0u16;
        let mut rejected = 0u16;

        for hint in hints.into_iter().take(IDENTITY_HEAD_HINT_EXCHANGE_MAX) {
            if let Err(e) = hint.verify_at(now) {
                tracing::debug!(target: "jolt_network::node", "Rejected identity-head hint: {e}");
                rejected = rejected.saturating_add(1);
                continue;
            }

            let identity = hint.body.identity.clone();
            let provider = hint.body.provider_peer_id.clone();
            let entry = self.hints.entry(identity).or_default();
            if let Some(existing) = entry
                .iter_mut()
                .find(|existing| existing.body.provider_peer_id == provider)
            {
                if hint.body.latest_sequence <= existing.body.latest_sequence {
                    rejected = rejected.saturating_add(1);
                    continue;
                }
                *existing = hint;
            } else {
                entry.push(hint);
            }

            entry.sort_by(|a, b| {
                b.body
                    .latest_sequence
                    .cmp(&a.body.latest_sequence)
                    .then_with(|| b.body.expires_at.cmp(&a.body.expires_at))
            });
            entry.truncate(IDENTITY_HEAD_HINT_MAX_PER_IDENTITY);
            accepted = accepted.saturating_add(1);
        }

        self.enforce_global_bound();
        (accepted, rejected)
    }

    fn prune(&mut self, now: u64) {
        self.hints.retain(|_, hints| {
            hints.retain(|hint| hint.verify_at(now).is_ok());
            !hints.is_empty()
        });
    }

    fn enforce_global_bound(&mut self) {
        self.enforce_global_bound_with_limit(IDENTITY_HEAD_HINT_MAX_GLOBAL);
    }

    pub(super) fn enforce_global_bound_with_limit(&mut self, limit: usize) {
        let mut ranked = Vec::new();
        for (identity, hints) in &self.hints {
            for hint in hints {
                ranked.push((
                    identity.clone(),
                    hint.body.provider_peer_id.clone(),
                    hint.body.latest_sequence,
                    hint.body.expires_at,
                ));
            }
        }
        if ranked.len() <= limit {
            return;
        }

        ranked.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.3.cmp(&a.3)));
        let keep: HashSet<_> = ranked
            .into_iter()
            .take(limit)
            .map(|(identity, provider, _, _)| (identity, provider))
            .collect();
        self.hints.retain(|identity, hints| {
            hints.retain(|hint| {
                keep.contains(&(identity.clone(), hint.body.provider_peer_id.clone()))
            });
            !hints.is_empty()
        });
    }

    pub(super) fn candidates(
        &self,
        identity: &IdentityId,
        now: u64,
    ) -> Vec<IdentityProviderCandidate> {
        let Some(hints) = self.hints.get(identity) else {
            return Vec::new();
        };

        hints
            .iter()
            .filter(|hint| hint.verify_at(now).is_ok())
            .map(|hint| IdentityProviderCandidate {
                peer_id: hint.body.provider_peer_id.clone(),
                addrs: hint.body.provider_addrs.clone(),
            })
            .collect()
    }

    pub(super) fn diagnose(
        &self,
        identity: &IdentityId,
        now: u64,
    ) -> RelayDiagnoseIdentityHeadObservation {
        let Some(hints) = self.hints.get(identity) else {
            return RelayDiagnoseIdentityHeadObservation {
                state: "miss".to_string(),
                fresh_count: 0,
                expired_count: 0,
                latest_sequence: None,
                provider_peer_id: None,
                expires_at: None,
            };
        };

        let mut fresh = Vec::new();
        let mut expired_count = 0;
        for hint in hints {
            if hint.verify_at(now).is_ok() {
                fresh.push(hint);
            } else {
                expired_count += 1;
            }
        }
        let best = fresh.iter().max_by_key(|hint| hint.body.latest_sequence);

        RelayDiagnoseIdentityHeadObservation {
            state: if fresh.is_empty() {
                "miss".to_string()
            } else {
                "hit".to_string()
            },
            fresh_count: fresh.len(),
            expired_count,
            latest_sequence: best.map(|hint| hint.body.latest_sequence),
            provider_peer_id: best.map(|hint| hint.body.provider_peer_id.clone()),
            expires_at: best.map(|hint| hint.body.expires_at),
        }
    }

    #[cfg(test)]
    pub(super) fn get(&self, identity: &IdentityId) -> Option<&Vec<IdentityHeadHint>> {
        self.hints.get(identity)
    }

    #[cfg(test)]
    pub(super) fn stored_count(&self) -> usize {
        self.hints.values().map(Vec::len).sum()
    }
}

impl NetworkNode {
    pub(super) fn identity_head_hints_for_exchange(
        &mut self,
        now: u64,
        limit: usize,
    ) -> Vec<IdentityHeadHint> {
        self.identity_head_hints.exchange_batch(now, limit)
    }

    pub fn record_identity_head_hint(
        &mut self,
        hint: IdentityHeadHint,
    ) -> Result<(), NetworkError> {
        let now = unix_now();
        hint.verify_at(now)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        self.record_identity_head_hints(vec![hint], now);
        Ok(())
    }

    pub(super) fn record_identity_head_hints(
        &mut self,
        hints: Vec<IdentityHeadHint>,
        now: u64,
    ) -> (u16, u16) {
        self.identity_head_hints.record_many(hints, now)
    }

    pub(super) fn diagnose_identity_head_hints(
        &self,
        identity: &IdentityId,
        now: u64,
    ) -> RelayDiagnoseIdentityHeadObservation {
        self.identity_head_hints.diagnose(identity, now)
    }
}
