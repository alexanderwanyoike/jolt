# 017: Global Jolt Resolution v0

**Type:** AFK
**Milestone:** M4.5 / M5
**Status:** Done
**Blocked by:** 016

## Why

`{identity}.jolt` is only useful globally if another node can turn it into verified, reachable content.

The important protocol bridge is:

```text
{identity}.jolt
  -> latest signed identity/update record
  -> home relay or provider reachability
  -> content path
  -> ContentId
  -> fetch from a reachable provider
```

Without this, `.jolt` addresses are only display identifiers. With it, Jolt starts behaving like a decentralized web: Bob can ask for Alice's content by identity, verify Alice signed the mutable state, and fetch through whichever relay/provider is currently keeping that content online.

## What to Decide

The design pass is captured in [Global Jolt Resolution](../12-global-jolt-resolution.md).

The v0 design decisions are:

- Reachability is signed as part of Alice's update log.
- `SetReachability` replaces the prior reachability set.
- Relays and DHT provider records are discovery hints, not authorities.
- The first implementation slice should be a pure resolver from `JoltAddress + verified update log` to `ContentId + reachability hints`.
- Network lookup can stage from local/provided records to known relay lookup and then DHT candidate discovery.

The minimal design should not solve payments, relay marketplaces, social naming, or relay-to-relay replication.

## Proposed v0 Shape

Start with an update-log action that says where Alice's identity can be reached:

```text
UpdateAction::SetReachability {
  relays: Vec<RelayHint>
}

RelayHint {
  identity: IdentityId,
  peer_id: String,
  addresses: Vec<String>,
  capabilities: Vec<RelayCapability>,
  expires_at: Option<u64>
}
```

Bob resolves:

```text
{identity}.jolt/profile
```

by:

1. Finding the latest signed update log for `{identity}`.
2. Verifying the log is signed by that identity.
3. Replaying it into current state.
4. Reading the latest reachability information.
5. Reading the requested path from the resolved record.
6. Fetching the resulting `ContentId` from the relay/provider path.

## What to Build

After the design is accepted, implement the smallest useful vertical slice:

- Add a signed reachability action to the update log.
- Resolve reachability from a verified update log.
- Add tests for replacing stale reachability with a newer signed entry.
- Add a resolver function that accepts `JoltAddress` plus a verified record/log and returns the target `ContentId` plus reachability hints.
- Document the Alice/Bob flow.

Network-wide lookup can be staged:

- v0 can resolve from a provided local log/record.
- v1 can ask a configured home relay.
- v2 can use DHT/provider discovery.

## Acceptance Criteria

- [x] The signed data model for reachability is documented.
- [x] Bob never accepts reachability records unless they verify against Alice's identity.
- [x] Newer signed reachability supersedes older reachability.
- [x] `{identity}.jolt/path` can resolve to a `ContentId` from verified signed state.
- [x] Resolver returns reachability hints separately from content identity.
- [x] Tests cover valid reachability, stale replacement, invalid signature rejection, and missing path.
- [x] Docs explain why `.jolt` is not DNS and why relays are carriers, not authorities.

## Notes

This is the important global addressing card.

Petnames should remain local UX on top of this:

```text
alice -> {identity}.jolt
```

Do not add payment, storage markets, relay ranking, or global usernames here.
