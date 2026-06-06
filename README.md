# Jolt

Jolt is a local-first peer-to-peer runtime for user-owned apps.

The short version: Jolt gives a user a cryptographic identity, a local daemon,
scoped app permissions, signed `.jolt` state, encrypted content, optional relay
availability, and recipient-controlled ingress for two-way app communication.
Apps such as Pastey and Spoke run outside the daemon and ask the local user for
permission before acting as that identity.

Jolt is not a finished product. v0 is a successful experiment: the core pieces
work well enough to build with, but the experience is still eventually
consistent, developer-oriented, and not yet packaged for normal users.

## Why

Most social, community, and collaboration tools bundle four things together:

| Platform bundle | Jolt primitive |
|---|---|
| Account identity | User-owned signing keys and `.jolt` identity addresses |
| Hosted content | Content-addressed blobs, signed update logs, encrypted envelopes |
| Platform distribution | Peer discovery, caching, optional relays, owner-directed pinning |
| App authority | Scoped local app sessions approved in Jolt Console |

Jolt's bet is that apps should not own the user's identity or keys. The daemon
is the local authority. Apps are untrusted clients that request specific
capabilities, such as publishing under `/spoke/*`, decrypting private Pastey
objects, or sending recipient ingress.

That model is useful for apps where authorship, portability, private sharing,
and continuity matter more than global platform reach.

## v0 Verdict

The v0 experiment proved enough to stop feature-building and judge the idea:

- A local daemon can own identity, keys, storage, networking, and app sessions.
- Jolt Console can approve/revoke app authority and start/manage the daemon.
- External apps can use Jolt without receiving private keys.
- Pastey proves public and private encrypted sharing.
- Spoke proves a small social app with posts, contacts, replies, and two-way
  recipient ingress.
- Relay/discovery work is good enough for v0; deeper relay operations can wait.

The main weakness is product feel. Spoke works, but it feels eventually
consistent because app state is rebuilt through polling, resolve, fetch, and
local materialization. That is acceptable for v0, but it is not yet the native,
realtime feel people expect from social or messaging software.

## What Works

Core protocol/runtime:

- Content-addressed publish/fetch by CID.
- Ed25519 identity keys and canonical `{identity}.jolt` addresses.
- Signed update logs for mutable identity-owned paths.
- `.jolt` resolve/fetch through local daemon APIs.
- Local content store, cache, pinning, and re-sharing.
- Kademlia/provider discovery and deterministic local TCP test transport.
- iroh transport path for real P2P/NAT traversal experiments.
- Relay mesh/discovery and owner-directed home relay pinning.
- Signed reachability records for recipient ingress.

App/security boundary:

- HTTP daemon API and capability-checked `/app/v1/*` API.
- App session request, approval, revocation, and capability scoping.
- Jolt Console as the privileged local trust surface.
- Apps can publish/fetch/resolve only within approved scopes.
- Apps can request encryption/decryption without receiving private keys.
- Apps can send recipient ingress by identity without manually entering receiver
  URLs.

Proof apps:

