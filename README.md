# jolt

A peer-to-peer community substrate built in Rust.

Jolt lets creators and users run digital communities without surrendering content, identity, or distribution to a central platform. Nodes discover each other, connect across NATs, and transfer signed content directly. Relays help with availability and discovery, but the user's or community's keys remain the authority.

> Your community, your content, your identity. Distributed by the network, owned by no platform.

## Thesis

Central platforms bundle identity, content hosting, distribution, and community coordination. That is why leaving a platform often means losing the audience, history, updates, files, and access relationships that made the community valuable.

Jolt unbundles those pieces:

| Platform Bundle | Jolt Primitive |
|---|---|
| Identity tied to an account | Identity owned by keys |
| Hosted content | Content-addressed, signed, encrypted when needed |
| Platform distribution | Relays, provider discovery, and peer caching |
| Platform community tools | Spaces, invites, feeds, membership, and app-specific coordination |
| Platform UI | Optional apps/interfaces over the same owned data |

Jolt is not trying to be "the web, but decentralized". The web is good at public pages and global search. Jolt is for private, public, and semi-private communities where authorship, access, distribution, and continuity matter more than platform reach.

HTML can still be a useful view of a Jolt space. The distinction is that signed space state is the authority, while HTML is a browseable rendering: tree structure, links, media, and layout for humans.

Example communities:

- A creator community with signed updates, member content, and portable community history.
- A game community distributing builds, mods, announcements, lobbies, and matchmaking without Steam or Discord owning the graph.
- A research group sharing datasets, notebooks, provenance, usage rights, and member-only access.
- A family or local group keeping a shared archive available without handing it to a cloud platform.
- A project/community publishing releases and announcements through its own identity.

## Current Status

**Transport, content transfer, daemon/API, signed identity addresses, and update-log primitives are in place.** Jolt has validated content flow across three machines, two NATs, and a carrier-grade NAT over iroh P2P. The next proof is higher level: a fresh node should join through bootstrap relays, resolve a `.jolt` community/person address, and fetch signed content while the publisher is offline.

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
- Canonical `{identity}.jolt` addresses
- Signed update-log primitives for mutable identity/community state
- Network request/response path for update-log sync in deterministic tests
- 95 tests including simulated NAT topologies (patchbay)

### Protocol Direction

Jolt separates ownership from availability. A user's or community's key is the authority over identity, content, membership, permissions, and signed state. Relays are replaceable nodes that help content stay reachable by providing discovery, NAT assistance, caching, and owner-directed pinning.

Replication should be owner-directed: the user's node chooses which relays intentionally pin content. Relays may cache what they fetch, but durable relay-to-relay mirroring is a future explicit authorization model, not a v0 default.

The emerging model is:

```text
Identity
  -> owns signed update logs
  -> defines a space/community
  -> grants access and membership
  -> publishes content references

Content
  -> immutable blobs addressed by CID
  -> signed by authors or communities
  -> encrypted when access-controlled

Distribution
  -> bootstrap relays for joining the mesh
  -> DHT/provider discovery for content and update logs
  -> relay pinning for offline availability
  -> peer caching for resilience
```

`.jolt` addresses are identity/content addresses, not first-contact network dial addresses. A fresh node still needs bootstrap relay multiaddrs to enter the mesh. Once connected, it can resolve signed `.jolt` state and fetch content from any authorized provider.

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
dweb publish ./myfile.txt --path /notes/hello --pin-home-relay
```

### Configure A Home Relay

```bash
dweb home-relay set "/ip4/<RELAY_IP>/tcp/<PORT>/p2p/<RELAY_PEER_ID>" \
  --capability pinning \
  --api-url "http://<RELAY_API_HOST>:9862"
dweb home-relay show
dweb home-relay pin <CONTENT_ID>
dweb status
```

The home relay is Alice's delegated availability node. It is stored locally, loaded when the daemon starts, and reported by `dweb status` and `GET /api/v1/status`. The `api-url` is the v0 relay HTTP control endpoint used for owner-signed pin requests.

### Fetch Content

```bash
curl -X POST http://127.0.0.1:9862/api/v1/fetch \
  -H 'Content-Type: application/json' \
  -d '{"content_id": "bafkr4i..."}'
```

### Local Two-Node Dashboard Demo

The default daemon transport is iroh for real P2P and NAT traversal. For a deterministic one-machine demo, use TCP transport and separate data directories:

```bash
cargo build

# Terminal 1: node A
JOLT_A=$(mktemp -d)
XDG_DATA_HOME="$JOLT_A" target/debug/dweb start \
  --transport tcp \
  --p2p-port 4901 \
  --api-port 9871 \
  --no-bootstrap

# Terminal 2: node B
JOLT_B=$(mktemp -d)
XDG_DATA_HOME="$JOLT_B" target/debug/dweb start \
  --transport tcp \
  --p2p-port 4902 \
  --api-port 9872 \
  --no-bootstrap
```

Open the dashboards:

- Node A: http://127.0.0.1:9871/dashboard
- Node B: http://127.0.0.1:9872/dashboard

Connect node B to node A:

```bash
PEER_A=$(curl -sS http://127.0.0.1:9871/api/v1/status \
  | sed -n 's/.*"peer_id":"\([^"]*\)".*/\1/p')

