# Work Cards

This folder tracks the next practical slices of Jolt work.

## Where We Are

Jolt has proved the hard transport path:

- Nodes can discover each other.
- Nodes can connect across NAT and CGNAT through iroh/libp2p.
- Nodes can publish immutable content by CID.
- Other nodes can fetch, verify, cache, and re-serve that content.
- The daemon and HTTP API exist.

The project is currently between two phases:

```text
Done:
  Fetch immutable content from peers.
  Resolve signed state from verified update logs in deterministic tests.

Now proven:
  Let a fresh node join the mesh, resolve a .jolt space/community address, and fetch while the publisher is offline.
```

The next useful product/protocol bridge is:

> Alice creates a signed space/community. Her home relay pins its content and update log. Bob can resolve Alice and fetch authorized content while Alice's device is offline.

There is also a product risk to keep visible while choosing technical slices:

> Why would anyone run or join this network before it has a strong first use case?

The answer should shape the next proof. This is a product discussion, not an implementation card. Jolt should not drift into a general storage-market project before it can demonstrate one concrete thing communities want that centralized platforms make awkward.

## Current Focus

Do not start with WASM apps, storage markets, payments, or storage-market mechanics.

Relay-to-relay communication is now part of the global discovery problem, not a future monetization feature. The v0 version should focus on bootstrapping, DHT/provider discovery, and signed update-log reachability. It should not copy user content between relays without owner intent.

The current focus is:

1. Stabilize and clarify the existing proof.
2. Add a local dashboard so nodes and relays are observable.
3. Make a local two-node dashboard demo reliable.
4. Implement signed mutable records.
5. Resolve latest state by identity.
6. Add canonical identity addresses so people can be addressed globally.
7. Design and implement global `.jolt` resolution through signed reachability.
8. Add network-backed update-log discovery for global `.jolt` lookup.
9. Add bootstrap config and relay mode so fresh nodes can have durable entry points.
10. Add bootstrap management UX.
11. Prove DHT-backed update-log discovery through a configured bootstrap relay.
12. Cache discovered relay/peer addresses after a successful join.
13. Make bootstrap/discovery state observable in status and dashboard.
14. Wire `.jolt` resolution into CLI, API, and dashboard.
15. Fetch content by `.jolt` address, not just by CID.
16. Add home relay / owner-directed pinning.
17. Prove Alice-offline/Bob-fresh fetch through a relay.
18. Add user-facing publish-to-home-relay pinning.
19. Make local published state and relay-backed state understandable in the dashboard.
20. Show a built-in space/application demo before committing to a WASM runtime.
21. Define signed relay records.
22. Add a bounded relay address book.
23. Let nodes and relays exchange verified relay records.
24. Let relays explore the relay mesh from one known relay.
25. Add relay-to-relay identity provider query forwarding.
26. Add bounded identity-head gossip for common lookups.
27. Make identity-head gossip batches fair across identities.
28. Make relay discovery and `.jolt` resolution failures explainable.
29. Run one relay-mesh milestone canary after the local slices pass.
30. Add local petnames after the global path works.

## Next Sprint: App Boundary and Private Sharing Foundations

Pastey proved that a separate app can consume Jolt through the daemon. That moves the next useful work away from more raw protocol plumbing and toward the daemon/app boundary:

```text
Jolt daemon = local authority, identities, keys, network access
Jolt Console = privileged local control surface
Jolt apps = untrusted clients with scoped sessions
```

The immediate next sprint should focus on:

1. Designing app sessions and capability grants.
2. Adding a session store and approval API.
3. Adding capability-checked app-facing endpoints.
4. Turning the dashboard into a Jolt Console shell.
5. Moving Pastey from trusted `/api/v1/*` calls to app sessions.
6. Designing encrypted object envelopes and crypto agility before private Pastey.

Keep Drops out of this sprint. Pastey is already enough pressure for the daemon/app boundary and private sharing model.

## Cards

