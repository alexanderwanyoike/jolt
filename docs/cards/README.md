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

Next:
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
9. Add bootstrap relay mesh behavior so fresh nodes can join global discovery.
10. Wire `.jolt` resolution into CLI, API, and dashboard.
11. Fetch content by `.jolt` address, not just by CID.
12. Add home relay / owner-directed pinning.
13. Prove Alice-offline/Bob-fresh fetch through a relay.
14. Add local petnames after the global path works.

## Cards

| Card | Type | Status | Summary |
|---|---|---|---|
| [001](001-current-state-and-test-harness.md) | AFK | Ready | Stabilize current test/dev ground and remove stale drift. |
| [002](002-local-node-dashboard-v0.md) | AFK | Ready | Add localhost dashboard for node/relay debugging. |
| [014](014-local-multi-node-demo-mode.md) | AFK | Ready after 002 | Make two local dashboard nodes connect and transfer content predictably. |
| [003](003-testing-strategy-and-harness.md) | AFK | Done | Define and automate the test layers. |
| [004](004-update-log-core.md) | AFK | Done | Add signed append-only update log primitives. |
| [005](005-resolve-latest-record.md) | AFK | Done | Resolve latest signed state for an identity. |
| [016](016-global-identity-address-v0.md) | AFK | Done | Add canonical `{identity}.jolt` addresses before local petnames. |
| [017](017-global-jolt-resolution-v0.md) | AFK | Done | Define global `.jolt` resolution through signed reachability records. |
| [018](018-global-update-log-discovery-v0.md) | AFK | Done | Discover, fetch, verify, and cache signed update logs for global `.jolt` lookup. |
| [019](019-bootstrap-relay-mesh-v0.md) | HITL | Ready | Define and implement the first global bootstrap/relay mesh path. |
| [020](020-jolt-resolve-api-cli-dashboard.md) | AFK | Blocked by 019 | Let users resolve `.jolt` addresses through CLI, API, and dashboard. |
| [021](021-fetch-by-jolt-address.md) | AFK | Blocked by 006, 020 | Fetch content from a `.jolt` address instead of a raw CID. |
| [022](022-offline-publisher-through-relay-smoke-test.md) | AFK | Blocked by 009, 021 | Prove Bob can fetch Alice's content while Alice is offline. |
| [015](015-local-petnames-and-address-book.md) | AFK | Deferred until 020 | Add local aliases for identity addresses after global resolution is usable. |
| [006](006-profile-and-feed-v0.md) | AFK | Ready | Publish and resolve a minimal signed space/feed. |
| [007](007-home-relay-configuration.md) | AFK | Ready | Configure a user's home relay. |
| [008](008-owner-signed-pin-protocol.md) | AFK | Blocked by 004, 007 | Define and implement owner-signed pin requests. |
| [009](009-relay-pinning-and-provider-announcement.md) | AFK | Blocked by 008 | Relay accepts pins, stores content, announces providers. |
| [010](010-offline-fetch-through-home-relay.md) | AFK | Blocked by 006, 009 | End-to-end offline publisher flow. |
| [011](011-availability-checks-v0.md) | AFK | Blocked by 009 | Node checks whether home relay still serves pinned content. |
| [012](012-crypto-agility-spike.md) | HITL | Later | Decide post-quantum-aware encryption direction. |
| [013](013-wasm-runtime-parking-lot.md) | HITL | Later | Park app runtime work until relay/mutable content lands. |

## Card Format

Each card uses:

- **Type:** AFK means an agent should be able to implement it without new product decisions. HITL means the card needs human direction first.
- **Blocked by:** Cards that should land first.
- **What to build:** End-to-end behavior, not just a layer.
- **Acceptance criteria:** Concrete checks for completion.
