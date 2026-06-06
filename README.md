# Jolt

Jolt is a peer-to-peer distributed content syndication network.

It is for platformless content distribution: a person or community publishes
content under their own cryptographic identity, other nodes can discover and
fetch that content, and apps can build experiences on top without owning the
audience, the account, or the keys.

Jolt is experimental v0 software. This repository contains the protocol,
daemon, HTTP/app API, Console, and local network implementation. The current
code proves signed identity state, content-addressed retrieval, scoped app
authority, encrypted objects, reachability records, and recipient-controlled
ingress. The system is still eventually consistent, developer-oriented, and not
yet packaged for normal users.

## Why Jolt Exists

On X, Instagram, Facebook, and similar platforms, distribution is owned by the
platform:

- your identity is a platform account;
- your audience is a platform graph;
- your posts live behind platform APIs and moderation systems;
- the feed is ranked by platform rules;
- the app controls what moves, what is hidden, and what can be exported.

That model works because it is convenient. It is also fragile. If a platform
changes rules, declines, bans an account, shuts down APIs, or stops showing your
work, your distribution can disappear even if your content still exists.

Jolt explores a different model: content distribution should be anchored in the
publisher's identity, not in a platform account. Apps can still exist, but they
should be replaceable views over content and relationships the platform does not
own.

## How Jolt Is Different

| Platform applications | Jolt |
|---|---|
| Identity is an account on the platform | Identity is a key owned by the user |
| Content is stored and distributed by the platform | Content is signed, content-addressed, and fetched peer-to-peer |
| Audience and relationships live in the platform graph | Apps can build relationship models over Jolt identities |
| The platform interface defines how content is experienced | Interfaces are replaceable app-level views over signed content |
| Apps hold the user's authority | Apps request scoped permission from the local daemon |
| Availability depends on the platform | Availability can come from peers, caches, and authorized relays |

The core idea is syndication at the network layer. A Jolt identity publishes
signed state that maps identity-owned protocol paths to content IDs. Nodes
resolve that state, fetch content from peers, caches, or authorized relays, and
verify that updates were signed by the identity owner. Applications can
interpret the signed content however they choose; those application schemas are
not part of the protocol.

## What Works Today

Jolt v0 has working implementations of the core protocol and local runtime
pieces:

- local daemon for identity, content, networking, and app permissions;
- Jolt Console for approving and revoking app access;
- `.jolt` identity addresses backed by signing keys;
- signed update logs for mutable identity-owned paths;
- content-addressed publish/fetch;
- encrypted content envelopes;
- app-scoped APIs with capability checks;
- peer discovery, local caching, and relay/discovery experiments;
- signed reachability records;
- recipient-controlled ingress for two-way app communication.

