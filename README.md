# Jolt

Jolt is a peer-to-peer distributed content syndication network.

It is for platformless content distribution: a person or community publishes
content under their own cryptographic identity, other nodes can discover and
fetch that content, and apps can build experiences on top without owning the
audience, the account, or the keys.

Jolt is experimental v0 software. The protocol and app boundary work, and the
[Spoke](https://github.com/alexanderwanyoike/spoke) social proof-of-concept can
publish posts and send replies between users. The system is still eventually
consistent, developer-oriented, and not yet packaged for normal users.

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

The core idea is syndication, not a single social network. A Jolt identity can
publish signed updates. Different apps can read those updates and render them as
a feed, profile, gallery, release channel, notebook, or community space.

## What Works Today

Jolt v0 has working implementations of the core pieces:

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

The current human-facing proof is
[Spoke](https://github.com/alexanderwanyoike/spoke), a small social app PoC.
Spoke can:

- create a local profile;
- publish posts under a Jolt identity;
- add known contacts by `.jolt` identity;
- read contact feeds;
- send encrypted replies through recipient ingress;
- auto-accept replies from known contacts while leaving unknown senders for
  manual review.

This is enough to show that Jolt can support platformless social-style
distribution. It is not yet enough to claim polished social-network UX.

## Who Jolt Is For

Jolt is currently for:

- developers exploring local-first or peer-to-peer apps;
- people interested in content distribution that is not tied to one platform;
- creators or communities who want portable publishing and audience
  relationships;
- researchers, hackers, and protocol builders evaluating user-owned identity and
  signed content systems;
- future app developers who want to build interfaces without owning the user's
  keys or account.

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

Jolt is not a hosted social app. It can support social applications, and Spoke
is the current proof, but Jolt itself is the underlying network and local
runtime for content syndication.

## Current Limitations

The main v0 limitation is product feel. Spoke works, but it feels eventually
consistent because it polls the daemon and rebuilds local app state through
resolve/fetch/materialization. That is acceptable for proving the model, but not
for a polished social product.

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
identity, external apps can use scoped local authority, and Spoke proves
two-way app communication. The next question is not "can this work?" The next
question is what the right v0 shape should be.

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
social features should wait until that review is done.

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
  example: Spoke

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

Try the Spoke PoC:

```bash
git clone https://github.com/alexanderwanyoike/spoke
```

Spoke currently needs a local Jolt daemon and a dev server pointed at that
daemon. The setup is still manual.

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
