# 029: Home Relay Publish Pinning UX

**Type:** AFK
**Milestone:** M5 / Relay Availability
**Status:** Done
**Blocked by:** 007

## Why

The offline relay proof works in the Rust integration test, but a user cannot perform the same flow cleanly from the CLI or dashboard yet.

Today Alice can publish content and the relay can accept owner-signed pin requests, but the missing bridge is:

```text
Alice publishes content.
Alice's node creates the owner-signed relay pin request.
Alice's node sends it to her configured home relay.
The dashboard/API shows whether the content is now relay-backed.
```

Without this, the strongest Jolt demo remains a test-only workflow. Users should not need to understand `PinRequest` JSON or manually call a relay API.

## What to Build

Add a user-facing pin-to-home-relay flow after publishing content.

The node should use its local identity key to create the owner-signed pin request for content it published, then send that request to the configured home relay. The relay should continue to verify the request exactly as it does today.

This should work through:

- HTTP API.
- CLI.
- Dashboard.

The dashboard should make the relay-backed state visible enough for local demos:

- Content published locally.
- Home relay configured or missing.
- Pin request pending/succeeded/failed.
- Relay response, including pinned CID and latest signed update-log sequence.

## Acceptance Criteria

- [x] Node can create an owner-signed `PinRequest` for locally published content without exposing private key material.
- [x] HTTP API exposes a local "pin this published CID to my home relay" operation.
- [x] CLI can publish and pin to the configured home relay, or pin an existing published CID.
- [x] Dashboard exposes a simple pin-to-home-relay action for recently published content.
- [x] Dashboard shows clear failure states when no home relay is configured, the relay is unreachable, the relay is not pin-capable, or the pin request is rejected.
- [x] Successful pinning pins both the content and signed update-log state on the relay.
- [x] A local manual demo can perform: Alice publish -> Alice pin to relay -> Alice offline -> Bob fetch by `.jolt`.
- [x] Tests cover signed request creation, API behavior, and the publish/pin workflow without testing dashboard scaffolding.

## Notes

Keep the relay as a carrier, not an authority. Alice's key signs the pin request. The relay verifies and stores; it does not decide ownership.

The v0 implementation stores an optional home relay `api_url` because relay pinning currently uses the HTTP control plane. The p2p home-relay control protocol remains a later cleanup once the local product path is proven.

This card should not add relay marketplaces, payment, automatic relay selection, or relay-to-relay replication.
