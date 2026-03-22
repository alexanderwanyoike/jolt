# Development Milestones

## M1: Two Nodes Talking

**Goal:** Two dweb nodes on a LAN discover each other and exchange a file.

**Deliverables:**
- Rust workspace with `dweb-core`, `dweb-identity`, `dweb-network`, `dweb-node` crates
- Ed25519 keypair generation and storage
- libp2p node with mDNS discovery
- Content-addressed file publishing (hash, store, announce)
- Content fetch protocol (request by ContentId, receive, verify)
- CLI interface: `dweb start`, `dweb publish <file>`, `dweb fetch <content-id>`

**Success criteria:** Node A publishes a file, Node B fetches it by ContentId, file integrity is verified.

---

## M2: Content Caching and Availability

**Goal:** Nodes cache content they fetch and serve it to other peers.

**Deliverables:**
- Local cache with LRU eviction
- Cache size configuration
- Nodes serve cached content to requesters (automatic re-sharing)
- Content pinning (prevent eviction)
- Cache statistics CLI: `dweb cache stats`, `dweb cache list`

**Success criteria:** Node A publishes, Node B fetches and caches, Node A goes offline, Node C fetches from Node B's cache.

---

## M3: Browser UI, HTTP Server, and Protocol Handler

**Goal:** Users interact with their node through a browser. `dweb://` links work system-wide.

**Deliverables:**
- `dweb-server` crate with axum HTTP server
- REST API for node operations (identity, content, peers)
- Basic browser UI: node status, peer list, publish/fetch content
- Localhost-only binding
- Register `dweb://` as OS protocol handler on install (Linux `.desktop`, macOS `Info.plist`, Windows registry)
- URI resolution: `dweb://` links resolve through the node and open in the default browser

**Success criteria:** User opens localhost in browser, sees their node status, can publish and fetch files through the UI. Clicking a `dweb://` link anywhere on the system opens the content in the browser.

---

## M4: Update Logs and Mutable Content

**Goal:** Users can update published content and peers can resolve the latest version.

**Deliverables:**
- `UpdateLog` implementation (append-only, signed, hash-chained)
- Update log sync protocol
- Mutable content resolution (PeerId -> latest content root)
- User profile (display name, bio)
- CLI: `dweb publish --update <path>`, `dweb resolve <peer-id>`

**Success criteria:** User publishes v1 of a file, updates to v2, other nodes resolve and fetch v2 by the user's PeerId.

---

## M5: DHT and Internet-Wide Networking

**Goal:** Nodes discover each other and exchange content over the internet, not just LAN.

**Deliverables:**
- Kademlia DHT integration
- Bootstrap nodes (run at least 2 for the project)
- NAT traversal: UPnP, hole punching, relay fallback
- Peer exchange (PEX)
- Connection management and limits

**Success criteria:** Two nodes on different networks (different homes/offices) discover each other via DHT and exchange content.

---

## M6: Encryption and Access Control

**Goal:** Users can publish private content encrypted for specific recipients or groups.

**Deliverables:**
- `dweb-crypto` crate
- X25519 key derivation from Ed25519 identity
- Encrypt content for single recipient
- Group key management (create, distribute, rotate)
- Visibility levels: public, private, group
- CLI: `dweb publish --private --recipient <peer-id>`

**Success criteria:** Alice publishes encrypted content for Bob. Bob decrypts it. Carol cannot.

---

## M7: WASM Runtime

**Goal:** The node can execute WASM applications in a sandboxed environment.

**Deliverables:**
- `dweb-runtime` crate with wasmtime integration
- Host API: storage (KV), logging
- Capability-based permission system
- Resource limits (memory, CPU, storage)
- App isolation (per-app sandbox)

**Success criteria:** A simple WASM app (counter that persists to KV store) runs on the node, data persists across restarts.

---

## M8: App Lifecycle

**Goal:** Users can install, run, update, and remove apps.

**Deliverables:**
- `dweb-apps` crate
- App manifest format
- App packaging: `dweb app pack`
- App publishing: `dweb app publish`
- App installation from network
- App update detection and installation
- App removal
- Browser UI: app launcher, install/update/remove

**Success criteria:** Developer publishes an app, user installs it from the network, app runs in browser, developer publishes update, user updates.

---

## M9: Host API - Network and Identity

**Goal:** WASM apps can communicate with peers and access identity.

**Deliverables:**
- Host API: peer messaging (send, receive, broadcast)
- Host API: peer discovery (list online peers running same app)
- Host API: identity (read peer ID, display name)
- Host API: crypto (encrypt for recipient, decrypt)
- App data sync protocol

**Success criteria:** A chat app where two users install it, discover each other, and exchange encrypted messages.

---

## M10: Content Streaming

**Goal:** Large files (video, audio) can be streamed efficiently.

**Deliverables:**
- File chunking (configurable chunk size)
- Parallel chunk downloads from multiple peers (swarming)
- HTTP range request support for browser-based playback
- Chunk-level caching
- Prefetch strategy for sequential playback
- Seek support

**Success criteria:** User publishes a video, another user streams it in the browser with reasonable start time and smooth playback.

---

## M11: Redundancy Groups

**Goal:** Groups of nodes cooperate to keep each other's content available.

**Deliverables:**
- Redundancy group creation and membership
- Content replication within groups
- Encrypted backup of private data to group members
- Group health monitoring
- Automatic sync when members come online

**Success criteria:** A group of 3 nodes. One goes offline. Its published content remains available via the other two.

---

## M12: Developer SDK and Documentation

**Goal:** Developers can build dweb apps easily.

**Deliverables:**
- `dweb-sdk-rust` crate with ergonomic host API wrappers
- `dweb-sdk-js` npm package (for QuickJS-based apps)
- App development tutorial
- API reference documentation
- Example apps: blog, chat, file sharing
- App template: `dweb app init --template <name>`

**Success criteria:** A developer with no dweb experience can follow the tutorial and publish a working app within an afternoon.

---

## Milestone Dependencies

```mermaid
graph TD
    M1["M1: Two Nodes Talking"]

    M1 --> M2["M2: Caching"]
    M1 --> M3["M3: Browser UI"]
    M1 --> M4["M4: Update Logs"]
    M1 --> M5["M5: DHT / Internet-Wide"]
    M1 --> M6["M6: Encryption"]

    M2 --> M7["M7: WASM Runtime"]
    M2 --> M11["M11: Redundancy Groups"]

    M7 --> M8["M8: App Lifecycle"]
    M7 --> M10["M10: Content Streaming"]

    M8 --> M9["M9: Host API (Network + Identity)"]

    M9 --> M12["M12: Developer SDK + Docs"]

    style M1 fill:#e94560,stroke:#fff,color:#fff
    style M2 fill:#0f3460,stroke:#fff,color:#fff
    style M3 fill:#0f3460,stroke:#fff,color:#fff
    style M4 fill:#0f3460,stroke:#fff,color:#fff
    style M5 fill:#0f3460,stroke:#fff,color:#fff
    style M6 fill:#0f3460,stroke:#fff,color:#fff
    style M7 fill:#533483,stroke:#fff,color:#fff
    style M8 fill:#533483,stroke:#fff,color:#fff
    style M9 fill:#533483,stroke:#fff,color:#fff
    style M10 fill:#533483,stroke:#fff,color:#fff
    style M11 fill:#533483,stroke:#fff,color:#fff
    style M12 fill:#533483,stroke:#fff,color:#fff
```

M1 through M5 can be worked on somewhat in parallel. M7-M9 build sequentially. M10, M11, and M12 can be developed independently once their dependencies are met.
