# Content Distribution and Caching

## Overview

jolt distributes content across the network using content addressing and peer-to-peer transfers. Popular content naturally replicates as more nodes cache it, improving availability and performance. This is particularly important for bandwidth-heavy use cases like video and music streaming.

Availability is not magic. If the publisher's node is offline, some other online node must have the content. jolt's answer is owner-directed relay pinning plus opportunistic caching:

- **Pinning** is intentional. The owner or local user asks a node to keep content.
- **Caching** is opportunistic. A node may keep content it fetched and may evict it later.
- **Mirroring** is future work. Owner-authorized relay-to-relay replication can be added later.

## Content Publishing

When a user publishes content:

```mermaid
flowchart TD
    File["Original File"] --> Hash["Hash whole file (BLAKE3) -> ContentId"]
    Hash --> Manifest["Create signed ContentManifest"]
    Manifest --> Store["Store blob + manifest in published/"]
    Store --> DHT["Announce provider record to DHT"]
    DHT --> Log["Add update log entry"]
```

Content moves as whole blobs; there is no chunking. The manifest describes a single blob:

```rust
struct ContentManifest {
    content_id: ContentId,
    size: u64,
    content_type: String,
    publisher_key: Vec<u8>,
    signature: Vec<u8>,
}
```

If the user has a home relay, publishing also asks that relay to pin the content and the signed update record:

```mermaid
flowchart TD
    Publish["Publish content locally"] --> Record["Sign update record"]
    Record --> Pin["Ask home relay to pin content"]
    Pin --> Announce["Relay announces provider records"]
    Announce --> Fetch["Other nodes can resolve and fetch while publisher is offline"]
```

### Chunking

> Future design, not implemented in v0. Content is transferred and cached as whole blobs.

Large files would be split into fixed-size chunks for efficient transfer and caching.

```
File: video.mp4 (500MB)

Chunk size: 256KB

Chunks:
  chunk_0: bytes[0..256KB]        -> ContentId: abc...
  chunk_1: bytes[256KB..512KB]    -> ContentId: def...
  chunk_2: bytes[512KB..768KB]    -> ContentId: ghi...
  ...
  chunk_n: bytes[last segment]    -> ContentId: xyz...

Manifest:
  {
    name: "video.mp4",
    total_size: 500_000_000,
    chunk_size: 262_144,
    chunks: [abc..., def..., ghi..., ..., xyz...],
    content_type: "video/mp4",
  }
  -> ManifestId: mno...
```

Intended benefits of chunking:
- **Parallel downloads** from multiple peers simultaneously
- **Partial caching** -- nodes cache individual chunks, not whole files
- **Resume** -- interrupted downloads continue from the last chunk
- **Deduplication** -- identical chunks across files are stored once
- **Streaming** -- video/audio can start playing before full download

## Content Fetching

Fetching is coordinated by the `FetchManager` in jolt-network. It asks every currently connected peer for the whole blob in parallel, while a DHT provider query runs alongside. If no connected peer has the content, the node dials a provider found via the DHT and requests the blob from it. The first successful whole-blob response wins; the fetch moves through the states `TryingPeers -> QueryingDht -> WaitingForProvider -> FetchingFromProvider`.

```mermaid
sequenceDiagram
    participant Node as Requester
    participant P1 as Connected Peer A
    participant P2 as Connected Peer B
    participant DHT as DHT
    participant P3 as Provider C

    par Try connected peers (whole blob each)
        Node->>P1: Request ContentId X
        Node->>P2: Request ContentId X
    and DHT query in parallel
        Node->>DHT: "Who provides ContentId X?"
    end

    P1--xNode: Not found
    P2--xNode: Not found
    DHT-->>Node: Provider C
    Node->>P3: Dial, then request ContentId X
    P3-->>Node: Whole content blob

    Node->>Node: Verify blob against ContentId
    Node->>Node: Cache locally
```

### Provider Selection

There is no latency or bandwidth ranking. The lookup order is:

```
Priority:
  1. Local store (published content, then cache)
  2. Currently connected peers (tried in parallel)
  3. Providers discovered via the DHT
```

### Swarming

> Future design, not implemented in v0. Without chunking, each fetch is a whole-blob transfer from a single peer.

For popular content, downloading would resemble BitTorrent:

```mermaid
graph TD
    subgraph before["10 nodes have cached the video"]
        A["Peer A"] & B["Peer B"] & C["Peer C"] & D["...7 more"]
    end

    New["New Viewer"] -->|chunk 0| A
    New -->|chunk 1| B
    New -->|chunk 2| C

    subgraph after["After download: 11 providers"]
        A2["Peer A"] & B2["Peer B"] & C2["Peer C"] & D2["...7 more"] & New2["New Viewer"]
    end

    before --> after

    style New fill:#e94560,stroke:#fff,color:#fff
    style New2 fill:#e94560,stroke:#fff,color:#fff
```

