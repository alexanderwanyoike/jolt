# 005: Home Relay Configuration

**Type:** AFK  
**Milestone:** M4.5  
**Status:** Blocked  
**Blocked by:** 005

## Why

Users should not have to think about relays for normal publishing. Their node should know which relay acts as their delegated online presence.

## What to Build

Add home relay configuration to the node.

The node should be able to store and read:

- Home relay peer ID.
- Home relay multiaddr.
- Whether the relay is discovery-only or pin-capable, if known.

For v0, this can be manual configuration. Automatic relay discovery can come later.

## Acceptance Criteria

- [ ] CLI/config supports setting a home relay multiaddr.
- [ ] Node startup loads configured home relay.
- [ ] Status output/API shows configured home relay.
- [ ] Invalid relay multiaddr is rejected with a clear error.
- [ ] Docs show how to configure a home relay.

## Notes

Keep this simple. No relay marketplace, no payment, no automatic selection.
