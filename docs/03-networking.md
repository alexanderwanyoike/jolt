# Networking

## Overview

jolt networking is built on libp2p and iroh, providing peer discovery, content routing, NAT traversal, relays, and encrypted transport. The network has no required central server -- only user nodes and relay nodes.

Relays are ordinary jolt nodes with public reachability and extra responsibilities. They may help with discovery, NAT traversal, content pinning, and serving. They are replaceable carriers, not authorities over identity or content.

## Transport

Nodes communicate over multiple transport protocols, selected based on capability:

| Transport | Use Case |
|---|---|
| QUIC | Primary transport. Fast, multiplexed, encrypted at transport layer. |
| TCP + Noise | Fallback when QUIC is unavailable. |
| WebSocket | For browser-based light clients (future). |

All transports use the Noise protocol for encryption and peer authentication.

## Peer Discovery

### Bootstrap Nodes

On first launch, a node connects to a set of well-known bootstrap nodes to join the DHT. Bootstrap nodes are ordinary jolt nodes that are publicly reachable and have agreed to serve as entry points.

```
Bootstrap list (hardcoded + user-configurable):
  /dns4/bootstrap1.jolt.network/tcp/4001/p2p/QmBootstrap1...
  /dns4/bootstrap2.jolt.network/tcp/4001/p2p/QmBootstrap2...
```

Bootstrap nodes do not have special authority. They only help new nodes discover other peers. Anyone can run a bootstrap node.

Some relays may be discovery-only. A discovery relay helps nodes find peers and provider records, but does not accept content pinning.

### Kademlia DHT

The primary discovery and content routing mechanism. Every node participates in a Kademlia distributed hash table.

The DHT stores:
- **Provider records** -- "I have content with this ContentId"
- **Peer records** -- "This PeerId can be reached at these addresses"
- **Signed peer records** -- "This user's latest update log entry is X" (for mutable content resolution)

```mermaid
sequenceDiagram
    participant Pub as Publisher Node
    participant DHT as Kademlia DHT
    participant Req as Requester Node

    Note over Pub,DHT: Publishing
    Pub->>Pub: ContentId = hash(content)
    Pub->>DHT: "I provide ContentId"
    DHT->>DHT: Store provider record on K closest nodes

    Note over DHT,Req: Fetching
    Req->>DHT: "Who provides ContentId?"
    DHT-->>Req: Provider peer addresses
    Req->>Pub: Direct connection, download content
```

### mDNS (Local Network Discovery)

Nodes on the same LAN discover each other automatically via multicast DNS. This enables:
- Zero-configuration local networking
- Fast transfers between devices on the same network
- Offline operation within a LAN (no internet needed)

### Peer Exchange (PEX)

Connected peers periodically exchange lists of known peers. This helps the network grow organically and reduces reliance on bootstrap nodes.

## NAT Traversal

Most home and mobile networks use NAT, which prevents inbound connections. jolt handles this with multiple strategies:

### 1. QUIC Hole Punching

libp2p's AutoNAT protocol detects whether a node is behind NAT. If so, it attempts UDP hole punching via a coordination relay.

```mermaid
sequenceDiagram
    participant Alice as Alice (behind NAT)
    participant R as Relay Node R
    participant Bob as Bob (behind NAT)

    Alice->>R: Connected
    Bob->>R: Connected
    R->>Alice: Coordinate hole punch
    R->>Bob: Coordinate hole punch
    Alice-->>Bob: Simultaneous connection attempt
    Note over Alice,Bob: Direct QUIC connection established
```

### 2. Relay (Circuit Relay v2)

When hole punching fails, traffic can be relayed through a public node.

```mermaid
graph LR
    Alice["Alice (behind NAT)"] <-->|encrypted| Relay["Relay Node"] <-->|encrypted| Bob["Bob (behind NAT)"]
```

Relay is a fallback, not the default. Relayed connections are:
- Slower (extra hop)
- Limited in bandwidth (relays impose quotas)
- Still end-to-end encrypted (relay cannot read content)

