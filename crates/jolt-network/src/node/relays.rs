use std::str::FromStr;

use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;
use tracing::debug;

use jolt_core::{RelayRecord, RelayRecordCapability};

use crate::error::NetworkError;
use crate::node::identity_heads::IDENTITY_HEAD_HINT_EXCHANGE_MAX;
use crate::protocol::{IdentityProviderCandidate, RelayExchangeRequest};

use super::{
    unix_now, NetworkNode, RELAY_EXCHANGE_MAX_RECORDS, RELAY_MESH_EXPLORATION_FANOUT,
    RELAY_RECORD_TTL_SECS,
};

fn relay_record_matches_capabilities(
    record: &RelayRecord,
    capabilities: &[RelayRecordCapability],
) -> bool {
    capabilities.is_empty()
        || capabilities
            .iter()
            .any(|capability| record.body.capabilities.contains(capability))
}

impl NetworkNode {
    /// Build this daemon's current signed relay record when relay mode is enabled.
    pub fn local_relay_record(&self, now: u64) -> Result<Option<RelayRecord>, NetworkError> {
        if !self.bootstrap_relay {
            return Ok(None);
        }

        let addrs = self
            .swarm
            .listeners()
            .map(|addr| addr.to_string())
            .collect();
        let record = RelayRecord::new(
            self.identity.public_key_bytes(),
            self.swarm.local_peer_id().to_string(),
            addrs,
            vec![
                RelayRecordCapability::Bootstrap,
                RelayRecordCapability::Discovery,
                RelayRecordCapability::Pinning,
            ],
            now,
            now + RELAY_RECORD_TTL_SECS,
            |bytes| self.identity.sign(bytes),
        )
        .map_err(|e| NetworkError::Protocol(e.to_string()));

        record.map(Some)
    }

    pub(super) fn start_relay_record_exchange(&mut self, peer: &libp2p::PeerId) {
        let now = unix_now();
        let mut known_records =
            self.relay_records_for_exchange(RELAY_EXCHANGE_MAX_RECORDS, &[], now);

        let announce_records: Vec<_> = known_records
            .drain(..)
            .filter(|record| record.body.peer_id != peer.to_string())
            .take(RELAY_EXCHANGE_MAX_RECORDS)
            .collect();

        if !announce_records.is_empty() {
            self.swarm.behaviour_mut().relay_exchange.send_request(
                peer,
                RelayExchangeRequest::AnnounceRelays {
                    records: announce_records,
                },
            );
        }

        let announce_hints =
            self.identity_head_hints_for_exchange(now, IDENTITY_HEAD_HINT_EXCHANGE_MAX);
        if !announce_hints.is_empty() {
            self.swarm.behaviour_mut().relay_exchange.send_request(
                peer,
                RelayExchangeRequest::AnnounceIdentityHeads {
                    hints: announce_hints,
                },
            );
        }

        self.swarm.behaviour_mut().relay_exchange.send_request(
            peer,
            RelayExchangeRequest::GetRelays {
                limit: RELAY_EXCHANGE_MAX_RECORDS as u16,
                capabilities: Vec::new(),
            },
        );

        self.swarm.behaviour_mut().relay_exchange.send_request(
            peer,
            RelayExchangeRequest::GetIdentityHeads {
                limit: IDENTITY_HEAD_HINT_EXCHANGE_MAX as u16,
            },
        );
    }

    pub(super) fn relay_records_for_exchange(
        &self,
        limit: usize,
        capabilities: &[RelayRecordCapability],
        now: u64,
    ) -> Vec<RelayRecord> {
        let mut records: Vec<RelayRecord> = self
            .store
            .load_relay_records(now)
            .map(|records| {
                records
                    .into_iter()
                    .map(|stored| stored.relay_record)
                    .collect()
            })
            .unwrap_or_else(|e| {
                debug!("Failed to load relay address book for exchange: {e}");
                Vec::new()
            });

        if let Ok(Some(local_record)) = self.local_relay_record(now) {
            records.push(local_record);
        }

        records.retain(|record| relay_record_matches_capabilities(record, capabilities));
        records.sort_by_key(|record| record.body.relay_id.to_string());
        records.dedup_by(|a, b| a.body.relay_id == b.body.relay_id);
        records.truncate(limit.min(RELAY_EXCHANGE_MAX_RECORDS));
        records
    }

