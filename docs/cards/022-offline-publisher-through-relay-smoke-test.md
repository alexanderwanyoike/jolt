# 022: Offline Publisher Through Relay Smoke Test

**Type:** AFK
**Milestone:** M5 / Relay Availability
**Status:** Done
**Blocked by:** None

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

- [x] Test starts Alice, relay, and Bob from clean state.
- [x] Alice creates a minimal space/community record.
- [x] Alice publishes signed content into that space.
- [x] Alice pins content and signed update-log state to the relay.
- [x] Alice stops.
- [x] Bob starts with only bootstrap relay configuration.
- [x] Bob fetches Alice's content by `.jolt` address.
- [x] Bob verifies Alice's signed state before accepting the result.
- [x] Bob fetches content from the relay while Alice is offline.
- [x] Dashboard/manual demo docs show the same flow.

## Manual Demo Flow

1. Start a relay-capable node and note its `/ip4/.../p2p/...` bootstrap address.
2. Start Alice with that bootstrap address.
3. Start Bob with that bootstrap address.
4. Publish from Alice with a path such as `/space/post`; the publish response returns `alice_identity.jolt/space/post`.
5. Send Alice's owner-signed pin request to `POST /api/v1/relay/pins` on the relay. The relay verifies the signature, fetches and pins the content, fetches Alice's signed update log, and announces itself as provider for both.
6. Stop Alice.
7. Fetch from Bob with the `.jolt` address. Bob should resolve Alice's signed update log through the relay and fetch the content from the relay, without knowing Alice's peer ID, CID, direct address, or cached state.

The automated smoke test is `test_offline_publisher_content_is_resolved_and_fetched_through_relay` in `crates/dweb-server/tests/api_integration.rs`.

## Non-Goals

- Payments.
- Relay marketplace.
- Automatic relay-to-relay content replication.
- Production bootstrap governance.
