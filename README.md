# jolt

A peer-to-peer content platform built in Rust. Nodes discover each other, connect across NATs, and transfer content directly -- no central servers, no middlemen. Content spreads through the network via caching: every fetch makes the mesh more resilient.

> Your node, your data. Connected to everyone, controlled by no one.

## Current Status

**Milestones 1-3 + 5 complete.** Validated across three machines, two NATs, and a carrier-grade NAT -- content flowing in every direction over iroh P2P.

```
                  Bootstrap Node
                  (public server)
                        |
            iroh QUIC   |   iroh QUIC
          (direct UDP)  |  (direct UDP)
                        |
       Node A --------- + --------- Node B
     (home NAT)                   (mobile CGNAT)
```

### What Works
- P2P content transfer across the internet (NAT, CGNAT, direct)
- Kademlia DHT for content discovery
- mDNS for zero-config LAN discovery
- Content caching with automatic re-sharing (mesh propagation)
- Daemon architecture with HTTP API
- 95 tests including simulated NAT topologies (patchbay)

## Quick Start

### Prerequisites

- Rust 1.89+

### Build

```bash
cargo build --release
```

### Run a Node

```bash
# Start as a bootstrap node (public server with fixed UDP port)
./target/release/dweb start --no-bootstrap --p2p-port 4001 --api-bind 0.0.0.0

# Start a client node (connects to bootstrap)
./target/release/dweb start \
  --bootstrap "/ip4/<BOOTSTRAP_IP>/udp/<PORT>/p2p/<BOOTSTRAP_PEER_ID>"
```

### Publish Content

```bash
curl -F "file=@myfile.txt" http://127.0.0.1:9862/api/v1/publish
# {"content_id": "bafkr4i...", "size": 1234}
```

### Fetch Content

```bash
curl -X POST http://127.0.0.1:9862/api/v1/fetch \
  -H 'Content-Type: application/json' \
  -d '{"content_id": "bafkr4i..."}'
```

### API Endpoints

```
GET  /api/v1/health          Health check
GET  /api/v1/status          Node status, peer count, uptime
GET  /api/v1/peers           Connected peer list
POST /api/v1/publish         Publish a file (multipart form)
POST /api/v1/fetch           Fetch content by ID
GET  /api/v1/cache/stats     Cache statistics
GET  /api/v1/cache/entries   List cached content
POST /api/v1/cache/pin/{id}  Pin content (prevent eviction)
DEL  /api/v1/cache/pin/{id}  Unpin content
```

### Run Tests

```bash
# Unit + integration tests
cargo test -p dweb-network --lib
cargo test -p dweb-network --test nat_traversal
cargo test -p dweb-network --test dht_integration
cargo test -p dweb-network --test cache_integration

# All crates
cargo test -p dweb-core
cargo test -p dweb-identity
cargo test -p dweb-store
cargo test -p dweb-node
```

## Architecture

```
dweb node
  +-- HTTP API (axum, localhost:9862)
  +-- Daemon Loop (tokio::select!)
  |     +-- FetchManager (state machine)
  |     +-- Command Channel (mpsc)
  +-- Identity (Ed25519 keypair)
  +-- Content Store (publish + LRU cache + pinning)
  +-- P2P Network
        +-- iroh transport (QUIC, DERP relay, hole punching)
        +-- Kademlia DHT (content provider discovery)
        +-- mDNS (LAN peer discovery)
        +-- request-response (content fetch protocol)
        +-- identify (peer protocol exchange)
```

### Crate Structure

| Crate | Purpose |
|---|---|
| `dweb-core` | Content addressing (SHA-256 + CIDv1), shared types |
| `dweb-identity` | Ed25519 keypair management, signing, verification |
| `dweb-store` | Content store with LRU cache, pinning, eviction |
| `dweb-network` | NetworkNode, DaemonHandle, FetchManager, P2P behaviours |
| `dweb-server` | axum HTTP API server |
| `dweb-node` | CLI binary and daemon management |

## Roadmap

| Milestone | Status | Description |
|---|---|---|
| M1: Two Nodes Talking | Done | mDNS discovery, content-addressed file exchange, signatures |
| M2: Caching | Done | LRU cache, pinning, serve cached content, re-sharing |
| M3: Daemon + API | Done | Persistent daemon, HTTP API, CLI thin client |
| M4: Update Logs | Next | Append-only signed logs for mutable content |
| M5: Internet-Wide P2P | Done | Kademlia DHT, iroh NAT traversal, real hardware validated |
| M6: Encryption | Planned | E2E encryption, group keys, access control |
| M7: WASM Runtime | Planned | wasmtime sandbox, host API, permissions |
| M8: App Lifecycle | Planned | Install, update, remove apps from the network |
| M9: Host API | Planned | Network + identity APIs for WASM apps |
| M10: Streaming | Planned | Chunked transfer, video/audio streaming |
| M11: Redundancy | Planned | Groups of nodes keeping content available |
| M12: Developer SDK | Planned | Rust + JS SDKs, templates, docs |

## Future Applications

- **jolt-video** -- YouTube without YouTube. Viewers become seeders.
- **jolt-chat** -- E2E encrypted messaging. No phone number required.
- **jolt-blog** -- Personal publishing. Your blog, your rules, forever.
- **jolt-drive** -- File storage and sharing. No cloud required.
- **jolt-social** -- Social feed without the algorithm.

## Design Docs

Detailed technical documentation in [`docs/`](docs/).

## License

MIT
