# Global Jolt Resolution

## Goal

Global Jolt resolution turns a canonical identity address into verified, reachable content:

```text
{identity}.jolt/path
  -> signed mutable identity state
  -> ContentId for /path
  -> reachability hints
  -> fetch from a provider or relay
```

This is the protocol bridge between "Jolt has global identity addresses" and "Bob can open Alice's content without knowing peer IDs or multiaddrs."

## Non-Goals

This first design does not solve:

- Global usernames.
- DNS integration.
- Payments or storage markets.
- Relay ranking.
- Relay-to-relay replication.
- Consensus over identity state.

The only authority for an identity is that identity's signing key.

## Core Rule

Relays and the DHT are carriers, not authorities.

Bob may learn about Alice from a relay, the DHT, a cache, or an imported invite, but Bob only accepts mutable identity state if it verifies against Alice's identity key.

```text
Discovery answers: "someone may have Alice's record."
Alice's signature answers: "this is Alice's record."
Content hashes answer: "these bytes match this content ID."
```

## Identity State

Alice's update log remains the source of mutable truth for her Jolt address.

The resolver should treat the latest verified update-log state as:

```text
ResolvedIdentityState {
  identity: IdentityId,
  latest_sequence: u64,
  paths: Map<String, ContentId>,
  profile: Option<Profile>,
  reachability: ReachabilitySet,
}
```

Paths answer what content Alice is publishing:

```text
/profile -> ContentId
/feed    -> ContentId
/posts/hello -> ContentId
```

Reachability answers where Bob can try to fetch that content or sync Alice's update log.

## Reachability Action

For v0, reachability should be an update-log action, not a separate unsigned or separately sequenced record.

Reasons:

- It reuses the existing signature and hash-chain machinery.
- It gives reachability a clear sequence ordering.
- It lets one replay produce both content paths and reachability.
- It prevents relays from becoming the source of truth for Alice's current location.

Proposed action:

```text
UpdateAction::SetReachability {
  relays: Vec<RelayHint>,
}
```

`SetReachability` replaces the previous reachability set. This is simpler than partial add/remove actions for v0 and makes stale relay removal explicit.

Relay hint:

```text
RelayHint {
  identity: IdentityId,
  peer_id: String,
  addresses: Vec<String>,
  capabilities: Vec<RelayCapability>,
  expires_at: Option<u64>,
}
```

Capabilities:

```text
RelayCapability::Discovery
RelayCapability::Pinning
RelayCapability::Serving
```

Field meanings:

| Field | Meaning |
|---|---|
| `identity` | The relay's long-lived Jolt identity. |
| `peer_id` | The relay's current libp2p transport peer ID. |
| `addresses` | Dialable multiaddrs known at the time Alice signed the entry. |
| `capabilities` | What Alice believes this relay can do for her. |
| `expires_at` | Optional Unix timestamp after which Bob should treat the hint as stale. |

## Discovery

Resolution has two separate phases:

1. Find candidates who may have Alice's signed update log.
2. Verify and replay Alice's signed update log.

The v0 implementation can stage discovery in three steps.

### Stage 0: Local/Provided Record

The pure resolver accepts:

```text
JoltAddress + verified update log entries
```

and returns:

```text
ResolvedJoltTarget {
  identity: IdentityId,
  path: String,
  content_id: ContentId,
  reachability: Vec<RelayHint>,
}
```

This is enough to test the protocol semantics without network lookup.

### Stage 1: Known Relay

Bob can ask a known relay for Alice's update log:

```text
Request:  { identity: IdentityId, since: Option<u64> }
Response: { entries: Vec<UpdateLogEntry> }
```

The relay may lie, omit entries, or be stale. Bob still verifies signatures, sequence continuity, and previous-entry hashes. If Bob already has a newer valid sequence locally, the stale relay response is ignored.

### Stage 2: DHT Candidate Discovery

Nodes that can serve Alice's update log announce themselves under a deterministic provider key derived from Alice's identity:

```text
jolt:update-log:{identity}
```

Bob queries providers for that key, dials candidates, asks for Alice's update log, and verifies the returned log.

The DHT does not store Alice's signed state and does not decide what is latest. It only helps Bob find nodes to ask.

## Resolution Flow

For:

```text
alice_identity.jolt/profile
```

Bob's node should:

1. Parse the address into `IdentityId` and `/profile`.
2. Load the newest locally cached verified log for that identity, if any.
3. Discover candidate providers or relays for that identity.
4. Request update-log entries from candidates.
5. Verify signatures, sequence ordering, owner identity, and previous-entry hashes.
6. Keep the highest valid contiguous sequence.
7. Replay the log into `ResolvedIdentityState`.
8. Resolve `/profile` to a `ContentId`.
9. Return the `ContentId` plus reachability hints.
10. Fetch the content from any provider/relay and verify the content hash.

## Staleness and Replacement

Newer valid signed state wins.

Rules:

- A reachability hint with `expires_at` in the past is ignored.
- A lower sequence response cannot replace a higher locally verified sequence.
- A higher sequence response is accepted only if it forms a valid chain from known state or from genesis.
- `SetReachability { relays: [] }` is a valid way for Alice to remove relay hints.

## Security Properties

The design must preserve these properties:

- A relay cannot claim to be Alice without Alice's signature.
- A relay cannot change `/profile` to another `ContentId` without invalidating Alice's log signature.
- A stale relay can at worst serve older valid state; Bob can prefer the highest sequence he has seen.
- A malicious DHT provider can waste Bob's time but cannot create valid Alice state.
- Content bytes remain verified by `ContentId`.

## User Experience Target

The eventual user-facing flow should be:

```text
Bob enters: alice_identity.jolt/profile
Bob's node resolves and fetches.
Bob sees Alice's profile.
```

Later, petnames make this:

```text
alice/profile
```

where Bob's local address book maps:

```text
alice -> alice_identity.jolt
```

## Implementation Slices

Recommended implementation order:

1. Add reachability types to `dweb-core`.
2. Add `UpdateAction::SetReachability`.
3. Extend latest-record replay to include reachability.
4. Add a pure resolver from `JoltAddress + UpdateLogEntry[]` to `ResolvedJoltTarget`.
5. Add tests for valid resolution, missing path, invalid owner/signature, stale replacement, and expired hints.
6. Add update-log sync by `IdentityId` rather than transport peer ID.
7. Add relay/DHT discovery of update-log providers.

The first PR should stop at the pure resolver if needed. Network lookup can land separately.
