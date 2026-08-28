use libp2p::request_response;
use libp2p::swarm::SwarmEvent;

use jolt_core::verify_update_log_for_identity;

use crate::behaviour::JoltBehaviourEvent;
use crate::error::NetworkError;
use crate::protocol::{
    ContentResponse, DeviceWriterSyncResponse, IngressSubmitResponse, RelayExchangeRequest,
    RelayExchangeResponse, UpdateLogRequest, UpdateLogResponse,
};

use super::{unix_now, NetworkNode, PeerConnectionInfo};

macro_rules! debug {
    ($($arg:tt)*) => {
        tracing::debug!(target: "jolt_network::node", $($arg)*)
    };
}

macro_rules! info {
    ($($arg:tt)*) => {
        tracing::info!(target: "jolt_network::node", $($arg)*)
    };
}

macro_rules! warn {
    ($($arg:tt)*) => {
        tracing::warn!(target: "jolt_network::node", $($arg)*)
    };
}

impl NetworkNode {
    pub fn handle_swarm_event(&mut self, event: SwarmEvent<JoltBehaviourEvent>) {
        match event {
            // --- mDNS ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::Mdns(libp2p::mdns::Event::Discovered(
                peers,
            ))) => {
                for (peer_id, addr) in peers {
                    info!("mDNS discovered peer: {peer_id} at {addr}");
                    self.local_device_sync_candidates.insert(peer_id);
                    self.swarm.add_peer_address(peer_id, addr.clone());
                    if let Err(e) = self.swarm.dial(addr) {
                        debug!("Failed to dial discovered peer: {e}");
                    }
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::Mdns(libp2p::mdns::Event::Expired(
                peers,
            ))) => {
                for (peer_id, _addr) in peers {
                    debug!("mDNS peer expired: {peer_id}");
                }
            }

            // --- Content Fetch ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::ContentFetch(
                request_response::Event::Message {
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                    ..
                },
            )) => {
                info!("Received content request for: {}", request.content_id);

                let response =
                    if let Some(content_data) = self.store.get_content(&request.content_id) {
                        ContentResponse {
                            data: content_data.data,
                            signature: content_data.signature,
                            publisher_key: content_data.publisher_key,
                        }
                    } else {
                        debug!("Content not found: {}", request.content_id);
                        ContentResponse {
                            data: vec![],
                            signature: vec![],
                            publisher_key: vec![],
                        }
                    };

                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .content_fetch
                    .send_response(channel, response)
                {
                    warn!("Failed to send response: {e:?}");
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::ContentFetch(
                request_response::Event::Message {
                    message:
                        request_response::Message::Response {
                            request_id,
                            response,
                        },
                    ..
                },
            )) => {
                info!("Received content response ({} bytes)", response.data.len());

                // Try fetch manager first (daemon-managed fetches)
                if !response.data.is_empty() {
                    if let Some(content_id_str) = self
                        .fetch_manager
                        .on_content_response(request_id, response.clone())
                    {
                        // Cache the fetched content for re-sharing
                        if let Err(e) = self.store.cache_content(
                            &content_id_str,
                            &response.data,
                            &response.publisher_key,
                            &response.signature,
                        ) {
                            warn!("Failed to cache content: {e}");
                        } else {
                            info!("Content cached for re-sharing: {content_id_str}");
                        }
                        return;
                    }
                }

                // Fall back to legacy pending_fetches (for non-daemon operations)
                if let Some((content_id_str, tx)) = self.pending_fetches.remove(&request_id) {
                    if !response.data.is_empty() {
                        let digest_ok = content_id_str
                            .parse::<jolt_core::ContentId>()
                            .map(|id| id.verify(&response.data))
                            .unwrap_or(false);
                        if !digest_ok {
                            warn!(
                                "Content response for {content_id_str} failed digest verification, rejecting"
                            );
                            let _ = tx.send(Err(crate::error::NetworkError::VerificationFailed));
                            return;
                        }
                        if let Err(e) = self.store.cache_content(
                            &content_id_str,
                            &response.data,
                            &response.publisher_key,
                            &response.signature,
                        ) {
                            warn!("Failed to cache content: {e}");
                        }
                    }
                    let _ = tx.send(Ok(response));
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::ContentFetch(
                request_response::Event::OutboundFailure {
                    request_id, error, ..
                },
            )) => {
                warn!("Outbound request failed: {error}");

                // Try fetch manager first
                if self.fetch_manager.on_request_failure(request_id) {
                    return;
                }

                // Fall back to legacy pending_fetches
                if let Some((_content_id, tx)) = self.pending_fetches.remove(&request_id) {
                    let _ = tx.send(Err(NetworkError::Protocol(error.to_string())));
                }
            }

            // --- Update Log Sync ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::UpdateLogSync(
                request_response::Event::Message {
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                    ..
                },
            )) => {
                info!("Received update-log request for: {}", request.identity);

                let response = UpdateLogResponse {
                    entries: self
                        .update_logs
                        .get(&request.identity)
                        .cloned()
                        .unwrap_or_default(),
                };

                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .update_log_sync
                    .send_response(channel, response)
                {
                    warn!("Failed to send update-log response: {e:?}");
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::UpdateLogSync(
                request_response::Event::Message {
                    message:
                        request_response::Message::Response {
                            request_id,
                            response,
                        },
                    ..
                },
            )) => {
                info!(
                    "Received update-log response ({} entries)",
                    response.entries.len()
                );

                if let Some((identity, tx)) = self.pending_update_log_requests.remove(&request_id) {
                    let result = if response.entries.is_empty() {
                        Ok(response)
                    } else {
                        self.store_verified_update_log(identity, response.entries.clone())
                            .map(|_| response)
                    };
                    let _ = tx.send(result);
                } else if let Some(pending) = self.pending_update_log_pins.remove(&request_id) {
                    let result = if response.entries.is_empty() {
                        Err(NetworkError::Protocol(format!(
                            "No update log entries returned for {}",
                            pending.identity
                        )))
                    } else {
                        verify_update_log_for_identity(&pending.identity, &response.entries)
                            .map_err(|e| NetworkError::Protocol(e.to_string()))
                            .and_then(|sequence| {
                                self.store_verified_update_log(
                                    pending.identity.clone(),
                                    response.entries,
                                )?;
                                self.announce_update_log_provider(&pending.identity)?;
                                Ok(sequence)
                            })
                    };
                    let _ = pending.response_tx.send(result);
                } else if let Some((address, now, _provider, tx)) =
                    self.pending_jolt_resolutions.remove(&request_id)
                {
                    let result = if response.entries.is_empty() {
                        Err(Self::identity_head_invalid_failure(
                            address.identity(),
                            "provider returned no update log entries",
                        ))
                    } else {
                        self.store_verified_update_log(address.identity().clone(), response.entries)
                            .map_err(|e| {
                                Self::identity_head_invalid_failure(
                                    address.identity(),
                                    e.to_string(),
                                )
                            })
                            .and_then(|_| self.resolve_cached_jolt_address(&address, now))
                    };
                    let _ = tx.send(result);
                } else if let Some(pending) = self.pending_daemon_resolutions.remove(&request_id) {
                    if response.entries.is_empty() {
                        if let Some(response_tx) = pending.response_tx {
                            let result = pending.fallback_response.map(Ok).unwrap_or_else(|| {
                                Err(Self::identity_head_invalid_failure(
                                    pending.address.identity(),
                                    "provider returned no update log entries",
                                ))
                            });
                            let _ = response_tx.send(result);
                        }
                    } else {
                        let result = self
                            .store_verified_update_log(
                                pending.address.identity().clone(),
                                response.entries,
                            )
                            .map_err(|e| {
                                Self::identity_head_invalid_failure(
                                    pending.address.identity(),
                                    e.to_string(),
                                )
                            });

                        if let Some(response_tx) = pending.response_tx {
                            let result = result.and_then(|_| {
                                self.resolve_response_from_cache(
                                    &pending.address,
                                    pending.now,
                                    "network",
                                )
                            });
                            let _ = response_tx.send(result);
                        } else if let Err(err) = result {
                            warn!(
                                "Background update-log refresh failed for {}: {err}",
                                pending.address.identity()
                            );
                        }
                    }
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::UpdateLogSync(
                request_response::Event::OutboundFailure {
                    request_id, error, ..
                },
            )) => {
                warn!("Outbound update-log request failed: {error}");

                if let Some((_identity, tx)) = self.pending_update_log_requests.remove(&request_id)
                {
                    let _ = tx.send(Err(NetworkError::Protocol(error.to_string())));
                }
                if let Some(pending) = self.pending_update_log_pins.remove(&request_id) {
                    let _ = pending
                        .response_tx
                        .send(Err(NetworkError::Protocol(error.to_string())));
                }
                if let Some((address, now, provider, tx)) =
                    self.pending_jolt_resolutions.remove(&request_id)
                {
                    if let Some(next_provider) = self.take_discovered_update_log_provider_except(
                        address.identity(),
                        Some(&provider),
                    ) {
                        let request = UpdateLogRequest {
                            identity: address.identity().clone(),
                            since: self
                                .update_logs
                                .get(address.identity())
                                .and_then(|entries| {
                                    verify_update_log_for_identity(address.identity(), entries).ok()
                                }),
                        };
                        let request_id = self
                            .swarm
                            .behaviour_mut()
                            .update_log_sync
                            .send_request(&next_provider, request);
                        self.pending_jolt_resolutions
                            .insert(request_id, (address, now, next_provider, tx));
                    } else {
                        let _ = tx.send(Err(NetworkError::Protocol(error.to_string())));
                    }
                }
                if let Some(pending) = self.pending_daemon_resolutions.remove(&request_id) {
                    if let Some(next_provider) = self.take_discovered_update_log_provider_except(
                        pending.address.identity(),
                        Some(&pending.provider),
                    ) {
                        self.request_daemon_update_log_from_provider(
                            pending.address,
                            pending.now,
                            &next_provider,
                            pending.response_tx,
                            pending.fallback_response,
                        );
                    } else if let Some(response_tx) = pending.response_tx {
                        let result = pending
                            .fallback_response
                            .map(Ok)
                            .unwrap_or_else(|| Err(NetworkError::Protocol(error.to_string())));
                        let _ = response_tx.send(result);
                    }
                }
            }

            // --- Device Writer Sync ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::DeviceWriterSync(
                request_response::Event::Message {
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                    ..
                },
            )) => {
                info!(
                    "Received device-writer sync request for: {}",
                    request.identity
                );

                let response = self
                    .device_writer_sync_snapshot(&request.identity)
                    .map(|(authority_records, device_logs)| {
                        DeviceWriterSyncResponse::for_request(
                            request.max_operation_version,
                            authority_records,
                            device_logs,
                        )
                    })
                    .unwrap_or_default();

                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .device_writer_sync
                    .send_response(channel, response)
                {
                    warn!("Failed to send device-writer sync response: {e:?}");
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::DeviceWriterSync(
                request_response::Event::Message {
                    message:
                        request_response::Message::Response {
                            request_id,
                            response,
                        },
                    ..
                },
            )) => {
                if let Some(pending) = self.pending_device_writer_syncs.remove(&request_id) {
                    info!(
                        "Received device-writer sync response for {} ({} device logs)",
                        pending.identity,
                        response.device_logs.len()
                    );
                    let result = if let Err(error) = response.ensure_supported(
                        crate::protocol::CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION,
                    ) {
                        Err(error)
                    } else if response.authority_records.is_empty() {
                        Err(Self::identity_head_invalid_failure(
                            &pending.identity,
                            "provider returned no device-authority records",
                        ))
                    } else {
                        self.store_verified_device_writer_logs(
                            pending.identity.clone(),
                            response.authority_records,
                            response.device_logs,
                        )
                        .and_then(|_| {
                            self.persist_synced_local_device_writer_state(&pending.identity)
                        })
                        .map_err(|e| {
                            Self::identity_head_invalid_failure(&pending.identity, e.to_string())
                        })
                    };
                    self.on_device_writer_sync_settled(
                        &pending.identity,
                        &pending.provider,
                        result.err(),
                    );
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::DeviceWriterSync(
                request_response::Event::OutboundFailure {
                    request_id, error, ..
                },
            )) => {
                warn!("Outbound device-writer sync request failed: {error}");
                if let Some(pending) = self.pending_device_writer_syncs.remove(&request_id) {
                    self.on_device_writer_sync_settled(
                        &pending.identity,
                        &pending.provider,
                        Some(NetworkError::Protocol(error.to_string())),
                    );
                }
            }

            // --- Relay Record Exchange ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::RelayExchange(
                request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                    ..
                },
            )) => {
                let now = unix_now();
                let response = match request {
                    RelayExchangeRequest::GetRelays {
                        limit,
                        capabilities,
                    } => RelayExchangeResponse::Relays {
                        records: self.relay_records_for_exchange(
                            limit as usize,
                            &capabilities,
                            now,
                        ),
                    },
                    RelayExchangeRequest::AnnounceRelays { records } => {
                        let (accepted, rejected) = self.store_relay_records(records, now);
                        RelayExchangeResponse::Announced { accepted, rejected }
                    }
                    RelayExchangeRequest::GetIdentityHeads { limit } => {
                        RelayExchangeResponse::IdentityHeads {
                            hints: self.identity_head_hints_for_exchange(now, usize::from(limit)),
                        }
                    }
                    RelayExchangeRequest::AnnounceIdentityHeads { hints } => {
                        let (accepted, rejected) = self.record_identity_head_hints(hints, now);
                        RelayExchangeResponse::Announced { accepted, rejected }
                    }
                    RelayExchangeRequest::FindIdentityProviders {
                        query_id,
                        identity,
                        limit,
                        ttl,
                        deadline_unix_ms,
                    } => {
                        self.handle_identity_provider_query(
                            peer,
                            query_id,
                            identity,
                            limit,
                            ttl,
                            deadline_unix_ms,
                            channel,
                        );
                        return;
                    }
                };

                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .relay_exchange
                    .send_response(channel, response)
                {
                    warn!("Failed to send relay exchange response: {e:?}");
                } else {
                    self.mark_relay_peer_success(&peer, now);
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::RelayExchange(
                request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Response {
                            request_id,
                            response,
                        },
                    ..
                },
            )) => match response {
                RelayExchangeResponse::Relays { records } => {
                    let now = unix_now();
                    let (accepted, rejected) = self.store_relay_records(records, now);
                    self.mark_relay_peer_success(&peer, now);
                    debug!(
                        "Stored relay exchange response records: {accepted} accepted, {rejected} rejected"
                    );
                }
                RelayExchangeResponse::Announced { accepted, rejected } => {
                    self.mark_relay_peer_success(&peer, unix_now());
                    debug!("Relay announcement response: {accepted} accepted, {rejected} rejected");
                }
                RelayExchangeResponse::IdentityHeads { hints } => {
                    let now = unix_now();
                    let (accepted, rejected) = self.record_identity_head_hints(hints, now);
                    self.mark_relay_peer_success(&peer, now);
                    debug!(
                        "Stored identity-head gossip response hints: {accepted} accepted, {rejected} rejected"
                    );
                }
                RelayExchangeResponse::IdentityProviders {
                    query_id: _,
                    identity,
                    providers,
                } => {
                    if self.handle_identity_diagnosis_response(request_id, providers.clone()) {
                        return;
                    }

                    if let Some(query_id) =
                        self.pending_identity_provider_forwards.remove(&request_id)
                    {
                        let mut should_respond = false;
                        if let Some(pending) = self
                            .pending_identity_provider_forward_groups
                            .get_mut(&query_id)
                        {
                            debug!(
                                "Received {} forwarded identity provider candidates for {}",
                                providers.len(),
                                identity
                            );
                            pending.providers.extend(providers);
                            pending.remaining_requests =
                                pending.remaining_requests.saturating_sub(1);
                            should_respond = pending.remaining_requests == 0
                                || pending.providers.len() >= pending.limit;
                        }

                        if should_respond {
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
                    } else {
                        debug!(
                            "Received {} identity provider candidates for {}",
                            providers.len(),
                            identity
                        );
                        let providers =
                            self.record_identity_provider_candidates(&identity, providers);
                        if let Some(provider) = providers.first() {
                            self.request_pending_resolves_from_provider(&identity, provider);
                            self.request_pending_device_writer_syncs_from_provider(
                                &identity, provider,
                            );
                        }
                    }
                }
            },

            SwarmEvent::Behaviour(JoltBehaviourEvent::RelayExchange(
                request_response::Event::OutboundFailure {
                    peer,
                    request_id,
                    error,
                    ..
                },
            )) => {
                if self.handle_identity_diagnosis_failure(request_id, error.to_string()) {
                    self.mark_relay_peer_failure(&peer);
                    debug!("Outbound relay exchange failed: {error}");
                    return;
                }

                if let Some(query_id) = self.pending_identity_provider_forwards.remove(&request_id)
                {
                    let mut should_respond = false;
                    if let Some(pending) = self
                        .pending_identity_provider_forward_groups
                        .get_mut(&query_id)
                    {
                        pending.remaining_requests = pending.remaining_requests.saturating_sub(1);
                        should_respond = pending.remaining_requests == 0;
                    }

                    if should_respond {
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
                self.mark_relay_peer_failure(&peer);
                debug!("Outbound relay exchange failed: {error}");
            }

            // --- Ingress Submit ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::IngressSubmit(
                request_response::Event::Message {
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                    peer,
                    ..
                },
            )) => {
                // A remote sender delivering an envelope for our identity. The
                // same validation as the trusted HTTP submit route applies:
                // signature must verify and the envelope must name the local
                // identity as a recipient, or it is rejected.
                let response =
                    match self.submit_ingress(request.receiver_id, request.encrypted_object, request.expires_at)
                    {
                        Ok(record) => {
                            info!(
                                "Queued p2p ingress envelope {} from peer {peer}",
                                record.ingress_id
                            );
                            IngressSubmitResponse::Accepted { record }
                        }
                        Err(e) => {
                            warn!("Rejected p2p ingress envelope from peer {peer}: {e}");
                            IngressSubmitResponse::Rejected {
                                error: e.to_string(),
                            }
                        }
                    };
                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .ingress_submit
                    .send_response(channel, response)
                {
                    warn!("Failed to send ingress-submit response: {e:?}");
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::IngressSubmit(
                request_response::Event::Message {
                    message:
                        request_response::Message::Response {
                            request_id,
                            response,
                        },
                    ..
                },
            )) => {
                if let Some(pending) = self.pending_ingress_submits.remove(&request_id) {
                    let result = match response {
                        IngressSubmitResponse::Accepted { record } => Ok(record),
                        IngressSubmitResponse::Rejected { error } => Err(NetworkError::Protocol(
                            format!("recipient rejected ingress envelope: {error}"),
                        )),
                    };
                    let _ = pending.response_tx.send(result);
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::IngressSubmit(
                request_response::Event::OutboundFailure {
                    request_id, error, ..
                },
            )) => {
                if let Some(mut pending) = self.pending_ingress_submits.remove(&request_id) {
                    // Idle-timeout closes and NAT path migrations kill the
                    // connection between exchanges; a fresh dial usually
                    // succeeds immediately, so retry transient transport
                    // failures instead of surfacing them to the sender. A
                    // protocol mismatch is permanent and fails at once.
                    let retryable = !matches!(
                        error,
                        request_response::OutboundFailure::UnsupportedProtocols
                    );
                    if retryable && pending.attempts_left > 0 {
                        pending.attempts_left -= 1;
                        info!(
                            "Retrying ingress delivery to {} after transport failure ({} attempts left): {error}",
                            pending.peer, pending.attempts_left
                        );
                        self.dispatch_ingress_submit(pending);
                    } else {
                        warn!("Outbound ingress-submit request failed: {error}");
                        let _ = pending.response_tx.send(Err(NetworkError::Protocol(format!(
                            "could not deliver ingress envelope to recipient: {error}"
                        ))));
                    }
                } else {
                    warn!("Outbound ingress-submit request failed: {error}");
                }
            }

            // --- Identify ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::Identify(
                libp2p::identify::Event::Received { peer_id, info, .. },
            )) => {
                debug!(
                    "Identified peer {peer_id}: {} protocols",
                    info.protocols.len()
                );
                for addr in &info.listen_addrs {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());
                }
                self.swarm.add_external_address(info.observed_addr.clone());
            }

            // --- Kademlia ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::Kademlia(event)) => {
                match event {
                    libp2p::kad::Event::OutboundQueryProgressed {
                        id, result, step, ..
                    } => {
                        match result {
                            libp2p::kad::QueryResult::Bootstrap(Ok(ok)) => {
                                info!("DHT bootstrap step: {} remaining", ok.num_remaining);
                            }
                            libp2p::kad::QueryResult::Bootstrap(Err(e)) => {
                                let message = format!("DHT bootstrap error: {e:?}");
                                warn!("{message}");
                                self.last_bootstrap_error = Some(message);
                            }
                            libp2p::kad::QueryResult::StartProviding(Ok(_)) => {
                                info!("DHT provider announcement confirmed");
                            }
                            libp2p::kad::QueryResult::StartProviding(Err(e)) => {
                                warn!("DHT provider announcement FAILED: {e:?}");
                            }
                            libp2p::kad::QueryResult::GetProviders(Ok(ok)) => {
                                match ok {
                                    libp2p::kad::GetProvidersOk::FoundProviders {
                                        key, providers, ..
                                    } => {
                                        let key_str = String::from_utf8_lossy(key.as_ref()).to_string();
                                        let pending_identity = self
                                            .pending_resolves
                                            .keys()
                                            .find(|identity| {
                                                Self::update_log_provider_key(identity) == key_str
                                            })
                                            .cloned();
                                        let pending_device_writer_identity = self
                                            .pending_device_writer_waiters
                                            .keys()
                                            .find(|identity| {
                                                Self::update_log_provider_key(identity) == key_str
                                            })
                                            .cloned();
                                        for provider in providers {
                                            if provider != *self.swarm.local_peer_id() {
                                                info!("DHT found provider {provider} for {key_str}");
                                                self.discovered_providers
                                                    .entry(key_str.clone())
                                                    .or_default()
                                                    .push(provider);

                                                // If fetch manager is waiting for a provider, record it
                                                if self.fetch_manager.is_awaiting_provider(&key_str) {
                                                    let already_connected = self.swarm.connected_peers()
                                                        .any(|p| *p == provider);
                                                    self.fetch_manager
                                                        .on_provider_discovered(&key_str, provider, already_connected);
                                                }

                                                // Dial the provider (iroh handles NAT traversal automatically)
                                                if let Err(e) = self.swarm.dial(provider) {
                                                    debug!("Failed to dial provider {provider}: {e}");
                                                }

                                                if let Some(identity) = &pending_identity {
                                                    self.request_pending_resolves_from_provider(
                                                        identity,
                                                        &provider,
                                                    );
                                                }
                                                if let Some(identity) =
                                                    &pending_device_writer_identity
                                                {
                                                    self.request_pending_device_writer_syncs_from_provider(
                                                        identity,
                                                        &provider,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    libp2p::kad::GetProvidersOk::FinishedWithNoAdditionalRecord {
                                        ..
                                    } => {
                                        info!("DHT provider search finished (no more records)");
                                    }
                                }
                            }
                            libp2p::kad::QueryResult::GetProviders(Err(e)) => {
                                warn!("DHT get_providers error: {e:?}");
                            }
                            _ => {}
                        }
                        if step.last {
                            self.active_update_log_provider_queries
                                .retain(|_, query_id| *query_id != id);
                        }
                    }
                    libp2p::kad::Event::RoutingUpdated { peer, .. } => {
                        debug!("Kademlia routing updated: {peer}");
                    }
                    libp2p::kad::Event::InboundRequest {
                        request:
                            libp2p::kad::InboundRequest::AddProvider {
                                record: Some(record),
                            },
                    } => {
                        let identity = self.identity.identity_id();
                        let key = String::from_utf8_lossy(record.key.as_ref()).to_string();
                        if key == Self::update_log_provider_key(&identity)
                            && record.provider != *self.swarm.local_peer_id()
                        {
                            let providers = self.discovered_providers.entry(key).or_default();
                            if !providers.contains(&record.provider) {
                                providers.push(record.provider);
                            }
                            self.begin_device_writer_sync(
                                super::resolution::DeviceWriterSyncWaiter::LocalRefresh {
                                    identity,
                                },
                            );
                        }
                    }
                    _ => {}
                }
            }

            // --- Swarm-level events ---
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {address}");
            }

            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                let conn_info = PeerConnectionInfo::from_endpoint(&endpoint);
                info!(
                    "Connected to peer: {peer_id} (relayed: {}, transport: {})",
                    conn_info.is_relayed, conn_info.transport
                );
                let remote_addr = conn_info.remote_addr.clone();

                // Track connection quality (keep best: direct > relayed)
                let dominated = self
                    .peer_connections
                    .get(&peer_id)
                    .map(|existing| existing.is_relayed && !conn_info.is_relayed)
                    .unwrap_or(true);
                if dominated {
                    self.peer_connections.insert(peer_id, conn_info);
                }
                let should_exchange_relay_records = self.bootstrap_peer_ids.contains(&peer_id)
                    || self.relay_mesh_peer_ids.contains(&peer_id);
                if should_exchange_relay_records {
                    self.last_bootstrap_error = None;
                    self.start_relay_record_exchange(&peer_id);
                }
                let source = if self.bootstrap_peer_ids.contains(&peer_id) {
                    "bootstrap"
                } else if self.relay_mesh_peer_ids.contains(&peer_id) {
                    "relay_mesh"
                } else {
                    "connection"
                };
                let hint = Self::peer_hint_multiaddr(&remote_addr, peer_id);
                if let Err(e) = self.store.record_discovered_peer_hint(&hint, source) {
                    debug!("Failed to record discovered peer hint {hint}: {e}");
                }

                // Notify fetch manager in case this is a provider we're waiting for
                self.fetch_manager.on_peer_connected(&peer_id);
                let identity = self.identity.identity_id();
                if self.has_device_writer_state(&identity)
                    && self.local_device_sync_candidates.contains(&peer_id)
                {
                    self.refresh_local_device_writer_state_from_candidate(peer_id, false);
                } else if self.has_device_writer_state(&identity) {
                    if let Err(error) = self.announce_update_log_provider(&identity) {
                        debug!("Failed to announce local identity after connection: {error}");
                    }
                }
            }

            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                debug!("Disconnected from peer: {peer_id}");
                if !self.swarm.is_connected(&peer_id) {
                    self.peer_connections.remove(&peer_id);
                }
            }

            SwarmEvent::ExternalAddrConfirmed { address } => {
                info!("External address confirmed: {address}");
            }

            SwarmEvent::NewExternalAddrCandidate { address } => {
                info!("New external address candidate: {address}");
            }

            _ => {}
        }
    }
}