| Card | Type | Status | Summary |
|---|---|---|---|
| [001](001-current-state-and-test-harness.md) | AFK | Done | Stabilize current test/dev ground and remove stale drift. |
| [002](002-local-node-dashboard-v0.md) | AFK | Superseded by 045 | Original localhost dashboard card; continue through Jolt Console work. |
| [014](014-local-multi-node-demo-mode.md) | AFK | Superseded by 054 | Original local dashboard demo; continue through Pastey two-node harness. |
| [003](003-testing-strategy-and-harness.md) | AFK | Done | Define and automate the test layers. |
| [004](004-update-log-core.md) | AFK | Done | Add signed append-only update log primitives. |
| [005](005-resolve-latest-record.md) | AFK | Done | Resolve latest signed state for an identity. |
| [016](016-global-identity-address-v0.md) | AFK | Done | Add canonical `{identity}.jolt` addresses before local petnames. |
| [017](017-global-jolt-resolution-v0.md) | AFK | Done | Define global `.jolt` resolution through signed reachability records. |
| [018](018-global-update-log-discovery-v0.md) | AFK | Done | Discover, fetch, verify, and cache signed update logs for global `.jolt` lookup. |
| [019](019-bootstrap-config-and-relay-mode-v0.md) | AFK | Done | Add persistent bootstrap config and explicit bootstrap/discovery relay mode. |
| [023](023-bootstrap-management-ux.md) | AFK | Done | Add CLI UX for listing, adding, and removing bootstrap relay addresses. |
| [024](024-dht-bootstrap-discovery-path.md) | AFK | Done | Prove Bob can discover Alice's update-log provider through a configured relay and DHT. |
| [025](025-discovered-relay-peer-cache.md) | AFK | Done | Cache useful discovered relay/node addresses for future starts. |
| [026](026-bootstrap-observability.md) | AFK | Done | Expose bootstrap state through status/API/dashboard. |
| [027](027-relay-gossip-v0.md) | HITL | Split into 033-039 | Umbrella design card for relay discovery, relay gossip, and identity/provider hints. |
| [028](028-three-node-canary-harness.md) | AFK | Done | Document/run Alice -> Relay -> Bob local test and real-world canary. |
| [020](020-jolt-resolve-api-cli-dashboard.md) | AFK | Done | Let users resolve `.jolt` addresses through CLI, API, and dashboard. |
| [021](021-fetch-by-jolt-address.md) | AFK | Done | Fetch content from a `.jolt` address instead of a raw CID. |
| [022](022-offline-publisher-through-relay-smoke-test.md) | AFK | Done | Prove Bob can fetch Alice's content while Alice is offline. |
| [015](015-local-petnames-and-address-book.md) | AFK | Deferred | Add local aliases for identity addresses after app sessions/identity UX settle. |
| [006](006-signed-path-publishing-v0.md) | AFK | Done | Publish and resolve generic signed path bindings. |
| [030](030-persistent-update-log-store.md) | AFK | Done | Persist the owner's update log so `.jolt` paths survive daemon restarts. |
| [007](007-home-relay-configuration.md) | AFK | Done | Configure a user's home relay. |
| [029](029-home-relay-publish-pinning-ux.md) | AFK | Done | Let users pin published content to their configured home relay from API, CLI, and dashboard. |
| [031](031-published-content-inventory-dashboard.md) | AFK | Done | Show local published content, relay pin state, stale paths, and repin actions in the dashboard. |
| [032](032-built-in-space-lens-demo.md) | HITL | Superseded by external app prototypes | Pastey/Drops should pressure-test app shape outside the protocol repo. |
| [033](033-relay-records-v0.md) | AFK | Done | Define signed relay records so relays can describe how they are reached. |
| [034](034-relay-address-book-v0.md) | AFK | Done | Persist verified relay records with expiry, deduplication, and bounds. |
| [035](035-relay-record-exchange-v0.md) | AFK | Done | Let nodes and relays exchange bounded sets of verified relay records. |
| [036](036-relay-mesh-exploration-v0.md) | AFK | Done | Let a relay with one known relay discover more of the relay mesh. |
| [037](037-identity-provider-query-forwarding-v0.md) | AFK | Done | Let relays forward identity/update-log provider queries across known relay neighbours. |
| [038](038-identity-head-gossip-v0.md) | AFK | Done | Exchange signed, expiring identity-head hints for common `.jolt` lookups. |
| [041](041-identity-head-gossip-fair-batching.md) | AFK | Done | Keep identity-head gossip batches fair across identities and requested limits. |
| [039](039-relay-discovery-failure-ux.md) | AFK | Done | Return clear failure reasons when relay discovery or `.jolt` resolution fails. |
| [040](040-relay-mesh-milestone-canary.md) | AFK | Done | Run one real-world relay-mesh canary after local process demos pass. |
| [008](008-owner-signed-pin-protocol.md) | AFK | Done | Define and implement owner-signed pin requests. |
| [009](009-relay-pinning-and-provider-announcement.md) | AFK | Done | Relay accepts pins, stores content, announces providers. |
| [010](010-offline-fetch-through-home-relay.md) | AFK | Superseded by 022 | End-to-end offline publisher flow. |
| [011](011-availability-checks-v0.md) | AFK | Done | Node checks whether home relay still serves pinned content. |
| [012](012-crypto-agility-spike.md) | HITL | Superseded by 049 | Original crypto-agility spike; continue through encrypted object envelope work. |
| [013](013-wasm-runtime-parking-lot.md) | HITL | Later | Park app runtime work until relay/mutable content lands. |
| [042](042-app-boundary-session-design.md) | HITL | Ready for review | Define daemon/app sessions, capabilities, console/admin boundary, and forbidden app powers. |
| [043](043-app-session-store-approval-api.md) | AFK | Ready after 042 | Persist pending/approved/revoked app sessions and approval APIs. |
| [044](044-capability-checked-app-api-v0.md) | AFK | Ready after 043 | Add session-token app APIs for resolve/fetch/publish/inventory/pin. |
| [045](045-jolt-console-shell-v0.md) | AFK | Ready after 042 | Turn the dashboard into a local daemon console with sidebar sections. |
| [046](046-app-permission-approval-ui.md) | AFK | Ready after 043 and 045 | Let Console approve/reject/revoke app permission requests. |
| [047](047-pastey-app-session-integration.md) | AFK | Ready after 044 and 046 | Move Pastey from trusted daemon APIs to session-token app APIs. |
| [048](048-identity-import-v0.md) | HITL | Ready for design | Define admin-only identity import/export v0 and shared-key risks. |
| [049](049-crypto-agility-encrypted-object-envelope.md) | HITL | Ready | Define encrypted object envelope, suite IDs, wrapping, and PQ-hybrid direction. |
| [050](050-identity-encryption-key-records.md) | AFK | Ready after 049 | Publish and resolve signed public encryption keys for identities. |
| [051](051-encrypted-object-implementation-v0.md) | AFK | Ready after 049 and 050 | Encrypt content once and wrap content keys for recipients. |
| [052](052-daemon-encrypt-decrypt-api.md) | AFK | Ready after 044 and 051 | Let the daemon encrypt/decrypt for app sessions without exposing keys. |
| [053](053-pastey-private-paste-v0.md) | AFK | Ready after 052 | Prove Alice can share an encrypted Pastey paste with Bob. |
| [054](054-pastey-two-node-local-demo-harness.md) | AFK | Ready | Add a repeatable local Alice/Bob/Pastey demo harness. |

## Card Format

Each card uses:

- **Type:** AFK means an agent should be able to implement it without new product decisions. HITL means the card needs human direction first.
- **Blocked by:** Cards that should land first.
- **What to build:** End-to-end behavior, not just a layer.
- **Acceptance criteria:** Concrete checks for completion.

## Relay Gossip Verification

Relay discovery and gossip cards should not each require a real internet canary.

Use this rule:

- Automated deterministic tests are required for every implementation card.
- One-machine multi-process demos are required when the card changes visible relay/network behaviour.
- The Hetzner/local-machine canary should happen once at the milestone boundary, not after every small slice.
