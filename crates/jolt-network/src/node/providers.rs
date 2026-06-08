use std::collections::HashSet;
use std::str::FromStr;
use std::time::Instant;

use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId};
use libp2p::Multiaddr;
use tokio::sync::oneshot;
use tracing::{debug, warn};

use jolt_core::{verify_update_log_for_identity, IdentityId};

use crate::command::{
    RelayDiagnoseCacheObservation, RelayDiagnoseForwarding, RelayDiagnoseForwardingAttempt,
    RelayDiagnoseIdentityHeadObservation, RelayDiagnoseIdentityResponse, RelayDiagnoseOutcome,
    RelayDiagnoseProviderCandidate,
};
use crate::error::NetworkError;
use crate::protocol::{IdentityProviderCandidate, RelayExchangeRequest, RelayExchangeResponse};

use super::{
    unix_now, unix_now_ms, NetworkNode, PendingIdentityProviderDiagnosis,
    PendingIdentityProviderForwardGroup, IDENTITY_PROVIDER_QUERY_FANOUT,
    IDENTITY_PROVIDER_QUERY_LIMIT, IDENTITY_PROVIDER_QUERY_TIMEOUT, IDENTITY_PROVIDER_QUERY_TTL,
    SEEN_IDENTITY_PROVIDER_QUERY_TTL,
};

impl From<IdentityProviderCandidate> for RelayDiagnoseProviderCandidate {
    fn from(candidate: IdentityProviderCandidate) -> Self {
        Self {
            peer_id: candidate.peer_id,
            addrs: candidate.addrs,
        }
    }
}

fn dedupe_diagnose_provider_candidates(
    candidates: Vec<RelayDiagnoseProviderCandidate>,
    limit: usize,
) -> Vec<RelayDiagnoseProviderCandidate> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for candidate in candidates {
        if seen.insert(candidate.peer_id.clone()) {
            deduped.push(candidate);
            if deduped.len() >= limit {
                break;
            }
        }
    }
    deduped
}

impl NetworkNode {
    fn identity_provider_candidates(
        &self,
        identity: &IdentityId,
        limit: usize,
        now: u64,
    ) -> Vec<IdentityProviderCandidate> {
        let mut candidates = Vec::new();
        if self.update_logs.contains_key(identity) {
            candidates.push(self.local_identity_provider_candidate());
        }

        let key = Self::update_log_provider_key(identity);
        if let Some(providers) = self.discovered_providers.get(&key) {
            for provider in providers {
                if let Some(candidate) = self.relay_record_candidate_for_peer(provider, now) {
                    candidates.push(candidate);
                } else {
                    candidates.push(IdentityProviderCandidate {
                        peer_id: provider.to_string(),
                        addrs: Vec::new(),
                    });
                }
            }
        }

        candidates.extend(self.identity_head_hints.candidates(identity, now));

        Self::dedupe_identity_provider_candidates(candidates, limit)
    }

    pub(super) fn dedupe_identity_provider_candidates(
        candidates: Vec<IdentityProviderCandidate>,
        limit: usize,
    ) -> Vec<IdentityProviderCandidate> {
        let mut seen = HashSet::new();
        let mut deduped = Vec::new();
        for candidate in candidates {
            if seen.insert(candidate.peer_id.clone()) {
                deduped.push(candidate);
                if deduped.len() >= limit {
                    break;
                }
            }
        }
        deduped
    }

    fn relay_query_peers(&self, excluded: Option<&libp2p::PeerId>) -> Vec<libp2p::PeerId> {
        self.swarm
            .connected_peers()
            .filter(|peer| Some(*peer) != excluded)
            .filter(|peer| {
                self.bootstrap_peer_ids.contains(peer) || self.relay_mesh_peer_ids.contains(peer)
            })
            .take(IDENTITY_PROVIDER_QUERY_FANOUT)
            .copied()
            .collect()
    }

    fn identity_provider_query_id(&self, identity: &IdentityId) -> String {
        format!(
            "{}:{}:{}",
            self.swarm.local_peer_id(),
            identity,
            unix_now_ms()
        )
    }