The more popular content is, the faster it would download. This is the opposite of centralized hosting where popularity = more server load = slower. (Nodes that cache content do already serve it to others, so popular content gains providers; the parallel chunk downloads above are the future part.)

## Caching

### Cache Policy

Every node maintains a content cache with LRU (Least Recently Used) eviction. The only configurable knob is the cache size:

```rust
struct CacheConfig {
    max_size_bytes: u64,   // default 2 GB
}
```

Eviction removes non-pinned entries in ascending last-accessed order until the new content fits. Pinned content is never evicted; if pinned content alone fills the cache, further caching fails with a cache-full error. The LRU policy is hard-coded, and there are no `min_free_disk` or pin-size limits.

### What Gets Cached

```
Automatically cached:
  - Any content fetched from the network (on access)

May be cached but unreadable:
  - Private/encrypted content from other users
  - The cache can improve availability even if the caching node cannot decrypt it

Pinned (cached permanently until unpinned):
  - Content the user explicitly pins
  - Content the owner asked this relay to pin
```

There is no publisher no-cache flag. Caching app binaries on install belonged to the abandoned in-process app model (see [App Boundary and Sessions](15-app-boundary-and-sessions.md)).

### Cache Contribution

Nodes serve cached content to any peer that requests it over the content fetch protocol, contributing bandwidth to the network. There is currently no gating: serving is always on.

> Future design, not implemented in v0: making cache sharing opt-in and configurable.

```toml
[cache.sharing]
enabled = true
max_upload_bandwidth = "5MB/s"    # bandwidth allocated to serving cache
serve_while_on_battery = false    # pause when on battery power
serve_on_metered = false          # pause on metered connections
```

## Streaming

> Future design, not implemented in v0. Streaming depends on chunking and range handling, neither of which exists yet; content is fetched as a whole blob before it can be served.

### Video Streaming

Video files would use a specialized chunking strategy for streaming:

```
Video file split into:
  - Manifest with byte-range-to-chunk mapping
  - Sequential chunks (for linear playback)
  - Optional: multiple quality levels (adaptive bitrate)

Playback:
  1. Browser requests video via localhost HTTP
  2. Node serves chunks as HTTP range responses
  3. Chunks fetched from network on-demand (prefetch ahead of playback)
  4. Browser plays via standard <video> tag
  5. Buffering strategy: prefetch next N chunks while playing current

Seeking:
  1. User seeks to timestamp T
  2. Map T to byte offset to chunk index
  3. Fetch that chunk (and following chunks) from network
  4. Continue playback from new position
```

### Audio Streaming

Same model as video. Chunks are smaller (64KB) for faster start time.

### Live Streaming (Future)

Live content uses a different model since content isn't pre-chunked:

```
1. Streamer's node encodes live video into chunks in real-time
2. Each chunk is published immediately (no full-file hash)
3. Viewers subscribe to the streamer's chunk stream
4. Chunks propagate through the viewer swarm
5. Latency target: 5-15 seconds (similar to HLS/DASH)
```

## Content Availability

The honest rule is:

```
Content is available while at least one node that has it is online and willing to serve it.
```

Availability comes from:

```
1. The publisher's node
   Available while the user's own node is online.

2. Home relay pinning
   The user's relay keeps selected published content online.

3. Owner-directed multi-relay pinning
   The user's node may upload and pin content on more than one relay.

4. Cache-on-fetch
   Nodes that fetch content may cache and serve it.
```

Relays should not silently create durable copies on other relays in v0. Replication is owner-directed so the user's key remains the authority over intentional persistence.

### Unavailability UX

When content is unavailable, the experience should be graceful:

```mermaid
graph TD
    subgraph offline_ux["Unavailability UX"]
        Msg["This content is currently offline"]
        Info["Publisher last seen: 2 hours ago<br/>Cached by: 0 peers"]
        Btn1["Notify me when available"]
        Btn2["View cached version (if available)"]

        Msg --> Info --> Btn1 & Btn2
    end

    style offline_ux fill:#1a1a2e,stroke:#e94560,color:#fff
```

## Bandwidth Contribution

jolt has no built-in token or payment system for bandwidth. For the core protocol, payment is out of scope. Participation relies on:

1. **Reciprocity** -- nodes that download also upload (cache sharing)
2. **Social incentive** -- redundancy groups are mutual arrangements
3. **Self-interest** -- caching popular content means faster access for yourself too

This avoids the complexity of token economics in the core protocol. The network functions like BitTorrent: most users contribute by default because the protocol is designed that way.
