# 018: Global Update-Log Discovery v0

**Type:** AFK
**Milestone:** M5
**Status:** Done
**Blocked by:** 017

## Why

Card 017 gives Jolt the trusted resolution rule:

```text
{identity}.jolt/path + verified signed update log
  -> ContentId + reachability hints
```

That is not enough for global use. Bob should not need Alice to paste him an update log. Given only:

```text
{identity}.jolt/profile
```

Bob's node needs a network path for finding candidate copies of Alice's signed update log, verifying them, choosing the newest valid state, and then resolving the requested path.

This is the piece that turns `.jolt` from a cryptographic address format into something that can be resolved across the mesh.

## Core Rule

Discovery is not authority.

Relays, DHT provider records, cached peers, and direct peers can only say:

```text
I might have Alice's update log.
```

Only Alice's identity signature can say:

```text
This is Alice's current signed state.
```

## What to Build

Implement the smallest network-backed resolver path:

```text
JoltAddress
  -> discover update-log candidates for address.identity
  -> request update-log entries from candidates
  -> verify candidate logs against address.identity
  -> keep the newest valid contiguous log
  -> resolve path with the Card 017 pure resolver
  -> return ContentId + reachability hints
```

The implementation should prefer a narrow vertical slice over a broad discovery framework.

## Proposed v0 Shape

Add an update-log discovery protocol message:

```text
UpdateLogRequest {
  identity: IdentityId,
  since: Option<u64>,
}

UpdateLogResponse {
  entries: Vec<UpdateLogEntry>,
}
```

Nodes that can serve an identity's update log advertise or answer under a deterministic discovery key:

```text
jolt:update-log:{identity}
```

Bob's resolver flow:

1. Parse `{identity}.jolt/path`.
2. Check local verified cache for that identity.
3. Discover candidate peers/relays for `jolt:update-log:{identity}`.
4. Request update-log entries from candidates.
5. Verify signatures, owner identity, sequence ordering, and previous-entry hashes.
6. Ignore invalid logs.
7. Ignore stale logs when Bob already has a newer valid sequence.
8. Resolve the path using the verified state from Card 017.

## Storage and Caching

The node should cache verified update logs by identity.

Rules:

- Store only logs that verify against the identity.
- Replace local state only with a newer valid contiguous sequence.
- Keep the local cache as an optimization, not as an authority.
- Make stale or malicious network responses harmless.

## Non-Goals

Do not add these in this card:

- Human names or petnames.
- Global usernames.
- Payment or storage markets.
- Relay ranking.
- Relay-to-relay replication.
- Profile/feed product features.
- Content encryption.

## Testing Strategy

This card needs deterministic tests first.

Prefer pure/service-level tests for:

- Selecting the newest valid log from several candidates.
- Rejecting invalid signatures.
- Rejecting identity mismatches.
- Ignoring stale lower-sequence responses.
- Resolving the requested `JoltAddress` after discovery.

Add a local multi-node test only after the pure selection logic is covered.

## Acceptance Criteria

- [x] A node can request update-log entries for a specific `IdentityId`.
- [x] A node can serve a local update log for an identity it knows about.
- [x] The resolver rejects candidate logs that do not verify against the requested identity.
- [x] The resolver chooses the newest valid contiguous log from multiple candidates.
- [x] A stale lower-sequence response does not replace a newer local verified log.
- [x] `{identity}.jolt/path` can be resolved through the network-backed discovery path in a deterministic local test.
- [x] The resolved `ContentId` is still produced by the signed-state resolver, not by the relay or DHT response itself.
- [x] Docs explain that DHT/relays locate candidate logs but do not define `.jolt` truth.

## Notes

This should happen before local petnames. Petnames improve UX, but this card is what makes canonical identity addresses usable beyond a local or manually supplied record.