    pub(super) fn diagnose_identity(
        &mut self,
        identity: IdentityId,
        response_tx: oneshot::Sender<Result<RelayDiagnoseIdentityResponse, NetworkError>>,
    ) {
        let now = unix_now();
        let local_update_log_cache = self.diagnose_update_log_cache(&identity);
        let identity_head_hint = self.diagnose_identity_head_hints(&identity, now);
        let local_provider_candidates: Vec<_> = self
            .identity_provider_candidates(&identity, IDENTITY_PROVIDER_QUERY_LIMIT as usize, now)
            .into_iter()
            .map(Into::into)
            .collect();
        let relay_targets = self.relay_query_peers(None);
        let provider_candidates = local_provider_candidates.clone();
        if !provider_candidates.is_empty() || relay_targets.is_empty() {
            let response = self.build_identity_diagnosis_response(
                &identity,
                local_update_log_cache,
                identity_head_hint,
                local_provider_candidates,
                provider_candidates,
                Vec::new(),
            );
            let _ = response_tx.send(Ok(response));
            return;
        }

        let query_id = self.identity_provider_query_id(&identity);
        let request = RelayExchangeRequest::FindIdentityProviders {
            query_id: query_id.clone(),
            identity: identity.clone(),
            limit: IDENTITY_PROVIDER_QUERY_LIMIT,
            ttl: IDENTITY_PROVIDER_QUERY_TTL,
            deadline_unix_ms: unix_now_ms() + self.resolve_timeout.as_millis() as u64,
        };
        let mut attempts = std::collections::HashMap::new();
        for peer in relay_targets {
            let request_id = self
                .swarm
                .behaviour_mut()
                .relay_exchange
                .send_request(&peer, request.clone());
            self.pending_identity_provider_diagnostic_requests
                .insert(request_id, query_id.clone());
            attempts.insert(
                request_id,
                RelayDiagnoseForwardingAttempt {
                    peer_id: peer.to_string(),
                    status: "pending".to_string(),
                    candidate_count: 0,
                    error: None,
                },
            );
        }

        self.pending_identity_provider_diagnostics.insert(
            query_id,
            PendingIdentityProviderDiagnosis {
                identity,
                local_update_log_cache,
                identity_head_hint,
                local_provider_candidates,
                response_tx,
                deadline: Instant::now() + self.resolve_timeout,
                attempts,
                providers: Vec::new(),
                limit: IDENTITY_PROVIDER_QUERY_LIMIT as usize,
            },
        );
    }

    fn build_identity_diagnosis_response(
        &self,
        identity: &IdentityId,
        local_update_log_cache: RelayDiagnoseCacheObservation,
        identity_head_hint: RelayDiagnoseIdentityHeadObservation,
        local_provider_candidates: Vec<RelayDiagnoseProviderCandidate>,
        provider_candidates: Vec<RelayDiagnoseProviderCandidate>,
        attempts: Vec<RelayDiagnoseForwardingAttempt>,
    ) -> RelayDiagnoseIdentityResponse {
        RelayDiagnoseIdentityResponse {
            identity: identity.to_string(),
            provider_key: Self::update_log_provider_key(identity),
            local_update_log_cache,
            identity_head_hint,
            local_provider_candidates,
            provider_candidates: provider_candidates.clone(),
            relay_forwarding: RelayDiagnoseForwarding {
                attempted: !attempts.is_empty(),
                target_count: attempts.len(),
                attempts,
            },
            outcome: self.diagnose_identity_outcome(identity, !provider_candidates.is_empty()),
        }
    }

    fn diagnose_update_log_cache(&self, identity: &IdentityId) -> RelayDiagnoseCacheObservation {
        let Some(entries) = self.update_logs.get(identity) else {
            return RelayDiagnoseCacheObservation {
                state: "miss".to_string(),
                entry_count: 0,
                latest_sequence: None,
            };
        };

        let latest_sequence = verify_update_log_for_identity(identity, entries).ok();
        RelayDiagnoseCacheObservation {
            state: if latest_sequence.is_some() {
                "hit".to_string()
            } else {
                "invalid".to_string()
            },
            entry_count: entries.len(),
            latest_sequence,
        }
    }

    fn diagnose_identity_outcome(
        &self,
        identity: &IdentityId,
        has_provider_candidates: bool,
    ) -> RelayDiagnoseOutcome {
        if has_provider_candidates {
            return RelayDiagnoseOutcome {
                code: "provider_candidates_found".to_string(),
                message: "Provider candidates are known for this identity.".to_string(),
            };
        }

        let no_bootstrap_relays = self.effective_bootstrap_relays.is_empty()
            && self.swarm.connected_peers().next().is_none();
        let relay_unreachable = !self.effective_bootstrap_relays.is_empty()
            && self.connected_bootstrap_peer_count() == 0;

        match Self::identity_provider_failure(identity, no_bootstrap_relays, relay_unreachable) {
            NetworkError::DiscoveryFailed { code, message } => RelayDiagnoseOutcome {
                code: code.as_str().to_string(),
                message,
            },
            other => RelayDiagnoseOutcome {
                code: "diagnose_failed".to_string(),
                message: other.to_string(),
            },
        }
    }

