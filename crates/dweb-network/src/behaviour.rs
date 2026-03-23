use libp2p::request_response;
use libp2p::swarm::NetworkBehaviour;

use crate::protocol::{ContentRequest, ContentResponse};

#[derive(NetworkBehaviour)]
pub struct DwebBehaviour {
    pub mdns: libp2p::mdns::tokio::Behaviour,
    pub content_fetch: request_response::cbor::Behaviour<ContentRequest, ContentResponse>,
    pub kademlia: libp2p::kad::Behaviour<libp2p::kad::store::MemoryStore>,
    pub identify: libp2p::identify::Behaviour,
    pub autonat: libp2p::autonat::Behaviour,
    pub relay_client: libp2p::relay::client::Behaviour,
    pub relay_server: libp2p::relay::Behaviour,
    pub dcutr: libp2p::dcutr::Behaviour,
    pub upnp: libp2p::upnp::tokio::Behaviour,
}
