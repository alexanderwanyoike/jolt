# 017: Global Jolt Resolution v0

**Type:** HITL
**Milestone:** M4.5 / M5
**Status:** Ready for design
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

This card needs a short design pass before implementation.

Decide the v0 shape for a signed reachability record:

- How Alice publishes home relay/provider information.
- Whether reachability is an `UpdateAction` in the existing update log or a separate signed record.
- What exact fields are signed.
- How records are discovered by Bob.
- Whether discovery starts local/manual, DHT-backed, relay-backed, or some combination.
- How stale relay/provider records expire or get superseded.

The minimal design should not solve payments, relay marketplaces, social naming, or relay-to-relay replication.

## Proposed v0 Shape

Start with an update-log action that says where Alice's identity can be reached:

```text
UpdateAction::SetReachability {
  home_relay_identity: IdentityId,
  home_relay_peer_id: PeerId,
  home_relay_addresses: Vec<Multiaddr>,
  capabilities: discovery | pinning | both,
  expires_at: Option<Timestamp>
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

- Add a signed reachability action or record.
- Resolve reachability from a verified update log.
- Add tests for replacing stale reachability with a newer signed entry.
- Add a resolver function that accepts `JoltAddress` plus a verified record/log and returns the target `ContentId` plus reachability hints.
- Document the Alice/Bob flow.

Network-wide lookup can be staged:

- v0 can resolve from a provided local log/record.
- v1 can ask a configured home relay.
- v2 can use DHT/provider discovery.

## Acceptance Criteria

- [ ] The signed data model for reachability is documented.
- [ ] Bob never accepts reachability records unless they verify against Alice's identity.
- [ ] Newer signed reachability supersedes older reachability.
- [ ] `{identity}.jolt/path` can resolve to a `ContentId` from verified signed state.
- [ ] Resolver returns reachability hints separately from content identity.
- [ ] Tests cover valid reachability, stale replacement, invalid signature rejection, and missing path.
- [ ] Docs explain why `.jolt` is not DNS and why relays are carriers, not authorities.

## Notes

This is the important global addressing card.

Petnames should remain local UX on top of this:

```text
alice -> {identity}.jolt
```

Do not add payment, storage markets, relay ranking, or global usernames here.