    pub(super) fn handle_identity_diagnosis_response(
        &mut self,
        request_id: OutboundRequestId,
        providers: Vec<IdentityProviderCandidate>,
    ) -> bool {
        let Some(query_id) = self
            .pending_identity_provider_diagnostic_requests
            .remove(&request_id)
        else {
            return false;
        };

        let Some(mut pending) = self.pending_identity_provider_diagnostics.remove(&query_id) else {
            return true;
        };

        if let Some(attempt) = pending.attempts.get_mut(&request_id) {
            attempt.status = "responded".to_string();
            attempt.candidate_count = providers.len();
        }
        pending.providers.extend(providers);

        if self.identity_diagnosis_should_finish(&pending) {
            self.finish_identity_diagnosis(pending);
        } else {
            self.pending_identity_provider_diagnostics
                .insert(query_id, pending);
        }

        true
    }

    pub(super) fn handle_identity_diagnosis_failure(
        &mut self,
        request_id: OutboundRequestId,
        error: String,
    ) -> bool {
        let Some(query_id) = self
            .pending_identity_provider_diagnostic_requests
            .remove(&request_id)
        else {
            return false;
        };

        let Some(mut pending) = self.pending_identity_provider_diagnostics.remove(&query_id) else {
            return true;
        };

        if let Some(attempt) = pending.attempts.get_mut(&request_id) {
            attempt.status = "failed".to_string();
            attempt.error = Some(error);
        }

        if self.identity_diagnosis_should_finish(&pending) {
            self.finish_identity_diagnosis(pending);
        } else {
            self.pending_identity_provider_diagnostics
                .insert(query_id, pending);
        }

        true
    }

    fn identity_diagnosis_should_finish(&self, pending: &PendingIdentityProviderDiagnosis) -> bool {
        pending
            .attempts
            .values()
            .all(|attempt| attempt.status != "pending")
            || pending.providers.len() >= pending.limit
    }

    fn finish_identity_diagnosis(&mut self, pending: PendingIdentityProviderDiagnosis) {
        for request_id in pending.attempts.keys() {
            self.pending_identity_provider_diagnostic_requests
                .remove(request_id);
        }

        let mut provider_candidates = pending.local_provider_candidates.clone();
        provider_candidates.extend(
            Self::dedupe_identity_provider_candidates(pending.providers, pending.limit)
                .into_iter()
                .map(Into::into),
        );
        provider_candidates =
            dedupe_diagnose_provider_candidates(provider_candidates, pending.limit);

        let response = self.build_identity_diagnosis_response(
            &pending.identity,
            pending.local_update_log_cache,
            pending.identity_head_hint,
            pending.local_provider_candidates,
            provider_candidates,
            pending.attempts.into_values().collect(),
        );
        let _ = pending.response_tx.send(Ok(response));
    }

    pub(super) fn check_identity_diagnosis_timeouts(&mut self) {
        let now = Instant::now();
        let timed_out: Vec<_> = self
            .pending_identity_provider_diagnostics
            .iter()
            .filter_map(|(query_id, pending)| (pending.deadline <= now).then_some(query_id.clone()))
            .collect();

        for query_id in timed_out {
            if let Some(mut pending) = self.pending_identity_provider_diagnostics.remove(&query_id)
            {
                for attempt in pending.attempts.values_mut() {
                    if attempt.status == "pending" {
                        attempt.status = "timeout".to_string();
                        attempt.error = Some("relay forwarding timed out".to_string());
                    }
                }
                self.finish_identity_diagnosis(pending);
            }
        }
    }

    pub(super) fn start_identity_provider_relay_query(&mut self, identity: &IdentityId) {
        let query_id = self.identity_provider_query_id(identity);
        let request = RelayExchangeRequest::FindIdentityProviders {
            query_id,
            identity: identity.clone(),
            limit: IDENTITY_PROVIDER_QUERY_LIMIT,
            ttl: IDENTITY_PROVIDER_QUERY_TTL,
            deadline_unix_ms: unix_now_ms() + IDENTITY_PROVIDER_QUERY_TIMEOUT.as_millis() as u64,
        };

        for peer in self.relay_query_peers(None) {
            self.swarm
                .behaviour_mut()
                .relay_exchange
                .send_request(&peer, request.clone());
        }
    }

