use libp2p::request_response;
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::swarm::NetworkBehaviour;

use crate::protocol::{
    ContentRequest, ContentResponse, RelayExchangeRequest, RelayExchangeResponse, UpdateLogRequest,
    UpdateLogResponse,
};

#[derive(NetworkBehaviour)]
pub struct JoltBehaviour {
    pub mdns: Toggle<libp2p::mdns::tokio::Behaviour>,
    pub content_fetch: request_response::cbor::Behaviour<ContentRequest, ContentResponse>,
    pub update_log_sync: request_response::cbor::Behaviour<UpdateLogRequest, UpdateLogResponse>,
    pub relay_exchange:
        request_response::cbor::Behaviour<RelayExchangeRequest, RelayExchangeResponse>,
    pub kademlia: libp2p::kad::Behaviour<libp2p::kad::store::MemoryStore>,
    pub identify: libp2p::identify::Behaviour,
}
