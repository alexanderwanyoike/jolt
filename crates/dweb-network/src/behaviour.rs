use libp2p::request_response;
use libp2p::swarm::NetworkBehaviour;

use crate::protocol::{ContentRequest, ContentResponse, UpdateLogRequest, UpdateLogResponse};

#[derive(NetworkBehaviour)]
pub struct DwebBehaviour {
    pub mdns: libp2p::mdns::tokio::Behaviour,
    pub content_fetch: request_response::cbor::Behaviour<ContentRequest, ContentResponse>,
    pub update_log_sync: request_response::cbor::Behaviour<UpdateLogRequest, UpdateLogResponse>,
    pub kademlia: libp2p::kad::Behaviour<libp2p::kad::store::MemoryStore>,
    pub identify: libp2p::identify::Behaviour,
}
