# M3 + M5: Daemon Architecture & P2P Networking

**Status: COMPLETE** (2026-03-26)

M3 (daemon architecture) and M5 (internet-wide networking) were delivered together.
jolt nodes now discover each other, connect across NATs, and transfer content
over the internet as a true peer-to-peer mesh.

## What Works

### Daemon Architecture
- Persistent daemon process with `jolt start` / `jolt stop` / `jolt status`
- Command channel architecture: `DaemonCommand` enum over mpsc, `DaemonHandle` for async API
- `FetchManager` state machine: TryingPeers -> QueryingDht -> WaitingForProvider -> FetchingFromProvider
- HTTP API (`jolt-server` crate, axum): health, status, peers, publish, fetch, cache stats/entries, pin/unpin
- CLI as thin client to daemon API

### P2P Networking
- **iroh transport** for automatic NAT traversal (replaces libp2p's broken dcutr stack)
- **Kademlia DHT** for content provider discovery across the internet
- **mDNS** for zero-config LAN peer discovery
- **Content caching** with automatic re-sharing (mesh propagation)
- Fixed UDP port support (`--p2p-port`) for servers behind firewalls

### Validated Across Real Hardware
- **Linux (home NAT, Kenya)** <-> **Hetzner (public IP, Germany)**: direct IPv4 UDP
- **Mac (Safaricom CGNAT, Kenya)** <-> **Hetzner**: direct IPv4 UDP through carrier-grade NAT
- **Mac (CGNAT)** <-> **Linux (home NAT)** via Hetzner as DHT bridge: full mesh content transfer
- All directions work. Content caches and re-shares through the mesh.

### Test Coverage
- 95 tests across all crates
- 7 patchbay NAT simulation tests (Home, Corporate, CGNAT, double NAT topologies)
- Content transfer tests using real NetworkNode instances inside simulated networks
- 3-node DHT discovery test: publish -> announce -> discover -> fetch

## Architecture

```
                    Hetzner (bootstrap)
                   89.167.68.65:4001/udp
                   /p2p/12D3KooWMNy...
                        |
            iroh QUIC   |   iroh QUIC
          (direct UDP)  |  (direct UDP)
                        |
        Linux --------- + --------- Mac
     192.168.1.67              172.20.10.6
     (home NAT)              (Safaricom CGNAT)
```

### Crate Structure
```
jolt-core        ContentId, ContentManifest, types
jolt-identity    Ed25519 keypair, signing, verification
jolt-store       Content store with LRU cache, pinning
jolt-network     NetworkNode, DaemonHandle, FetchManager, behaviours
jolt-server      axum HTTP API
jolt-node        CLI binary, daemon management
```

### Key Dependencies
- `iroh 0.97` - QUIC transport with DERP relay and hole punching
- `libp2p-iroh` - bridge between iroh transport and libp2p protocols (forked: github.com/alexanderwanyoike/libp2p-iroh)
- `libp2p 0.56` - protocol framework (Kademlia, mDNS, request-response, identify)
- `patchbay 0.1` - Linux network namespace NAT simulation (dev dependency)

## Running It

### Start a bootstrap node (public server)
```bash
jolt start --api-bind 0.0.0.0 --no-bootstrap --p2p-port 4001
```
Note the Peer ID from the output.

### Start a client node
```bash
jolt start --bootstrap "/ip4/<SERVER_IP>/udp/4001/p2p/<PEER_ID>"
```

### Publish content
```bash
curl -F "file=@myfile.txt" http://127.0.0.1:9862/api/v1/publish
# Returns: {"content_id": "bafkr4i...", "size": 1234}
```

### Fetch content
```bash
curl -X POST http://127.0.0.1:9862/api/v1/fetch \
  -H 'Content-Type: application/json' \
  -d '{"content_id": "bafkr4i..."}'
# Returns: {"data": [...], "content_id": "bafkr4i...", "size": 1234}
```

### Other endpoints
```
GET  /api/v1/health          - Health check
GET  /api/v1/status          - Node status (peer count, uptime, etc.)
GET  /api/v1/peers           - Connected peer list
GET  /api/v1/cache/stats     - Cache statistics
GET  /api/v1/cache/entries   - List cached content
POST /api/v1/cache/pin/{id}  - Pin content (prevent eviction)
DEL  /api/v1/cache/pin/{id}  - Unpin content
```

## Known Issues

1. **DERP relay substream forwarding**: When direct UDP isn't available, iroh falls back to DERP relay for the initial connection, but request-response protocol substreams don't flow through DERP reliably. Direct UDP (with explicit IP in bootstrap address) is required for content transfer.

2. **macOS IPv4 discovery**: iroh on macOS doesn't try IPv4 direct paths unless the IP is explicitly provided in the bootstrap multiaddr. Always use the full format: `/ip4/x.x.x.x/udp/port/p2p/<peer_id>`.

3. **Daemon event loop stability**: The event loop can hang after prolonged use. The forked libp2p-iroh fixes the actor panic but the underlying cause needs more investigation.

4. **PID file on macOS**: The daemon PID file isn't written correctly on macOS, so `jolt status` and `jolt fetch` CLI commands fail. Use curl to the HTTP API directly as a workaround.

## What's Next (M4)

M4: Update Logs and Mutable Content
- `UpdateLog` (append-only, signed, hash-chained)
- Mutable content resolution (PeerId -> latest content root)
- User profiles
