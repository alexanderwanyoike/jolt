# Jolt Vision

## What Is Jolt?

Jolt is a peer-to-peer substrate for creator-owned and user-owned digital communities.

It lets people run spaces without surrendering content, identity, or distribution to a central platform. A space can be public, private, or semi-private. It can hold posts, releases, game builds, datasets, documents, announcements, media, lobbies, or any other content a community needs.

> Your community, your content, your identity. Distributed by the network, owned by no platform.

## Core Thesis

Central platforms bundle four things:

```text
identity
content hosting
distribution
community coordination
```

That bundle is convenient, but it creates lock-in. If a creator leaves a platform, they often lose the audience, posts, files, community graph, release history, and access relationships that made the platform valuable. If a user leaves, they lose access to spaces where their identity and history live.

Jolt unbundles the platform:

```text
Identity       -> owned by keys
Content        -> content-addressed, signed, encrypted when needed
Distribution   -> relays, provider discovery, peer caching
Community      -> spaces, membership, feeds, invites, access grants
Applications   -> optional clients/tools over the same owned graph
```

Jolt is not "the web, but decentralized". The web is excellent at public pages, hyperlinks, and global search. Jolt is for communities where authorship, access, continuity, and platform independence matter more than being globally indexed.

## The Product Shape

The core experience is:

```text
Bob connects to Alice or a community.
Bob sees what that identity has allowed him to see.
Bob can verify who authored or granted each thing.
Bob can fetch content from Alice, a relay, or another authorized peer.
Alice can go offline without losing the community's reachable state.
```

This is a permissioned content graph, not a public page graph.

When Bob connects to Alice, he is not browsing random files. He is entering Alice's signed space:

```text
Alice's Space
  -> profile / introduction
  -> feeds and announcements
  -> content Alice authored
  -> communities Alice belongs to
  -> identities Alice recommends or vouches for
  -> content Bob is allowed to access
  -> version and provenance history
```

Everything Bob accepts is verified against signatures and access rules. A relay may carry bytes, but it does not become the authority.

## Why Not Just Use The Web?

The web's primitive is:

```text
URL -> server -> HTML page
```

That works well when the desired outcome is a public website.

Jolt's primitive is:

```text
identity -> signed state -> authorized content graph
```

That works better when the desired outcome is a community or relationship-owned space:

- A creator community that should not depend on Patreon, Discord, Substack, YouTube, or X.
- A game community that wants signed builds, mods, announcements, lobbies, and matchmaking without Steam owning distribution.
- A research group that needs datasets, notebooks, provenance, and usage rights.
- A private group that wants durable content without handing everything to a cloud platform.
- A project that wants signed releases and community state under its own identity.

## Authority Model

Jolt does not remove the physical need for online computers. If content must be reachable while its owner is offline, some online node must store or serve it.

Jolt changes who has authority:

```text
Current platforms:
  The platform account/server is the authority.

Jolt:
  The identity key is the authority.
  Relays and peers are replaceable carriers.
```

An authorized peer may copy content they can decrypt. No network can prevent that once plaintext is disclosed. Jolt instead preserves provenance:

- Unauthorized peers cannot decrypt private content.
- Relays cannot forge an author's signature.
- Modified content gets a different CID.
- Clients can show "authored by Alice, served by Bob/Relay".
- Access grants and usage rights can be signed and audited.

## Core Primitives

### Identity

An identity is a long-lived key that owns a person, community, project, or application space.

Canonical Jolt addresses are identity based:

```text
{identity}.jolt
{identity}.jolt/feed
{identity}.jolt/releases/latest
```

These are not first-contact network dial addresses. A fresh node still needs bootstrap relay multiaddrs to join the mesh.

### Signed State

Mutable state is represented with signed append-only update logs.

The update log can express:

- content references
- profile/community metadata
- membership changes
- access grants and revocations
- version replacements
- relay reachability hints
- app/interface preferences

### Content

Content is immutable and content-addressed. It can be public or encrypted.

Popular or shared content can be cached and re-served by peers without losing authorship, because the CID and signatures remain verifiable.

### Spaces

A space is the signed content graph owned by an identity.

It can represent a creator page, game community, research group, project, family archive, legal workspace, or any other digital community.

### Relays

Relays provide availability and discovery. They may:

- help nodes enter the mesh
- announce provider records
- pin owner-authorized content
- keep signed update logs reachable
- assist with NAT traversal

Relays are not platforms. They do not define truth for a space.

### Apps

Apps are optional interfaces for spaces.

A Jolt app can render or edit a particular kind of space: a game community, research workspace, creator feed, legal document graph, or private group. Apps are external clients that request capability-scoped sessions from the local daemon; they never hold the user's keys. The app is not the core product. The core product is owned identity, signed state, access, and distribution.

The protocol should not know what those interfaces mean. It should expose verifiable identity-owned state, generic paths, content references, access grants, and relay policies. Profiles, feeds, galleries, timelines, and games are higher-layer interpretations of signed content.

### HTML Views

HTML is still useful as a view of a space.

Jolt should not make HTML the authority model. The authority is signed state: identities, claims, content references, access grants, and update logs. But HTML is a good browseable projection of that state because it gives users a familiar tree, links, media, and layout.

That means a space can expose both:

```text
structured signed state
  -> machine-verifiable source of truth

HTML view
  -> human-browseable rendering of that space
```

For example:

```text
{identity}.jolt/
{identity}.jolt/feed
{identity}.jolt/releases
```

may render as HTML in a Jolt client, while the client still verifies the underlying signed records and content IDs before trusting what it displays.

## First Proof

The next important proof is not "browse the whole decentralized web".

It is:

```text
Alice creates a space.
Bob joins by address or invite.
Alice publishes signed content to that space.
Alice delegates availability to a relay.
Alice goes offline.
Bob starts fresh with only bootstrap relay configuration.
Bob resolves Alice's .jolt address.
Bob fetches and verifies the content from the relay or an authorized peer.
```

If that works, Jolt has proved the core: creator-owned/community-owned distribution without a central platform.

## What Jolt Is Not

- Not a blockchain. No tokens, mining, or global consensus are required for v0.
- Not a public web replacement. It does not compete with browsers and search engines first.
- Not a storage marketplace. Payments and relay economics can wait.
- Not just file sharing. Content matters because it is part of signed community state.
- Not an application runtime. Apps are interfaces over spaces, not the reason the network exists.
