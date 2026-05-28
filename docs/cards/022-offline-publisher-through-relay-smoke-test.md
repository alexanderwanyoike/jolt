# 022: Offline Publisher Through Relay Smoke Test

**Type:** AFK
**Milestone:** M5 / Relay Availability
**Status:** Blocked by 009
**Blocked by:** 009

## Why

This is the first proof that Jolt behaves like a decentralized web rather than a local peer-to-peer file demo.

The test should prove:

```text
Alice creates a space.
Alice publishes signed content into the space.
Alice delegates availability to a relay.
Alice goes offline.
Bob starts fresh.
Bob resolves Alice's .jolt address through bootstrap/discovery.
Bob fetches authorized content from the relay.
```

If this does not work, the relay/addressing story is not real yet.

## What to Build

Add an end-to-end smoke test and a documented manual demo.

The deterministic test can use local processes or in-process nodes, but it must model the real roles:

- Alice node.
- Relay/bootstrap node.
- Bob node with no prior Alice state.

Bob should start with only bootstrap relay configuration, not Alice's peer ID, raw CID, update log, or direct multiaddr.

## Acceptance Criteria

- [ ] Test starts Alice, relay, and Bob from clean state.
- [ ] Alice creates a minimal space/community record.
- [ ] Alice publishes signed content into that space.
- [ ] Alice pins content and signed update-log state to the relay.
- [ ] Alice stops.
- [ ] Bob starts with only bootstrap relay configuration.
- [ ] Bob fetches Alice's content by `.jolt` address.
- [ ] Bob verifies Alice's signed state before accepting the result.
- [ ] Bob fetches content from the relay while Alice is offline.
- [ ] Dashboard/manual demo docs show the same flow.

## Non-Goals

- Payments.
- Relay marketplace.
- Automatic relay-to-relay content replication.
- Production bootstrap governance.