    pub(super) fn record_identity_provider_candidates(
        &mut self,
        identity: &IdentityId,
        providers: Vec<IdentityProviderCandidate>,
    ) -> Vec<libp2p::PeerId> {
        let key = Self::update_log_provider_key(identity);
        let mut accepted = Vec::new();

        for provider in providers {
            let Ok(peer_id) = libp2p::PeerId::from_str(&provider.peer_id) else {
                continue;
            };
            if peer_id == *self.swarm.local_peer_id() {
                continue;
            }

            for addr in provider.addrs {
                if let Ok(addr) = addr.parse::<Multiaddr>() {
                    let addr = if addr
                        .iter()
                        .any(|protocol| matches!(protocol, Protocol::P2p(_)))
                    {
                        addr
                    } else {
                        addr.with(Protocol::P2p(peer_id))
                    };
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());
                    self.swarm.add_peer_address(peer_id, addr.clone());
                    if let Err(e) = self.swarm.dial(addr) {
                        debug!("Failed to dial identity provider candidate {peer_id}: {e}");
                    }
                }
            }

            let providers = self.discovered_providers.entry(key.clone()).or_default();
            if !providers.contains(&peer_id) {
                providers.push(peer_id);
                accepted.push(peer_id);
            }
        }

        accepted
    }

    fn prune_seen_identity_provider_queries(&mut self) {
        let now = Instant::now();
        self.seen_identity_provider_queries
            .retain(|_, seen_at| now.duration_since(*seen_at) < SEEN_IDENTITY_PROVIDER_QUERY_TTL);
    }

    pub(super) fn handle_identity_provider_query(
        &mut self,
        peer: libp2p::PeerId,
        query_id: String,
        identity: IdentityId,
        limit: u16,
        ttl: u8,
        deadline_unix_ms: u64,
        channel: request_response::ResponseChannel<RelayExchangeResponse>,
    ) {
        self.prune_seen_identity_provider_queries();
        let now_ms = unix_now_ms();
        let limit = usize::from(limit.min(IDENTITY_PROVIDER_QUERY_LIMIT));
        let providers = self.identity_provider_candidates(&identity, limit, unix_now());
        let already_seen = self
            .seen_identity_provider_queries
            .insert(query_id.clone(), Instant::now())
            .is_some();
        debug!(
            "Identity provider query {query_id} for {identity}: {} local candidates, ttl={ttl}",
            providers.len()
        );

        if already_seen
            || !providers.is_empty()
            || ttl == 0
            || deadline_unix_ms <= now_ms
            || providers.len() >= limit
        {
            self.respond_identity_providers(channel, query_id, identity, providers, limit);
            return;
        }

        let next_peers = self.relay_query_peers(Some(&peer));
        if next_peers.is_empty() {
            self.respond_identity_providers(channel, query_id, identity, providers, limit);
            return;
        }
        debug!(
            "Forwarding identity provider query {query_id} for {identity} to {} relay(s) with ttl={}",
            next_peers.len(),
            ttl.saturating_sub(1)
        );

        let remaining_requests = next_peers.len();
        for next_peer in next_peers {
            let request_id = self.swarm.behaviour_mut().relay_exchange.send_request(
                &next_peer,
                RelayExchangeRequest::FindIdentityProviders {
                    query_id: query_id.clone(),
                    identity: identity.clone(),
                    limit: limit as u16,
                    ttl: ttl.saturating_sub(1),
                    deadline_unix_ms,
                },
            );
            self.pending_identity_provider_forwards
                .insert(request_id, query_id.clone());
        }

        self.pending_identity_provider_forward_groups.insert(
            query_id.clone(),
            PendingIdentityProviderForwardGroup {
                query_id,
                identity,
                providers,
                channel,
                deadline: Instant::now() + IDENTITY_PROVIDER_QUERY_TIMEOUT,
                remaining_requests,
                limit,
            },
        );
    }

    pub(super) fn respond_identity_providers(
        &mut self,
        channel: request_response::ResponseChannel<RelayExchangeResponse>,
        query_id: String,
        identity: IdentityId,
        providers: Vec<IdentityProviderCandidate>,
        limit: usize,
    ) {
        let providers = Self::dedupe_identity_provider_candidates(providers, limit);
        if let Err(e) = self.swarm.behaviour_mut().relay_exchange.send_response(
            channel,
            RelayExchangeResponse::IdentityProviders {
                query_id,
                identity,
                providers,
            },
        ) {
            warn!("Failed to send identity provider response: {e:?}");
        }
    }

    pub(super) fn check_identity_provider_forward_timeouts(&mut self) {
        let now = Instant::now();
        let timed_out: Vec<_> = self
            .pending_identity_provider_forward_groups
            .iter()
            .filter_map(|(query_id, pending)| (pending.deadline <= now).then_some(query_id.clone()))
            .collect();

        for query_id in timed_out {
            if let Some(pending) = self
                .pending_identity_provider_forward_groups
                .remove(&query_id)
            {
                self.pending_identity_provider_forwards
                    .retain(|_, pending_query_id| pending_query_id != &query_id);
                self.respond_identity_providers(
                    pending.channel,
                    pending.query_id,
                    pending.identity,
                    pending.providers,
                    pending.limit,
                );
            }
        }
    }
}
