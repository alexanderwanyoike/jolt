# dweb

A decentralized peer-to-peer web platform built in Rust. Every user runs a node that serves as their personal server, app runtime, and data store. Apps are distributed as WASM binaries, installed locally, and connect users directly -- no corporations, no central servers, no middlemen.

> Your node, your apps, your data. Connected to everyone, controlled by no one.

## Why?

- **Corporations own your data.** Your posts, messages, and files live on servers you don't control.
- **Content is walled in platforms.** Your audience on one platform doesn't transfer to another.
- **Single points of failure.** If a platform goes down, nobody can use it.
- **No real privacy.** Your data is a breach or a subpoena away from exposure.
- **Creators get taxed.** Platforms take 15-45% of creator revenue.

## How dweb works

1. **Apps are installed, not visited.** Like mobile apps, dweb apps are WASM binaries that download to your node and run locally. If the developer disappears, the app still works.
2. **Data stays with its owner.** Your messages, files, and content live on your machine. Always.
3. **Content spreads through the network.** Public content is cached by nodes that access it. Popular content becomes more available, not less.
4. **Users connect directly.** No server in the middle. Peer-to-peer, encrypted by default.

## Current Status

**Milestone 1 complete** -- two nodes on a LAN discover each other via mDNS, exchange content-addressed files, and verify both hash integrity and Ed25519 signatures.

### Architecture

```
dweb node
  +-- Browser UI (localhost)
  +-- HTTP Server (axum)
  +-- Node Runtime
  |     +-- App Manager
  |     +-- Identity (Ed25519 keypair)
  |     +-- Content Manager
  |     +-- WASM Runtime (wasmtime)
  |     +-- Data Store (per-app isolated)
  |     +-- Crypto (encryption / key exchange)
  +-- P2P Network (libp2p)
        +-- Discovery (DHT + mDNS)
        +-- Transport (QUIC + TCP)
        +-- NAT Traversal
        +-- Protocols (content fetch, app sync, messaging)
```

### Crate Structure

| Crate | Purpose |
|---|---|
| `dweb-core` | Content addressing (BLAKE3 + CIDv1), shared types |
| `dweb-identity` | Ed25519 keypair management, signing, verification |
| `dweb-network` | libp2p node, mDNS discovery, content fetch protocol |
| `dweb-node` | CLI entry point (`dweb start`, `dweb publish`, `dweb fetch`) |

## Quick Start

### Prerequisites

- Rust 1.75+

### Build

```bash
cargo build
```

### Usage

Publish a file and start serving it:

```bash
# Publish a file (stores locally and prints the ContentId)
cargo run -- publish ~/my-file.txt

# Start the node (serves published content to the network)
cargo run -- start
```

Fetch content from another node on the same LAN:

```bash
# In another terminal (or another machine on the same network)
cargo run -- fetch <content-id>
```

### Run Tests

```bash
cargo test --workspace
```

30 tests covering content addressing, identity management, P2P networking (including a two-node integration test), and CLI parsing.

## Roadmap

| Milestone | Status | Description |
|---|---|---|
| M1: Two Nodes Talking | Done | mDNS discovery, content-addressed file exchange, Ed25519 signatures |
| M2: Caching | Planned | LRU cache, pinning, serve cached content to other peers |
| M3: Browser UI | Planned | axum HTTP server, REST API, `dweb://` protocol handler |
| M4: Update Logs | Planned | Append-only signed logs for mutable content |
| M5: DHT Networking | Planned | Kademlia DHT, NAT traversal, internet-wide discovery |
| M6: Encryption | Planned | E2E encryption, group keys, access control |
| M7: WASM Runtime | Planned | wasmtime sandbox, host API, capability-based permissions |
| M8: App Lifecycle | Planned | Install, update, remove apps from the network |
| M9: Host API | Planned | Network + identity APIs for WASM apps |
| M10: Streaming | Planned | Chunked file transfer, video/audio streaming |
| M11: Redundancy | Planned | Groups of nodes keeping each other's content available |
| M12: Developer SDK | Planned | Rust + JS SDKs, app templates, documentation |

## Example Applications (Future)

- **dweb-video** -- YouTube without YouTube. Viewers become seeders.
- **dweb-chat** -- E2E encrypted messaging. No phone number required.
- **dweb-blog** -- Personal publishing. Your blog, your rules, forever.
- **dweb-drive** -- File storage and sharing. Dropbox without the cloud.
- **dweb-market** -- P2P marketplace. No platform fees.
- **dweb-social** -- Social feed without the algorithm.

## Design Docs

Detailed technical documentation is in the [`docs/`](docs/) directory:

- [Vision](docs/00-vision.md)
- [Architecture](docs/01-architecture.md)
- [Identity and Cryptography](docs/02-identity-and-crypto.md)
- [Networking](docs/03-networking.md)
- [WASM Runtime](docs/04-wasm-runtime.md)
- [Data Model](docs/05-data-model.md)
- [Application System](docs/06-apps.md)
- [Content Distribution](docs/07-content-distribution.md)
- [Access Control](docs/08-access-control.md)
- [Milestones](docs/09-milestones.md)
- [Example Apps](docs/10-example-apps.md)

## License

MIT
