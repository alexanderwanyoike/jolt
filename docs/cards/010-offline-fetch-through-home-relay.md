# 008: Offline Fetch Through Home Relay

**Type:** AFK  
**Milestone:** M4.5  
**Status:** Blocked  
**Blocked by:** 009

## Why

This is the core Jolt story after the current P2P proof:

> Alice publishes content. Alice goes offline. Bob can still resolve Alice's latest signed state and fetch the content from Alice's home relay.

## What to Build

Create an end-to-end test and user-facing flow for offline publisher availability through a home relay.

The flow should include:

- Alice publishes content and update-log state.
- Alice pins content and signed state to home relay.
- Alice's node goes offline.
- Bob resolves Alice's latest state.
- Bob fetches the content from the relay.
- Bob verifies content hash and owner signature.

## Acceptance Criteria

- [ ] Test covers Alice online publish -> relay pin -> Alice offline -> Bob fetch.
- [ ] Bob does not require Alice's node to be online.
- [ ] Bob verifies the signed state belongs to Alice.
- [ ] Bob verifies fetched content matches the CID.
- [ ] Docs include the successful flow as the first relay demo.

## Notes

This should become the new project demo. It is more important than WASM runtime work.
