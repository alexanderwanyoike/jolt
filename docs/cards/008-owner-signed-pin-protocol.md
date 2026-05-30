# 008: Owner-Signed Pin Protocol

**Type:** AFK  
**Milestone:** M4.5  
**Status:** Done

**Blocked by:** None

## Why

Relays should only intentionally pin content when the owner asks them to. That keeps authority with the owner's key.

## What to Build

Define and implement an owner-signed pin request.

The request should state:

- Owner identity.
- Content CID to pin.
- Optional signed record/update-log CID associated with the content.
- Signature proving owner intent.

The relay should verify the request before accepting it.

## Acceptance Criteria

- [x] Pin request type exists and serializes over the chosen wire format.
- [x] Owner can sign a pin request.
- [x] Relay-side verification accepts valid owner signatures.
- [x] Relay-side verification rejects invalid signatures or mismatched owners.
- [x] Tests cover valid request, wrong signer, malformed content ID, and tampered request.

## Notes

Do not add relay-to-relay replication. This card is only owner -> selected relay.

Implemented as `PinRequest` in `jolt-core`, signed over canonical bytes with the owner's identity key.