- [Pastey](https://github.com/alexanderwanyoike/pastey): public and private
  paste sharing over Jolt app sessions.
- [Spoke](https://github.com/alexanderwanyoike/spoke): identity-owned social
  notebook PoC with contacts, feeds, posts, encrypted replies, and known-contact
  auto-accept.

## What Needs Work

These are not hidden. They are the current v0 limitations.

- **Distribution:** Jolt still needs a packaged Console + daemon + optional CLI
  artifact. Users should not build from source or manually start several
  processes.
- **Realtime/local materialization:** apps currently poll and rebuild state.
  Jolt needs a better local app/daemon interface for subscriptions or
  materialized app views.
- **Performance:** recursive resolve/fetch paths have been reduced, but social
  feed refreshes can still feel slow.
- **Identity UX:** `.jolt` identity addresses are not human-friendly. Apps can
  use local contact names, but global search/naming is intentionally not solved.
- **Offline ingress:** direct recipient ingress works when the recipient is
  reachable. Store-and-forward/offline inbox semantics need a separate design.
- **Relay policy:** pinning must remain owner-directed and authorized. Public
  pinning, quotas, abuse limits, and relay operator policy need hardening.
- **Operational polish:** logs, metrics, diagnostics, app setup docs, and reset
  flows need product-level cleanup.
- **Security review:** crypto uses standard primitives, but the whole system
  still needs review before anyone treats it as production security software.

## Architecture

```text
Jolt Console
  privileged local UI for daemon lifecycle, permissions, settings, diagnostics

External apps
  Pastey, Spoke, future apps
  untrusted clients using scoped app sessions

Jolt daemon
  identity keys
  content store
  signed update logs
  encryption/decryption APIs
  app session authority
  reachability and ingress
  P2P networking and relay discovery

Jolt network
  peers
  relays
  provider discovery
  cached and pinned content
```

The protocol layer stays app-agnostic. It knows about identities, content IDs,
signed paths, update logs, reachability, encrypted objects, relays, pinning, and
capabilities. It does not know about Spoke posts, Pastey pastes, feeds, inboxes,
profiles, timelines, or contacts. Those are app-level schemas stored as signed
content.

## Crates

| Crate | Purpose |
|---|---|
| `jolt-core` | Content IDs, `.jolt` addresses, reachability records, shared protocol types |
| `jolt-identity` | Ed25519 identity key management, signing, verification |
| `jolt-store` | Local content store, cache, pinning, eviction |
| `jolt-network` | Daemon node, P2P networking, fetch/resolve/update-log flows |
| `jolt-server` | HTTP daemon API and app API |
| `jolt-node` | CLI binary and daemon commands |
| `apps/jolt-console` | Tauri desktop Console |

## Quick Start For Developers

Prerequisite:

- Rust 1.89+

Build:

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

Publish content:

```bash
curl -fsS -F "file=@README.md" http://127.0.0.1:9862/api/v1/publish | jq .
```

Fetch content:

```bash
curl -fsS -X POST http://127.0.0.1:9862/api/v1/fetch \
  -H 'content-type: application/json' \
  -d '{"content_id":"bafkr4i..."}' | jq .
```

Run Jolt Console in development:

```bash
cd apps/jolt-console
npm install
npm run tauri dev
```

## Demo Apps

Pastey and Spoke live outside this repository.

```bash
git clone https://github.com/alexanderwanyoike/pastey
git clone https://github.com/alexanderwanyoike/spoke
```

Current local demos use isolated Alice/Bob daemon data directories and Vite dev
servers pointed at different daemon API ports. They are useful for proving the
app boundary, permissions, private content, and recipient ingress, but they are
not yet a normal-user install flow.

Pastey has a harness in this repository for deterministic app-API smoke checks:

```bash
./scripts/pastey-two-node-demo.sh --smoke --no-pastey
```

For the full human demo, run Jolt Console, run two daemons, start Pastey or
Spoke against each daemon, approve app sessions in Console, then test:

- public publish/fetch;
- private encrypted publish/open;
- contact feed reading;
- recipient ingress replies;
- known-contact auto-accept in Spoke.

## API Snapshot

Public daemon API:

```text
GET    /api/v1/health
GET    /api/v1/status
GET    /api/v1/peers
POST   /api/v1/peers/connect
POST   /api/v1/publish
POST   /api/v1/fetch
POST   /api/v1/resolve
GET    /api/v1/cache/stats
GET    /api/v1/cache/entries
POST   /api/v1/cache/pin/{id}
DELETE /api/v1/cache/pin/{id}
POST   /api/v1/ingress
```

App API, guarded by approved app sessions:

```text
GET    /app/v1/session
POST   /app/v1/publish
GET    /app/v1/published
POST   /app/v1/fetch
POST   /app/v1/resolve
POST   /app/v1/encrypted/publish
POST   /app/v1/encrypted/decrypt
POST   /app/v1/encrypted/open
GET    /app/v1/ingress/pending
POST   /app/v1/ingress/send
POST   /app/v1/ingress/{id}/open
POST   /app/v1/ingress/{id}/accept
POST   /app/v1/ingress/{id}/reject
```

Admin/Console API:

```text
GET    /admin/v1/app-requests
POST   /admin/v1/app-requests/{id}/approve
POST   /admin/v1/app-requests/{id}/reject
GET    /admin/v1/app-sessions
POST   /admin/v1/app-sessions/{id}/revoke
```

## Testing

Normal local verification:

```bash
./scripts/test-local.sh
```

That script runs the deterministic Rust workspace checks and the Pastey demo
harness contract check.

Focused checks used heavily during v0:

```bash
cargo test --locked --workspace --exclude jolt-console
npm test --prefix apps/jolt-console
npm run build --prefix apps/jolt-console
```

Spoke and Pastey have their own `npm test` and `npm run build` checks in their
separate repositories.

Manual network checks still matter. The strongest confidence test is a public
bootstrap/relay node plus two client machines on different networks, including
one behind CGNAT/mobile when possible.

## v0 Freeze

Jolt is now in a v0 freeze posture:

- no new protocol features;
- no new Console surfaces unless needed for setup or bug fixes;
- no app store/catalog work;
- no relay metrics/structured-logs push until product use is clearer;
- bug fixes, packaging, docs, demos, and setup polish only.

The next meaningful product step is distribution: ship a Jolt Console desktop
app that bundles the daemon sidecar and gives users one obvious way to start,
stop, configure, and approve apps.

## Roadmap After v0

Only continue if the v0 demos create real interest.

High-value next work:

- packaged Jolt Console + daemon + optional CLI;
- better app/daemon state subscriptions or materialized views;
- clearer identity/contact/invite UX;
- offline recipient ingress/store-and-forward design;
- relay abuse controls, quotas, pin authorization, and operator diagnostics;
- richer Pastey/Spoke docs and install instructions.

Deferred until there is clear demand:

- app store/catalog inside Console;
- global search;
- protocol-level social/feed semantics;
- OS service/tray/menu-bar lifecycle;
- WASM app runtime;
- streaming/media-specific transport work.

## Documentation

Design notes and implementation cards live in [`docs/`](docs/). Current planning
cards are in [`docs/cards/`](docs/cards/).

## License

MIT