A separate application,
[Spoke](https://github.com/alexanderwanyoike/spoke), has been used as a proof
that external apps can use these primitives through scoped daemon access. Spoke
is useful evidence for the protocol boundary, but it is not part of this repo
and does not define Jolt's data model.

## Who Jolt Is For

Jolt is currently for:

- protocol and application developers building on platformless distribution;
- people operating or experimenting with peer, cache, relay, and availability
  infrastructure;
- creators or communities who want portable publishing under identities they
  control;
- researchers, hackers, and protocol builders evaluating user-owned identity,
  signed state, encrypted content, and peer-aware transport;
- future app developers who want to build interfaces without owning the user's
  keys, account, or distribution channel.

Jolt is not currently for non-technical users. The setup is still too manual,
identity addresses are not friendly, and the first installable distribution is
not done yet.

## What Jolt Is Not

Jolt is not a blockchain. It does not provide global consensus, tokens, mining,
staking, smart contracts, or a shared economic ledger.

Jolt is not a cryptocurrency project. There is no native coin, token incentive
system, or financial layer required for the protocol to work.

Jolt is not IPFS. Although it uses content addressing and peer-to-peer retrieval
ideas, it is not trying to be a universal distributed filesystem or permanent
public storage network.

Jolt is not BitTorrent. Content transfer is one capability, but Jolt is not
primarily a swarm-based file-sharing protocol.

Jolt is not a backend framework. It does not replace Rails, Django, NestJS,
Laravel, or other application frameworks. Jolt provides lower-level network,
identity, content, reachability, and permission primitives that applications can
build on.

Jolt is not just CRUD over the network. The goal is not to recreate REST APIs in
a distributed shape, but to enable peer discovery, routing, relay-assisted
communication, signed content distribution, and direct content exchange.

Jolt is not Tor. Relays exist to improve reachability, handshakes, routing,
content availability, and peer discovery. Anonymity routing is not the default
goal.

Jolt is not a mesh VPN. It does not route arbitrary IP traffic or create a
private LAN between machines. It operates at the application transport and
content-distribution layer.

Jolt is not trying to replace the internet. It is designed to augment existing
networks with peer-aware communication for applications that benefit from
decentralized transport and platformless distribution.

Jolt is not a hosted social app. It can carry social applications, but those
applications are consumers of the network. Jolt itself is the underlying
protocol, local runtime, and peer network for content syndication.

## Current Limitations

The main v0 limitation is that the application-daemon boundary is not settled.
The current app APIs proved scoped authority, publish/fetch flows, and recipient
ingress, but the REST-style boundary may not be the right long-term interface
for live application state, subscriptions, and efficient local materialized
views.

Other important limitations:

- **No packaged install yet:** users still need developer tooling.
- **Identity UX is rough:** `.jolt` addresses are long and not human-friendly.
- **No global discovery/search:** users need to know identities or receive them
  out of band.
- **Offline ingress is not solved:** direct recipient ingress works when the
  recipient is reachable; store-and-forward needs more design.
- **Relay policy needs hardening:** pinning must be authorized and abuse-limited.
- **Security needs review:** standard crypto primitives are used, but v0 has not
  had a full security review.

## Project Status

Jolt is now in a v0 freeze posture.

The experiment is mildly successful: Jolt can distribute signed content by
identity, external apps can use scoped local authority, and recipient ingress
proves two-way application communication at the protocol boundary. The next
question is not "can this work?" The next question is what the right v0 shape
should be.

Before adding more features, Jolt needs deeper review:

- project and protocol review;
- protocol optimization and security review;
- project documentation;
- v0 RFC design;
- a clearer application-daemon interface. The current REST-style boundary works
  for proving the idea, but it may not be the right long-term interface for
  local apps that need live state, subscriptions, and efficient materialized
  views.

New protocol features, app-store work, global search, relay metrics, and richer
application use-cases should wait until that review is done.

## Architecture

```text
Jolt Console
  local control surface for daemon lifecycle, settings, and app permissions

Jolt daemon
  identity keys
  signed update logs
  content store
  encryption/decryption
  app capability enforcement
  peer and relay networking
  recipient ingress

Apps
  untrusted clients with scoped permissions

Network
  peers
  caches
  optional relays
  provider discovery
```

The protocol layer stays app-agnostic. It knows about identities, content IDs,
signed paths, update logs, reachability, encrypted objects, relays, pinning, and
capabilities. It does not know about posts, feeds, profiles, timelines, inboxes,
or contacts. Those are app-level schemas.

## Try It

Jolt does not yet have a normal packaged install. For now this is a developer
workflow.

Prerequisite:

- Rust 1.89+

Build Jolt:

```bash
cargo build --locked
```

Run a local daemon:

```bash
cargo run -p jolt-node -- start
```

Check status:

```bash
curl -fsS http://127.0.0.1:9862/api/v1/status | jq .
```

Run Jolt Console:

```bash
cd apps/jolt-console
npm install
npm run tauri dev
```

Try an external application proof:

```bash
git clone https://github.com/alexanderwanyoike/spoke
```

Spoke is a separate application that exercises Jolt app sessions and recipient
ingress. It is not part of this protocol repo and does not define the protocol
model. The setup is still manual.

## Developer Notes

Crates:

| Crate | Purpose |
|---|---|
| `jolt-core` | Content IDs, `.jolt` addresses, reachability records, shared protocol types |
| `jolt-identity` | Ed25519 identity key management, signing, verification |
| `jolt-store` | Local content store, cache, pinning, eviction |
| `jolt-network` | Daemon node, P2P networking, fetch/resolve/update-log flows |
| `jolt-server` | HTTP daemon API and app API |
| `jolt-node` | CLI binary and daemon commands |
| `apps/jolt-console` | Tauri desktop Console |

Normal local verification:

```bash
./scripts/test-local.sh
```

Focused checks:

```bash
cargo test --locked --workspace --exclude jolt-console
npm test --prefix apps/jolt-console
npm run build --prefix apps/jolt-console
```

## Documentation

Design notes and implementation cards live in [`docs/`](docs/). Current planning
cards are in [`docs/cards/`](docs/cards/).

## License

MIT