curl -sS -X POST http://127.0.0.1:9872/api/v1/peers/connect \
  -H 'Content-Type: application/json' \
  -d "{\"multiaddr\":\"/ip4/127.0.0.1/tcp/4901/p2p/$PEER_A\"}"
```

Then publish on node A and fetch from node B:

```bash
printf 'hello from node A' > /tmp/jolt-demo.txt
CID=$(curl -sS -F "file=@/tmp/jolt-demo.txt" http://127.0.0.1:9871/api/v1/publish \
  | sed -n 's/.*"content_id":"\([^"]*\)".*/\1/p')

curl -sS -X POST http://127.0.0.1:9872/api/v1/fetch \
  -H 'Content-Type: application/json' \
  -d "{\"content_id\":\"$CID\"}"
```

### API Endpoints

```
GET  /api/v1/health          Health check
GET  /api/v1/status          Node status, peer count, uptime
GET  /api/v1/peers           Connected peer list
POST /api/v1/peers/connect   Dial a peer multiaddr
POST /api/v1/publish         Publish a file (multipart form)
POST /api/v1/fetch           Fetch content by ID
GET  /api/v1/cache/stats     Cache statistics
GET  /api/v1/cache/entries   List cached content
POST /api/v1/cache/pin/{id}  Pin content (prevent eviction)
DEL  /api/v1/cache/pin/{id}  Unpin content
```

### Run Tests

Normal local development should use the deterministic suite:

```bash
./scripts/test-local.sh
```

That script currently runs:

```bash
cargo test --workspace
```

`cargo test --workspace` is expected to be boring and repeatable on a normal developer machine. It includes pure protocol/storage/identity tests, server API tests, CLI tests, and TCP-backed local multi-node network tests. It excludes ignored manual tests for iroh transport smoke checks and patchbay network namespaces.

Test matrix:

| Layer | Command | Default? | Notes |
|---|---|---:|---|
| Deterministic local suite | `./scripts/test-local.sh` | Yes | Normal pre-PR check. |
| Pure crates only | `cargo test -p dweb-core -p dweb-identity -p dweb-store` | Yes | Fast protocol, identity, and store feedback. |
| Local TCP network tests | `cargo test -p dweb-network --lib --tests` | Yes | Uses TCP transport for local determinism. |
| Daemon/API tests | `cargo test -p dweb-node -p dweb-server` | Yes | Covers CLI parsing, daemon config, and HTTP routes. |
| iroh smoke test | `cargo test -p dweb-network new_iroh_creates_node_without_error -- --ignored` | No | Manual because it creates an iroh endpoint and may depend on local network or relay availability. |
| Patchbay topologies | `cargo test -p dweb-network --test nat_traversal -- --ignored` | No | Linux/user-namespace tests for LAN, NAT, CGNAT, and DHT topology simulation. |
| Docker topology harness | `cd tests/docker && bash test-all.sh` | No | Optional/manual harness for old container topology checks. Not part of the normal dev loop. |
| Real-world canary | Public relay/bootstrap plus two client machines on different networks | No | Final confidence check for NAT/CGNAT behavior. |

Manual network checks:

```bash
# Linux network namespace / patchbay topology tests
cargo test -p dweb-network --test nat_traversal -- --ignored

# Manual iroh transport smoke test
cargo test -p dweb-network new_iroh_creates_node_without_error -- --ignored

# Optional Docker topology harness
cd tests/docker && bash test-all.sh
```

Real-world release canary remains the strongest confidence test: a public bootstrap/relay node plus two client machines on different networks, including a CGNAT/mobile network when possible.

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
| M4: Update Logs | Done | Append-only signed logs for mutable content |
| M4.5: Identity Addresses | Done | Canonical `{identity}.jolt` addresses and signed resolver core |
| M5: Internet-Wide P2P | Done | Kademlia DHT, iroh NAT traversal, real hardware validated |
| M6: Bootstrap Relay Mesh | Next | Fresh nodes join global discovery through relay multiaddrs |
| M7: User-Facing `.jolt` Resolution | Next | CLI/API/dashboard resolve and fetch by `.jolt` address |
| M8: Home Relays | Planned | Owner-directed pinning, availability checks, offline publisher flow |
| M9: Encryption | Planned | E2E encryption, group keys, access control |
| M10: Built-In Space Lens | Planned | First application-shaped space demo without a WASM runtime |
| M11: Space Apps / WASM | Planned | Portable executable interfaces over community spaces |
| M12: Streaming | Planned | Chunked transfer, video/audio streaming |
| M13: Redundancy | Planned | Groups of nodes keeping content available |
| M14: Developer SDK | Planned | Rust + JS SDKs, templates, docs |

## Future Applications

- **Creator spaces** -- signed updates, member content, and portable community history.
- **Game communities** -- distribute builds/mods, coordinate lobbies, publish announcements, and match players without a central platform owning the graph.
- **Research spaces** -- datasets, notebooks, provenance, usage rights, and member-only access.
- **Project spaces** -- signed releases, docs, issue artifacts, and community announcements.
- **Private groups** -- family/local/team archives with relay-backed availability and explicit access.

## Design Docs

Detailed technical documentation in [`docs/`](docs/).

## License

MIT
