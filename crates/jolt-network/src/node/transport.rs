use std::time::Duration;

use libp2p::request_response::{self, ProtocolSupport};
use libp2p::{StreamProtocol, Swarm, Transport};

use jolt_identity::NodeIdentity;

use crate::behaviour::JoltBehaviour;
use crate::config::NetworkConfig;
use crate::error::NetworkError;

const DEVICE_WRITER_SYNC_PROTOCOLS: [&str; 2] =
    ["/jolt/device-writer/2.0.0", "/jolt/device-writer/1.0.0"];

pub(super) struct BuiltTransport {
    pub swarm: Swarm<JoltBehaviour>,
    pub iroh_endpoint: Option<iroh::Endpoint>,
    pub transport_name: &'static str,
}

pub(super) async fn build_iroh_swarm(
    identity: &NodeIdentity,
    config: &NetworkConfig,
) -> Result<BuiltTransport, NetworkError> {
    let libp2p_keypair = identity.to_libp2p_keypair();
    let peer_id = libp2p_keypair.public().to_peer_id();

    let transport = if config.p2p_port > 0 {
        tracing::info!(
            "Binding iroh transport to fixed UDP port {}",
            config.p2p_port
        );
        libp2p_iroh::Transport::new_with_port(Some(&libp2p_keypair), config.p2p_port)
            .await
            .map_err(|e| NetworkError::Swarm(format!("Failed to create iroh transport: {e}")))?
    } else {
        libp2p_iroh::Transport::new(Some(&libp2p_keypair))
            .await
            .map_err(|e| NetworkError::Swarm(format!("Failed to create iroh transport: {e}")))?
    };

    let iroh_endpoint = transport.endpoint().ok();
    let behaviour = build_behaviour(peer_id, &libp2p_keypair, config.enable_mdns)?;
    let swarm = Swarm::new(
        transport.boxed(),
        behaviour,
        peer_id,
        libp2p::swarm::Config::with_tokio_executor()
            .with_idle_connection_timeout(Duration::from_secs(300)),
    );

    Ok(BuiltTransport {
        swarm,
        iroh_endpoint,
        transport_name: "iroh",
    })
}

pub(super) fn build_tcp_swarm(
    identity: &NodeIdentity,
    config: &NetworkConfig,
) -> Result<BuiltTransport, NetworkError> {
    let libp2p_keypair = identity.to_libp2p_keypair();
    let peer_id = libp2p_keypair.public().to_peer_id();

    let transport = libp2p::tcp::tokio::Transport::default()
        .upgrade(libp2p::core::upgrade::Version::V1)
        .authenticate(
            libp2p::noise::Config::new(&libp2p_keypair)
                .map_err(|e| NetworkError::Swarm(e.to_string()))?,
        )
        .multiplex(libp2p::yamux::Config::default())
        .boxed();

    let behaviour = build_behaviour(peer_id, &libp2p_keypair, config.enable_mdns)?;
    let swarm = Swarm::new(
        transport,
        behaviour,
        peer_id,
        libp2p::swarm::Config::with_tokio_executor()
            .with_idle_connection_timeout(Duration::from_secs(300)),
    );

    Ok(BuiltTransport {
        swarm,
        iroh_endpoint: None,
        transport_name: "tcp",
    })
}

fn build_behaviour(
    peer_id: libp2p::PeerId,
    libp2p_keypair: &libp2p::identity::Keypair,
    enable_mdns: bool,
) -> Result<JoltBehaviour, NetworkError> {
    let mdns = if enable_mdns {
        Some(
            libp2p::mdns::tokio::Behaviour::new(libp2p::mdns::Config::default(), peer_id)
                .map_err(|e| NetworkError::Swarm(e.to_string()))?,
        )
    } else {
        None
    }
    .into();

    let content_fetch = request_response::cbor::Behaviour::new(
        [(
            StreamProtocol::new("/jolt/content/1.0.0"),
            ProtocolSupport::Full,
        )],
        request_response::Config::default(),
    );
    let update_log_sync = request_response::cbor::Behaviour::new(
        [(
            StreamProtocol::new("/jolt/update-log/1.0.0"),
            ProtocolSupport::Full,
        )],
        request_response::Config::default(),
    );
    let device_writer_sync = request_response::cbor::Behaviour::new(
        DEVICE_WRITER_SYNC_PROTOCOLS
            .map(|protocol| (StreamProtocol::new(protocol), ProtocolSupport::Full)),
        request_response::Config::default(),
    );
    let relay_exchange = request_response::cbor::Behaviour::new(
        [(
            StreamProtocol::new("/jolt/relays/1.0.0"),
            ProtocolSupport::Full,
        )],
        request_response::Config::default(),
    );
    let ingress_submit = request_response::cbor::Behaviour::new(
        [(
            StreamProtocol::new("/jolt/ingress/1.0.0"),
            ProtocolSupport::Full,
        )],
        request_response::Config::default(),
    );

    let mut kad_config = libp2p::kad::Config::new(StreamProtocol::new("/jolt/kad/1.0.0"));
    kad_config.set_query_timeout(Duration::from_secs(60));
    let kad_store = libp2p::kad::store::MemoryStore::new(peer_id);
    let kademlia = libp2p::kad::Behaviour::with_config(peer_id, kad_store, kad_config);

    let identify = libp2p::identify::Behaviour::new(libp2p::identify::Config::new(
        "/jolt/id/1.0.0".to_string(),
        libp2p_keypair.public(),
    ));

    Ok(JoltBehaviour {
        mdns,
        content_fetch,
        update_log_sync,
        device_writer_sync,
        relay_exchange,
        ingress_submit,
        kademlia,
        identify,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_writer_sync_advertises_current_then_legacy_protocol() {
        assert_eq!(
            DEVICE_WRITER_SYNC_PROTOCOLS,
            ["/jolt/device-writer/2.0.0", "/jolt/device-writer/1.0.0"]
        );
    }
}