    pub(super) fn store_relay_records(&self, records: Vec<RelayRecord>, now: u64) -> (u16, u16) {
        let mut accepted = 0u16;
        let mut rejected = 0u16;
        for record in records.into_iter().take(RELAY_EXCHANGE_MAX_RECORDS) {
            match self.store.record_relay_record(record, now) {
                Ok(()) => accepted = accepted.saturating_add(1),
                Err(e) => {
                    debug!("Rejected exchanged relay record: {e}");
                    rejected = rejected.saturating_add(1);
                }
            }
        }
        (accepted, rejected)
    }

    fn relay_record_peer_addr(record: &RelayRecord) -> Option<(libp2p::PeerId, Multiaddr)> {
        let peer_id = libp2p::PeerId::from_str(&record.body.peer_id).ok()?;
        let addr = record.body.addrs.first()?.parse::<Multiaddr>().ok()?;
        let addr = if addr
            .iter()
            .any(|protocol| matches!(protocol, Protocol::P2p(_)))
        {
            addr
        } else {
            addr.with(Protocol::P2p(peer_id))
        };
        Some((peer_id, addr))
    }

    pub(super) fn mark_relay_peer_success(&self, peer: &libp2p::PeerId, now: u64) {
        if let Err(e) = self
            .store
            .mark_relay_record_peer_success(&peer.to_string(), now)
        {
            debug!("Failed to mark relay peer success for {peer}: {e}");
        }
    }

    pub(super) fn mark_relay_peer_failure(&self, peer: &libp2p::PeerId) {
        if let Err(e) = self.store.mark_relay_record_peer_failure(&peer.to_string()) {
            debug!("Failed to mark relay peer failure for {peer}: {e}");
        }
    }

    pub(super) fn explore_relay_mesh(&mut self) {
        if !self.bootstrap_relay {
            return;
        }

        let now = unix_now();
        let records = match self.store.load_relay_records(now) {
            Ok(records) => records,
            Err(e) => {
                debug!("Failed to load relay records for mesh exploration: {e}");
                return;
            }
        };
        if records.is_empty() {
            return;
        }

        let local_peer = *self.swarm.local_peer_id();
        let start = self.relay_mesh_exploration_cursor % records.len();
        let mut explored = 0usize;

        for offset in 0..records.len() {
            if explored >= RELAY_MESH_EXPLORATION_FANOUT {
                break;
            }

            let index = (start + offset) % records.len();
            self.relay_mesh_exploration_cursor = (index + 1) % records.len();
            let record = &records[index].relay_record;
            let Some((peer_id, addr)) = Self::relay_record_peer_addr(record) else {
                continue;
            };
            if peer_id == local_peer {
                continue;
            }

            explored += 1;
            self.relay_mesh_peer_ids.insert(peer_id);
            self.swarm
                .behaviour_mut()
                .kademlia
                .add_address(&peer_id, addr.clone());

            if self.swarm.is_connected(&peer_id) {
                self.start_relay_record_exchange(&peer_id);
                continue;
            }

            debug!("Exploring relay mesh through {peer_id} at {addr}");
            if let Err(e) = self.swarm.dial(addr) {
                debug!("Failed to dial relay mesh peer {peer_id}: {e}");
                self.mark_relay_peer_failure(&peer_id);
            }
        }
    }

    pub(super) fn local_identity_provider_candidate(&self) -> IdentityProviderCandidate {
        let peer_id = self.swarm.local_peer_id().to_string();
        let addrs = self
            .swarm
            .listeners()
            .map(|addr| {
                if addr
                    .iter()
                    .any(|protocol| matches!(protocol, Protocol::P2p(_)))
                {
                    addr.to_string()
                } else {
                    addr.clone()
                        .with(Protocol::P2p(*self.swarm.local_peer_id()))
                        .to_string()
                }
            })
            .collect();
        IdentityProviderCandidate { peer_id, addrs }
    }

    pub(super) fn relay_record_candidate_for_peer(
        &self,
        peer: &libp2p::PeerId,
        now: u64,
    ) -> Option<IdentityProviderCandidate> {
        let peer_id = peer.to_string();
        let records = self.store.load_relay_records(now).ok()?;
        records
            .into_iter()
            .find(|record| record.relay_record.body.peer_id == peer_id)
            .map(|record| {
                let addrs = record
                    .relay_record
                    .body
                    .addrs
                    .into_iter()
                    .map(|addr| {
                        if addr.contains("/p2p/") {
                            addr
                        } else {
                            format!("{addr}/p2p/{peer_id}")
                        }
                    })
                    .collect();
                IdentityProviderCandidate { peer_id, addrs }
            })
    }
}
