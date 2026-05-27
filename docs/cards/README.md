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

Next:
  Resolve a user's latest signed state and keep it reachable through relays.
```

The next useful product/protocol bridge is:

> Alice publishes a signed mutable web presence. Her home relay pins it. Bob can resolve Alice and fetch the latest content while Alice's device is offline.

## Current Focus

Do not start with WASM apps, storage markets, payments, or relay-to-relay replication.

The current focus is:

1. Stabilize and clarify the existing proof.
2. Add a local dashboard so nodes and relays are observable.
3. Make a local two-node dashboard demo reliable.
4. Implement signed mutable records.
5. Resolve latest state by identity.
6. Add local petnames so people do not handle raw peer IDs.
7. Add home relay / owner-directed pinning.
8. Add basic availability checks.

## Cards

| Card | Type | Status | Summary |
|---|---|---|---|
| [001](001-current-state-and-test-harness.md) | AFK | Ready | Stabilize current test/dev ground and remove stale drift. |
| [002](002-local-node-dashboard-v0.md) | AFK | Ready | Add localhost dashboard for node/relay debugging. |
| [014](014-local-multi-node-demo-mode.md) | AFK | Ready after 002 | Make two local dashboard nodes connect and transfer content predictably. |
| [003](003-testing-strategy-and-harness.md) | AFK | Done | Define and automate the test layers. |
| [004](004-update-log-core.md) | AFK | Done | Add signed append-only update log primitives. |
| [005](005-resolve-latest-record.md) | AFK | Done | Resolve latest signed state for an identity. |
| [015](015-local-petnames-and-address-book.md) | AFK | Ready | Add local aliases for peer IDs before human-facing profile/feed work. |
| [006](006-profile-and-feed-v0.md) | AFK | Ready | Publish and resolve a minimal profile/feed. |
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
