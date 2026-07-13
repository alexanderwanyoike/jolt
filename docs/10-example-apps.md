# Example Applications

> **Status: illustrative concepts.** None of the applications below are built,
> and this document predates the implemented app model. It is kept as a
> sketchbook of what the primitives could support, not a roadmap.
>
> The real example application is
> [Spoke](https://github.com/alexanderwanyoike/spoke), an external social app
> (profiles, posts, feed, encrypted replies) that runs against the local
> daemon through a capability-scoped session and never holds keys. The
> implemented app boundary and capability vocabulary are defined in
> [App Boundary and Sessions](15-app-boundary-and-sessions.md).
>
> The `Permissions:` lines below use an old capability sketch (`storage`,
> `network`, ...). A real app requests path-scoped capabilities instead, e.g.
> a video app would request `publish:/video/*`, `inventory:/video/*`,
> `enumerate:any:/video/*`, and `fetch:public`.

## Overview

These are example application concepts that illustrate what could be built on jolt. Each follows the same model: an app the user runs locally, data owned by the user's identity, communication via P2P.

---

## jolt-video

**Decentralized video platform. YouTube without YouTube.**

Type: Client-side (hybrid optional for background transcoding)

```mermaid
sequenceDiagram
    participant Creator
    participant Node as Creator's Node
    participant Net as Network / Swarm
    participant Viewer as Viewer's Node

    Note over Creator,Node: Publishing
    Creator->>Node: Select video file
    Node->>Node: Chunk video (256KB chunks)
    Node->>Node: Optionally transcode to multiple qualities
    Node->>Net: Publish chunks + manifest
    Node->>Node: Add entry to update log

    Note over Net,Viewer: Watching
    Viewer->>Viewer: Browse followed creators (update logs)
    Viewer->>Net: Request video chunks
    Net-->>Viewer: Stream chunks from swarm
    Note over Viewer: More viewers = faster streaming
    Viewer->>Viewer: Becomes a new provider (caches chunks)
```

**Discovery:** Follow by public key, chronological feed (no algorithm), DHT search, curated channels

**Permissions:** storage, network, content

**Sustainability:** No ads and no platform-controlled monetization in the core protocol. Any app-level economic model is outside the protocol.

---

## jolt-chat

**End-to-end encrypted messaging. Signal without the phone number.**

Type: Hybrid (server for background message receiving, client for UI)

```mermaid
sequenceDiagram
    participant Alice
    participant ANode as Alice's Node
    participant BNode as Bob's Node
    participant Bob

    Note over Alice,Bob: Setup: identity IS public key, no registration

    Alice->>ANode: Compose message
    ANode->>ANode: Encrypt to Bob's public key
    ANode->>BNode: P2P direct connection
    BNode->>BNode: Server component receives + stores
    Bob->>BNode: Open chat in browser
    BNode->>Bob: Client component displays message

    Note over ANode,BNode: If Bob is offline
    ANode->>ANode: Queue message
    ANode->>BNode: Deliver when Bob comes online
```

**Group chat:** Creator generates group key, encrypts to each member's public key. All members read, outsiders cannot.

**Permissions:** storage, network, crypto, identity

---

## jolt-blog

**Personal publishing. Your blog, your rules, forever.**

Type: Client-side

```mermaid
graph LR
    subgraph writer["Writer"]
        Editor["Markdown Editor"] --> Publish["Publish<br/>(content-addressed, signed)"]
        Publish --> Log["Update Log Entry"]
    end

    subgraph reader["Reader"]
        Follow["Follow by public key"] --> Feed["Chronological Feed"]
        Feed --> Cache["Cached locally for offline"]
    end

    subgraph comments["Comments"]
        Comment["Signed comment<br/>referencing post ContentId"] --> Aggregate["Blogger aggregates"]
        Aggregate --> Moderate["Moderate (approve/hide)"]
    end

    Log --> Feed
```

**Permissions:** storage, network, content

---

## jolt-drive

**Personal file storage and sharing. Dropbox without the cloud.**

Type: Client-side (hybrid optional for background sync)

```mermaid
graph TD
    subgraph storage["Storage"]
        Files["Files on user's node"] --> Folders["Organized in folders"]
        Folders --> CAddr["Content-addressed (dedup)"]
    end

    subgraph sharing["Sharing"]
        Public["Public: jolt:// link"]
        Private["Private: encrypted for recipients"]
        Group["Group: shared folder model"]
    end

    subgraph sync["Sync"]
        Multi["Multiple devices"] <-->|P2P| Multi
        Conflict["Conflict resolution:<br/>last-write-wins or manual merge"]
        Redundancy["Redundancy group backup"]
    end

    storage --> sharing
    storage --> sync
```

**Permissions:** storage, network, crypto

---

## jolt-music

**Music streaming and distribution. Spotify without the middleman.**

Type: Client-side

```mermaid
graph LR
    subgraph artist["Artist"]
        Upload["Upload tracks<br/>(chunked for streaming)"]
        Albums["Create albums/playlists"]
        Price["Set pricing<br/>free / pay-what-you-want / fixed"]
        Upload --> Albums --> Price --> PubNet["Publish to network<br/>(keep 100% revenue)"]
    end

    subgraph listener["Listener"]
        Browse["Browse/search artists"]
        Stream["Stream from swarm"]
        Library["Build personal library<br/>(cached offline, yours forever)"]
        Browse --> Stream --> Library
    end

    PubNet --> Browse
```

**Discovery:** Follow artists (update logs), genre tags, community playlists, no algorithm

**Permissions:** storage, network, content

---

## jolt-market

**Peer-to-peer marketplace. eBay/Etsy without the fees.**

Type: Client-side

```mermaid
sequenceDiagram
    participant Seller
    participant Net as jolt Network
    participant Buyer

    Seller->>Net: Create listing (photos, description, price)
    Buyer->>Net: Search / browse listings
    Net-->>Buyer: Matching listings
    Buyer->>Seller: Contact directly (P2P encrypted)
    Buyer->>Seller: Agree terms outside protocol
    Seller->>Seller: Ship item or provide access
    Buyer->>Net: Leave signed review

    Note over Net: No listing fee, no transaction fee<br/>No platform cut, seller keeps 100%
```

**Trust:** Signed reviews from verified buyers, reviews tied to real jolt identities (Sybil-resistant), public transaction history

**Permissions:** storage, network, identity, crypto

---

## jolt-social

**Social networking. The feed without the algorithm.**

Type: Client-side

```mermaid
graph TD
    subgraph posting["Posting"]
        Write["Write post<br/>(text, images, video, links)"]
        Write --> UpdateLog["Publish to update log"]
        UpdateLog --> Followers["Followers' nodes pick it up"]
    end

    subgraph following["Following"]
        Follow["Follow by public key"]
        Follow --> Subscribe["Subscribe to update log"]
        Subscribe --> Feed["Chronological feed<br/>(no algorithm, no ranking)"]
    end

    subgraph interactions["Interactions"]
        Reply["Replies: signed, referencing ContentId"]
        Repost["Reposts: reference in your log"]
        Like["Likes: signed attestation"]
    end

    UpdateLog --> Feed
    Feed --> interactions
```

**Privacy:** Public or group-only posts, encrypted DMs, no tracking, node-level blocking

**Permissions:** storage, network, content, identity

---

## jolt-wiki

**Collaborative knowledge base. Wikipedia without the foundation.**

Type: Client-side

```mermaid
sequenceDiagram
    participant Editor as Contributor
    participant Wiki as Wiki (update log)
    participant Maintainer as Maintainer
    participant Reader

    Editor->>Wiki: Publish signed edit (fork-and-propose)
    Maintainer->>Wiki: Review edit
    Maintainer->>Wiki: Accept / reject
    Wiki->>Wiki: Version history preserved (content-addressed)

    Reader->>Wiki: Fetch latest root manifest
    Wiki-->>Reader: Pages (each content-addressed)
    Reader->>Reader: Cache locally for offline access

    Note over Wiki: Governance: editor keys, community voting,<br/>forkable, no single entity controls content
```

**Permissions:** storage, network, content

---

## App Development Patterns

All the above apps share common patterns:

1. **Local-first clients** -- UI and logic run in an app the user controls,
   talking to the local daemon through a scoped session, not to a server
2. **Local data** -- all user data stored on the user's node
3. **P2P communication** -- users interact directly, no intermediary
4. **Content-addressed publishing** -- published content is immutable and verifiable
5. **Update logs for mutability** -- subscriptions and feeds use append-only logs
6. **Encryption for privacy** -- private content is encrypted to specific keys
7. **Signed everything** -- all actions are signed, providing identity and accountability
