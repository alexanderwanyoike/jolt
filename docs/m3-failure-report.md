# M3 Report: Daemon Architecture & P2P Network Testing

## Real-World Test Results (3 Machines: Kenya, Germany, Mac)

### What Works

**LAN Direct P2P -- CONFIRMED**
- Mac (`192.168.1.68`) and Kenya machine (`192.168.1.67`) on the same WiFi
- mDNS discovers peers instantly, connection is `relayed: false`
- Content fetch completes in <20ms, direct peer-to-peer
- Content auto-cached for re-sharing
- No bootstrap needed for LAN discovery (but DHT works in parallel)

**DHT Provider Discovery -- CONFIRMED**
- Content published on Kenya machine, Mac finds provider via DHT through Hetzner bootstrap
- Provider found in <1s across the internet
- Works regardless of network topology

**Relay-Based Transfer -- CONFIRMED**
- When peers are on different networks (cross-NAT), content transfers via relay
- Data arrives correctly but goes through bootstrap (not true P2P for cross-NAT)

### What Doesn't Work

**Cross-NAT Direct Connection -- FAILED**
- When Mac is on mobile hotspot (`172.20.10.x`) and Kenya on home WiFi (`192.168.1.x`)
- DHT finds provider, but `Dialing provider via relay circuit` never results in connection
- FetchManager stuck in `WaitingForProvider` until timeout
- dcutr hole punching fails: "Giving up after 3 dial attempts"

### Critical Discovery: QUIC vs TCP Bootstrap

Using QUIC bootstrap (`/udp/.../quic-v1/`) instead of TCP (`/tcp/`) makes the identify protocol discover UDP external addresses. With TCP bootstrap, only TCP addresses are discovered, and TCP hole punching almost never works through NAT. **Always use QUIC bootstrap addresses.**

### Remaining Problem: Relay Circuit Dial

The relay circuit dial from the FetchManager flow doesn't complete when peers are on different networks. The code dials `provider via relay circuit` but no `Connected to peer` event fires. This needs investigation -- the relay reservation is confirmed, the bootstrap relay is running, but the circuit connection from Mac to Kenya via relay doesn't establish.

---

## What Was Built

### Daemon Architecture (Phases 1-4) - Working
- Command channel infrastructure: `DaemonCommand` enum, `DaemonHandle` (mpsc channel), `run_daemon_loop()` with `tokio::select!`
- `FetchManager` state machine: `TryingPeers` -> `QueryingDht` -> `WaitingForProvider` -> `FetchingFromProvider`
- `dweb-server` crate: axum HTTP API with 10 endpoints (health, status, peers, publish, fetch, cache stats/entries, pin/unpin)
- CLI refactored: `dweb start` launches daemon + HTTP server, `dweb stop/status`, thin-client `publish/fetch/cache`
- 100 unit/integration tests passing

### P2P Networking (Phase 5) - FAILED
The network does not achieve direct peer-to-peer connections across NAT boundaries. All content transfers between NAT-ed peers go through a relay server, making it centralized infrastructure disguised as P2P.

---

## Failure 1: Docker Network Simulation

### What Happened
Built a Docker Compose topology simulating NAT, CGNAT, LAN, and internet:
- 5 Docker networks (internet, lan1, lan2, isp, home)
- 4 NAT gateway containers (iptables MASQUERADE)
- 4 dweb nodes + 1 bootstrap
- Gateway containers needed `docker network connect` post-creation because Docker Compose fails to attach containers to multiple networks at creation time ("Address already in use" on .0.1 IPs which Docker reserves for bridge gateway)