In addition to traffic relay, jolt uses the term relay for delegated availability. A user's home relay can pin that user's signed/encrypted content and announce provider records so the content remains reachable when the user's personal device is offline.

Traffic relay and persistence relay are separate capabilities. A node may offer one, both, or neither.

### 3. UPnP / NAT-PMP

The node attempts to configure port forwarding on the router automatically. Works on many home networks without user intervention.

### Strategy Priority

```mermaid
graph TD
    Start["Connection Attempt"] --> Direct{"Direct connection<br/>possible?"}
    Direct -->|Yes| Done["Connected"]
    Direct -->|No| Punch{"QUIC hole punch<br/>successful?"}
    Punch -->|Yes| Done
    Punch -->|No| Relay["Relay (last resort)"]
    Relay --> Done
```

## Protocols

jolt defines custom libp2p protocols for different operations:

### `/jolt/content/1.0.0` -- Content Fetch

Request and serve content by ContentId.

```mermaid
sequenceDiagram
    participant Req as Requester
    participant DHT as DHT
    participant Prov as Provider

    Req->>DHT: Query providers of ContentId
    DHT-->>Req: Provider addresses
    Req->>Prov: Request { content_id }
    Prov-->>Req: Response { data, signature }
    Req->>Req: Verify hash matches ContentId
    Req->>Req: Cache content locally
```

### `/jolt/pin/1.0.0` -- Relay Pinning

Request that a relay intentionally keep content available.

For v0, pinning is owner-directed: the user's node chooses relays and uploads content to them. Relays do not independently replicate durable copies to other relays.

```
Request:  { owner: PeerId, content_id: ContentId, record_id: Option<ContentId>, signature: Signature }
Response: { accepted: bool, reason: Option<String> }
```

The signature proves that the owner requested this pin. A relay may reject a pin request for any local reason: capacity, policy, unknown user, invalid signature, or unsupported content.

### `/jolt/updatelog/1.0.0` -- Update Log Sync

Synchronize a user's update log (for mutable content resolution).

```mermaid
sequenceDiagram
    participant Req as Requester
    participant Resp as Responder

    Req->>Resp: Request { peer_id, since: sequence_num }
    Resp-->>Req: Response { entries: SignedLogEntry[] }
    Req->>Req: Verify signatures
    Req->>Req: Append to local copy
```

### Deferred App Protocols

Earlier app-platform sketches used names such as `/jolt/appsync/1.0.0` and
`/jolt/message/1.0.0` for app data sync and direct messaging:

```
Request:  { app_id: ContentId, sync_type: SyncType, payload: Vec<u8> }
Response: { payload: Vec<u8> }

SyncType:
  - FullSync: exchange all state
  - Delta: exchange changes since last sync
  - Custom: app-defined protocol
```

```
Request:  { to: PeerId, encrypted_payload: Vec<u8> }
Response: { status: Ack | Queued | Error }
```

Those are not current core protocol commitments. The current direction is in
[Bidirectional Communication and Signed Reachability](19-signed-reachability-endpoints.md):
Jolt should first provide signed reachability metadata and, if needed, generic
app-authorized opaque streams or bounded object ingress. App sync, inboxes,
messages, contacts, and conversation semantics stay above the protocol layer.

## Bandwidth Management

Nodes have configurable limits to prevent abuse:

```toml
[network]
max_connections = 100
max_upload_bandwidth = "10MB/s"    # total upload cap
max_download_bandwidth = "50MB/s"  # total download cap
relay_bandwidth_limit = "1MB/s"    # if acting as relay
cache_serve_limit = "5MB/s"        # bandwidth for serving cached content
```

## Network Resilience

- **No single point of failure.** The DHT is distributed across all nodes.
- **Graceful degradation.** If most nodes go offline, remaining nodes still function.
- **Partition tolerance.** Disconnected clusters operate independently and merge when reconnected.
- **Eclipse attack resistance.** Kademlia's structure makes it difficult for a small number of malicious nodes to isolate a target.
