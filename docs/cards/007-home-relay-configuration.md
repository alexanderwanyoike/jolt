# 007: Home Relay Configuration

**Type:** AFK
**Milestone:** M5 / Relay Availability
**Status:** Done
**Blocked by:** None

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

- [x] CLI/config supports setting a home relay multiaddr.
- [x] Node startup loads configured home relay.
- [x] Status output/API shows configured home relay.
- [x] Invalid relay multiaddr is rejected with a clear error.
- [x] Docs show how to configure a home relay.

## Notes

Keep this simple. No relay marketplace, no payment, no automatic selection.

Implemented as:

```bash
dweb home-relay set /ip4/<RELAY_IP>/tcp/<PORT>/p2p/<RELAY_PEER_ID> --capability pinning
dweb home-relay show
dweb home-relay clear
```

The configured home relay is persisted in node settings, loaded on daemon startup, returned by `/api/v1/status`, and shown in `dweb status` and the dashboard relay panel.
