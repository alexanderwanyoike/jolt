use std::str::FromStr;
use std::time::Instant;

use tracing::info;

use jolt_core::{verify_update_log_for_identity, ContentId, JoltAddress, PinRequest};

use crate::command::{
    CacheEntryInfo, CacheStatsResponse, DaemonCommand, FetchResult, PeerConnectResponse, PeerInfo,
    PublishResponse,
};
use crate::error::NetworkError;
use crate::protocol::{ContentRequest, UpdateLogRequest};

use super::{NetworkNode, PendingResolve, PendingUpdateLogPin};

impl NetworkNode {
    /// Handle a single daemon command.
    pub(super) fn handle_command(&mut self, command: DaemonCommand) {
        match command {
            DaemonCommand::Publish {
                file_path,
                path,
                response_tx,
            } => {
                let size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);
                let result = match path {
                    Some(path) => self.publish_file_at_path(&file_path, &path).map(
                        |(content_id, address, latest_sequence)| PublishResponse {
                            content_id: content_id.to_string(),
                            size,
                            path: Some(address.path().to_string()),
                            address: Some(address.to_string()),
                            latest_sequence: Some(latest_sequence),
                        },
                    ),
                    None => self
                        .publish_file(&file_path)
                        .map(|content_id| PublishResponse {
                            content_id: content_id.to_string(),
                            size,
                            path: None,
                            address: None,
                            latest_sequence: None,
                        }),
                };
                let _ = response_tx.send(result);
            }
            DaemonCommand::Fetch {
                content_id,
                response_tx,
            } => {
                // Check local store first
                if let Some(content_data) = self.store.get_content(&content_id) {
                    let _ = response_tx.send(Ok(FetchResult {
                        data: content_data.data.clone(),
                        content_id: content_id.clone(),
                        size: content_data.data.len() as u64,
                    }));
                    return;
                }

                // Try connected peers first
                let peers: Vec<_> = self.swarm.connected_peers().cloned().collect();
                info!(
                    "Fetch {content_id}: {} connected peers, querying them first",
                    peers.len()
                );
                let mut request_ids = Vec::new();
                let request = ContentRequest {
                    content_id: content_id.clone(),
                };
                for peer in &peers {
                    let req_id = self
                        .swarm
                        .behaviour_mut()
                        .content_fetch
                        .send_request(peer, request.clone());
                    request_ids.push(req_id);
                }

                // Also start DHT provider query
                info!("Fetch {content_id}: starting DHT provider query");
                let key = libp2p::kad::RecordKey::new(&content_id.clone().into_bytes());
                self.swarm.behaviour_mut().kademlia.get_providers(key);

                self.fetch_manager
                    .start_fetch(content_id, response_tx, request_ids);
            }
            DaemonCommand::Resolve {
                address,
                response_tx,
            } => {
                let address = match JoltAddress::from_str(&address) {
                    Ok(address) => address,
                    Err(e) => {
                        let _ = response_tx.send(Err(NetworkError::InvalidInput(e.to_string())));
                        return;
                    }
                };

                let fallback_response = self
                    .resolve_response_from_cache(&address, None, "cache")
                    .ok();
                let identity = address.identity().clone();

                if let Some(response) = fallback_response.clone() {
                    let _ = response_tx.send(Ok(response));
                    if self.should_refresh_cached_resolution(&identity) {
                        self.find_update_log_providers(&identity);
                        if let Some(provider) = self.take_discovered_update_log_provider(&identity)
                        {
                            self.request_daemon_refresh_from_provider(address, &provider);
                        }
                    }
                    return;
                }

                self.find_update_log_providers(&identity);

                if let Some(provider) = self.take_discovered_update_log_provider(&identity) {
                    self.request_daemon_resolve_from_provider(
                        address,
                        None,
                        &provider,
                        response_tx,
                        fallback_response,
                    );
                } else {
                    self.pending_resolves
                        .entry(identity)
                        .or_default()
                        .push(PendingResolve {
                            address,
                            response_tx,
                            deadline: Instant::now() + self.resolve_timeout,
                            fallback_response,
                        });
                }
            }
            DaemonCommand::DiagnoseIdentity {
                identity,
                response_tx,
            } => {
                self.diagnose_identity(identity, response_tx);
            }
            DaemonCommand::EnsureLocalIdentityEncryptionKey { response_tx } => {
                let result = self.ensure_local_identity_encryption_key();
                let _ = response_tx.send(result);
            }
            DaemonCommand::EncryptObject {
                plaintext,
                content_type,
                recipients,
                response_tx,
            } => {
                let result = self.encrypt_object(plaintext, content_type, recipients);
                let _ = response_tx.send(result);
            }
            DaemonCommand::DecryptObject {
                encrypted_object,
                response_tx,
            } => {
                let result = self.decrypt_object(&encrypted_object);
                let _ = response_tx.send(result);
            }
            DaemonCommand::PublishReachability {
                sequence_hint,
                expires_at,
                live,
                offline_ingress,
                response_tx,
            } => {
                let result =
                    self.publish_reachability(sequence_hint, expires_at, live, offline_ingress);
                let _ = response_tx.send(result);
            }
            DaemonCommand::SubmitIngress {
                receiver_id,
                encrypted_object,
                expires_at,
                response_tx,
            } => {
                let result = self.submit_ingress(receiver_id, encrypted_object, expires_at);
                let _ = response_tx.send(result);
            }
            DaemonCommand::ListPendingIngress { response_tx } => {
                let _ = response_tx.send(Ok(self.list_pending_ingress()));
            }
            DaemonCommand::OpenIngress {
                ingress_id,
                response_tx,
            } => {
                let result = self.open_ingress(&ingress_id);
                let _ = response_tx.send(result);
            }
            DaemonCommand::AcceptIngress {
                ingress_id,
                response_tx,
            } => {
                let result = self.accept_ingress(&ingress_id);
                let _ = response_tx.send(result);
            }
            DaemonCommand::RejectIngress {
                ingress_id,
                response_tx,
            } => {
                let result = self.reject_ingress(&ingress_id);
                let _ = response_tx.send(result);
            }
            DaemonCommand::ConnectPeer {
                multiaddr,
                response_tx,
            } => {
                let result =
                    self.connect_peer_multiaddr(&multiaddr)
                        .map(|peer_id| PeerConnectResponse {
                            peer_id: peer_id.map(|p| p.to_string()),
                            multiaddr,
                        });
                let _ = response_tx.send(result);
            }
            DaemonCommand::GetStatus { response_tx } => {
                let _ = response_tx.send(self.build_status());
            }
            DaemonCommand::GetPeers { response_tx } => {
                let peers = self
                    .swarm
                    .connected_peers()
                    .map(|p| {
                        let conn = self.peer_connections.get(p);
                        PeerInfo {
                            peer_id: p.to_string(),
                            is_relayed: conn.map(|c| c.is_relayed).unwrap_or(false),
                            transport: conn
                                .map(|c| c.transport.clone())
                                .unwrap_or_else(|| self.transport_name.to_string()),
                            remote_addr: conn.map(|c| c.remote_addr.clone()).unwrap_or_default(),
                        }
                    })
                    .collect();
                let _ = response_tx.send(peers);
            }
            DaemonCommand::GetCacheStats { response_tx } => {
                let stats = self.store.stats();
                let _ = response_tx.send(CacheStatsResponse {
                    total_cached: stats.total_cached,
                    total_published: stats.total_published,
                    cached_items: stats.cached_items,
                    published_items: stats.published_items,
                    pinned_items: stats.pinned_items,
                    pinned_size: stats.pinned_size,
                    max_size: stats.max_size,
                    available: stats.available,
                });
            }
            DaemonCommand::ListCacheEntries { response_tx } => {
                let entries = self
                    .store
                    .list_entries()
                    .into_iter()
                    .map(|e| CacheEntryInfo {
                        content_id: e.content_id.clone(),
                        size: e.size,
                        cached_at: e.cached_at,
                        last_accessed: e.last_accessed,
                        pinned: e.pinned,
                    })
                    .collect();
                let _ = response_tx.send(entries);
            }
            DaemonCommand::ListPublishedContent { response_tx } => {
                let _ = response_tx.send(self.published_content_inventory());
            }
            DaemonCommand::Pin {
                content_id,
                response_tx,
            } => {
                let result = self
                    .store
                    .pin(&content_id)
                    .map_err(|e| NetworkError::Protocol(e.to_string()))
                    .and_then(|_| {
                        let content_id = ContentId::from_str(&content_id)
                            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
                        self.announce_provider(&content_id)
                    });
                let _ = response_tx.send(result);
            }
            DaemonCommand::CreatePinRequest {
                content_id,
                response_tx,
            } => {
                let result = ContentId::from_str(&content_id)
                    .map_err(|e| NetworkError::InvalidInput(e.to_string()))
                    .and_then(|parsed| {
                        if !self
                            .store
                            .published_ids()
                            .iter()
                            .any(|published| published == &content_id)
                        {
                            return Err(NetworkError::InvalidInput(format!(
                                "content is not locally published: {content_id}"
                            )));
                        }

                        let update_log_content_id =
                            self.publish_update_log_snapshot(&self.identity.identity_id())?;

                        PinRequest::with_update_log(
                            self.identity.public_key_bytes(),
                            parsed,
                            update_log_content_id,
                            |bytes| self.identity.sign(bytes),
                        )
                        .map_err(|e| NetworkError::Protocol(e.to_string()))
                    });
                let _ = response_tx.send(result);
            }
            DaemonCommand::RecordHomeRelayPin {
                content_id,
                path,
                relay,
                latest_sequence,
                response_tx,
            } => {
                let result = self.record_home_relay_pin(&content_id, path, relay, latest_sequence);
                let _ = response_tx.send(result);
            }
            DaemonCommand::UpdateNetworkSettings {
                configured_bootstrap_relays,
                effective_bootstrap_relays,
                home_relay,
                response_tx,
            } => {
                self.bootstrap_peer_ids =
                    Self::parse_bootstrap_peer_ids(&effective_bootstrap_relays);
                self.configured_bootstrap_relays = configured_bootstrap_relays;
                self.effective_bootstrap_relays = effective_bootstrap_relays;
                self.home_relay = home_relay;
                let _ = response_tx.send(Ok(()));
            }
            DaemonCommand::PinUpdateLog {
                identity,
                response_tx,
            } => {
                if let Some(entries) = self.update_logs.get(&identity) {
                    let result = verify_update_log_for_identity(&identity, entries)
                        .map_err(|e| NetworkError::Protocol(e.to_string()))
                        .and_then(|sequence| {
                            self.announce_update_log_provider(&identity)?;
                            Ok(sequence)
                        });
                    let _ = response_tx.send(result);
                    return;
                }

                let peers: Vec<_> = self.swarm.connected_peers().cloned().collect();
                let Some(peer) = peers.first() else {
                    let _ = response_tx.send(Err(NetworkError::NoPeers));
                    return;
                };

                let request = UpdateLogRequest {
                    identity: identity.clone(),
                    since: None,
                };
                let request_id = self
                    .swarm
                    .behaviour_mut()
                    .update_log_sync
                    .send_request(peer, request);
                self.pending_update_log_pins.insert(
                    request_id,
                    PendingUpdateLogPin {
                        identity,
                        response_tx,
                    },
                );
            }
            DaemonCommand::StoreUpdateLog {
                identity,
                entries,
                response_tx,
            } => {
                let result = self
                    .store_verified_update_log(identity.clone(), entries)
                    .and_then(|_| {
                        let entries = self.update_logs.get(&identity).ok_or_else(|| {
                            NetworkError::Protocol(format!(
                                "No verified update log cached for {identity}"
                            ))
                        })?;
                        let sequence = verify_update_log_for_identity(&identity, entries)
                            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
                        self.announce_update_log_provider(&identity)?;
                        Ok(sequence)
                    });
                let _ = response_tx.send(result);
            }
            DaemonCommand::Unpin {
                content_id,
                response_tx,
            } => {
                let result = self
                    .store
                    .unpin(&content_id)
                    .map_err(|e| NetworkError::Protocol(e.to_string()));
                let _ = response_tx.send(result);
            }
            DaemonCommand::Shutdown { response_tx } => {
                let _ = response_tx.send(());
            }
        }
    }
}
