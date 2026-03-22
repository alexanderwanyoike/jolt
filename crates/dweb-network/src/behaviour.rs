use libp2p::request_response;
use libp2p::swarm::NetworkBehaviour;

use crate::protocol::{ContentRequest, ContentResponse};

#[derive(NetworkBehaviour)]
pub struct DwebBehaviour {
    pub mdns: libp2p::mdns::tokio::Behaviour,
    pub content_fetch: request_response::cbor::Behaviour<ContentRequest, ContentResponse>,
}