### Results
- **LAN test (mDNS)**: PASSED - 2 nodes on same subnet discovered each other and exchanged content
- **NAT test**: Initially FAILED (DHT provider records not found), then PASSED after fixing the FetchManager to not send content requests before the relay connection established
- **CGNAT test**: PASSED (content arrived)
- **Large file test**: FAILED (JSON byte array encoding doesn't scale)

### Why It's Useless
Every "successful" cross-NAT fetch showed `(relayed: true)` in the logs. The dcutr hole punch logs showed: "Hole punch failed: Giving up after 3 dial attempts". ALL content data flowed through the bootstrap relay node. The tests proved the relay proxy works, not that P2P works.

Docker's iptables MASQUERADE creates port-restricted cone NAT (conntrack entries are per-flow), which means:
- Outbound packet from node to relay creates mapping: `(node_internal:port -> NAT_external:mapped_port)` for destination=relay only
- When another peer tries to send to `NAT_external:mapped_port`, the packet is dropped because the conntrack entry was for a different destination
- This makes hole punching impossible in Docker's virtual networking

This is the same NAT behavior regardless of whether Docker or network namespaces are used, since both use the same kernel iptables/conntrack.

---

## Failure 2: Real Network Test (3 Machines)

### Setup
- **Hetzner server (Germany)**: `89.167.68.65`, public IP, running k3s + trading bots
  - SSH: `ssh -i ~/.ssh/the0-prod root@89.167.68.65`
  - RULES: Only touch `/tmp/dweb-test/`, no destructive actions, don't interfere with trading bots
  - Firewall: ufw, ports opened: 4001/tcp, 4001/udp, 9862/tcp (comment "dweb P2P/API")
  - Binary built on server from git: `/tmp/dweb-test/repo/target/release/dweb`
  - Note: server has glibc 2.35, local machine has 2.39 - binaries compiled locally won't run there. Must build on the server or use musl static binary.
- **Local machine (Kenya)**: Behind ISP NAT, external IP `41.90.179.143`
  - Feature branch: `feature/m3-daemon-architecture`
  - Binary: `/home/alexander/Code/Apps/dweb/target/release/dweb`
- **Mac**: Behind mobile hotspot NAT (`172.20.10.6`), external IP `105.161.178.21`
  - Built from same git branch

### How the Test Ran
1. Hetzner: `dweb start --port 4001 --api-port 9862 --api-bind 0.0.0.0 --no-bootstrap`
   - Peer ID: `12D3KooWMNyMvsz8S1UBJz5RUtT2tyFTbphoj6SSR9YJ6qD4jkwk`
2. Kenya: `dweb start --port 4001 --api-port 9863 --bootstrap /ip4/89.167.68.65/tcp/4001/p2p/12D3KooWMNyMvsz8S1UBJz5RUtT2tyFTbphoj6SSR9YJ6qD4jkwk`
   - Published test content: `bafkr4ig2xt6llj2b2lkh5vu3lcduaggweyz76oqrlu5oxthntz7rxqswai`
3. Mac: Same bootstrap address, fetched the content ID

### Results
- **DHT provider discovery**: WORKED - Mac found Kenya node as provider via DHT within ~0.5s
- **Relay connection**: WORKED - Mac connected to Kenya node via Hetzner relay
- **Content transfer**: WORKED - 80 bytes received correctly
- **Hole punching**: FAILED - `Hole punch failed with 12D3KooWFokDeVJD...: Giving up after 3 dial attempts`
- **Connection type**: `(relayed: true)` - ALL data went through Hetzner relay

### Why Hole Punching Failed
Both Kenya and Mac are behind carrier-grade NAT (CGNAT):
- Kenya: ISP NAT at `41.90.179.143` (Safaricom/mobile carrier)
- Mac: Mobile hotspot NAT at `105.161.178.21`

CGNAT typically uses symmetric NAT (endpoint-dependent mapping), where the external port changes for each destination. When dcutr tries to coordinate hole punching:
1. Kenya sends probe to Mac's external address -> Kenya's NAT creates mapping for destination=Mac
2. Mac sends probe to Kenya's external address -> Mac's NAT creates mapping for destination=Kenya
3. But the port mappings are different from the ones used for the relay connection
4. The probes arrive at the wrong ports and get dropped

---

## Why BitTorrent and Soulseek Work

This is the key question. They DO work behind NAT. Here's how:

### BitTorrent
- **uTP (Micro Transport Protocol)**: BitTorrent's own UDP-based protocol specifically designed for NAT traversal
- **UPnP/NAT-PMP**: BitTorrent clients aggressively use UPnP to open ports on the home router. This doesn't work through CGNAT, but it works on most home networks.
- **Relay through DHT**: BitTorrent's DHT supports relaying small metadata. Actual file transfers use:
  - Direct connections when UPnP succeeds
  - "Hole punching" via the tracker/DHT coordination
  - **Fallback to relay for piece exchange** - BitTorrent DOES use intermediary peers as relays when direct connection fails. The difference is that ANY peer can relay, not just a single bootstrap.
- **PEX (Peer Exchange)**: Peers share connection info about other peers they know, helping find more paths

### Soulseek
- Soulseek uses a **central server** for coordination and often **does fail behind strict NAT**. Many Soulseek users report "cannot connect" issues behind CGNAT.
- When it works, it's typically because UPnP opened a port on the home router.

### What dweb Is Missing vs BitTorrent
1. **No UPnP success**: Our UPnP implementation attempts port mapping but both test machines were behind CGNAT where UPnP can't reach the outermost NAT. On a normal home network, UPnP should work.
2. **Single relay point**: We only have one bootstrap/relay. BitTorrent uses ANY connected peer as a potential relay. If peer A can reach both B and C directly, A can relay data between them.
3. **No multi-path delivery**: BitTorrent splits files into pieces and can fetch different pieces from different peers simultaneously. We fetch the entire content from a single provider.
4. **TCP hole punching is harder than UDP**: Our QUIC transport uses UDP (good), but dcutr's hole punch attempts still failed. BitTorrent's uTP is specifically optimized for this.
5. **No STUN/TURN**: We rely on libp2p's identify protocol for address discovery, which is less sophisticated than dedicated STUN servers. We don't have TURN-style relay infrastructure.

---

## The Fundamental Problem

The content DOES transfer across the internet via relay. The daemon architecture, HTTP API, DHT provider discovery, and FetchManager state machine all work correctly. The failure is specifically:

**dcutr hole punching fails through CGNAT/symmetric NAT, and we have no fallback mechanism for direct connectivity.**

This means on networks where both peers are behind strict NAT (mobile carriers, some ISPs), all data flows through the relay. This is not peer-to-peer.

### What Would Fix This
1. **QUIC hole punching improvements**: libp2p's dcutr for QUIC is still maturing. The hole punch probes may need better timing or more attempts.
2. **Multiple relay peers**: Instead of a single bootstrap relay, every node should be able to relay for others (we already have `relay_server` in the behaviour, but it's not being leveraged for content routing).
3. **UPnP on non-CGNAT networks**: On home networks with a single NAT router, UPnP should open ports and eliminate the need for hole punching entirely. We should verify this works.
4. **STUN-like address discovery**: More robust external address detection using multiple servers.
5. **Port prediction**: For symmetric NAT, predict the next port allocation and attempt to punch through with the predicted port.

---

## Current State of the Codebase

### Branch
`feature/m3-daemon-architecture` on `github.com/alexanderwanyoike/dweb`

### What Works
- Daemon architecture with command channels (100 unit tests passing)
- HTTP API (all endpoints functional)
- CLI as thin client to daemon
- DHT provider discovery across the internet
- Content transfer via relay (not direct P2P)
- Content caching for re-sharing
- FetchManager with proper relay settle delay (based on the working pre-daemon fetch.rs flow)

### What Doesn't Work
- Direct peer-to-peer connections between NAT-ed peers (hole punching fails)
- Docker network simulation for NAT testing (same iptables issue)
- Large file transfer via JSON API (byte array encoding doesn't scale)

### Hetzner Server Cleanup Required
Files added to the server that should be removed when done:
```
/tmp/dweb-test/           # entire directory
```
Firewall rules added:
```
ufw delete allow 4001/tcp
ufw delete allow 4001/udp
ufw delete allow 9862/tcp
```
Running process:
```
pkill -f '/tmp/dweb-test/repo/target/release/dweb'
```

### Running the Test Yourself
```bash
# On Hetzner (bootstrap, public IP):
ssh -i ~/.ssh/the0-prod root@89.167.68.65
source ~/.cargo/env
cd /tmp/dweb-test/repo
git pull
cargo build --release
RUST_LOG=info ./target/release/dweb start --port 4001 --api-port 9862 --api-bind 0.0.0.0 --no-bootstrap

# Get the peer ID from the output, then on any other machine:
git clone https://github.com/alexanderwanyoike/dweb.git && cd dweb
git checkout feature/m3-daemon-architecture
cargo build --release
RUST_LOG=info ./target/release/dweb start --port 4001 --api-port 9862 \
  --bootstrap /ip4/89.167.68.65/tcp/4001/p2p/<PEER_ID_FROM_BOOTSTRAP>

# Publish:
curl -F "file=@somefile.txt" http://127.0.0.1:9862/api/v1/publish

# Fetch (from another machine):
curl -X POST http://127.0.0.1:9862/api/v1/fetch \
  -H 'Content-Type: application/json' \
  -d '{"content_id": "<CONTENT_ID>"}'
```
