# 035: Relay Record Exchange v0

**Type:** AFK  
**Milestone:** M5+  
**Status:** Done
**Blocked by:** 033, 034

## Why

Relays should let nodes know about other relays. Nodes should also be able to share relays they know with discovered relays.

This makes the network denser over time:

```text
Tim knows R2
R2 knows R1 and R3
Tim connects to R2
Tim learns R1 and R3
```

This is relay discovery, not identity/content discovery yet.

## What to Build

Add a bounded relay-record exchange:

```text
GetRelays {
  limit
  capabilities
}

Relays {
  records
}

AnnounceRelays {
  records
}
```

When a node connects to a relay:

1. Node may announce a bounded set of verified relay records it knows.
2. Relay verifies and stores useful records.
3. Node asks relay for known relays.
4. Relay returns a bounded set of verified records.
5. Node stores those records as candidate relays.

## Acceptance Criteria

- [x] A node can request relay records from a connected relay.
- [x] A node can announce verified relay records to a relay.
- [x] Invalid relay records are rejected.
- [x] Exchange is bounded by count and record validity.
- [x] Learned relays persist in the local relay address book.
- [x] Existing bootstrap behavior still works without relay exchange.

## One-Machine Process Demo

Required for review.

Run locally:

```text
R1 knows R2 and R3
Tim starts knowing only R1
Tim connects to R1
Tim learns R2 and R3
```

Verifier should be able to inspect:

- Tim's known relay count increases.
- Tim's known relays include R2 and R3.
- No Hetzner or second physical machine is required.

## Non-Goals

- Relay-to-relay exploration.
- Identity provider lookup.
- Content provider lookup.
