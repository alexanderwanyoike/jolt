# 023: Bootstrap Management UX

**Type:** AFK
**Milestone:** M5
**Status:** Done
**Blocked by:** 019

## Why

Persistent bootstrap config is useful only if a user or operator can inspect and change it without manually editing files.

The current `--bootstrap` flag is fine for one-off demos, but a real node needs persistent bootstrap relay management:

```text
jolt bootstrap list
jolt bootstrap add <multiaddr>
jolt bootstrap remove <multiaddr>
```

This is configuration UX, not relay discovery itself.

## What to Build

Add CLI commands for managing configured bootstrap relay multiaddrs.

The commands should operate on the same node config introduced in Card 019.

Suggested shape:

```text
jolt bootstrap list
jolt bootstrap add /dns4/bootstrap.example/udp/4001/quic-v1/p2p/12D3...
jolt bootstrap remove /dns4/bootstrap.example/udp/4001/quic-v1/p2p/12D3...
```

The command output should distinguish:

- configured relays
- built-in defaults, if any
- effective relays used at startup

## Acceptance Criteria

- [x] CLI can list configured bootstrap relays.
- [x] CLI can add a bootstrap relay multiaddr.
- [x] CLI rejects malformed bootstrap relay multiaddrs with a clear error.
- [x] CLI does not add duplicate relay addresses.
- [x] CLI can remove a configured bootstrap relay multiaddr.
- [x] CLI list output distinguishes configured relays from built-in defaults.
- [x] Tests cover add, list, remove, duplicates, and malformed addresses.

## Non-Goals

- Dialing or proving bootstrap connectivity.
- Dashboard management UI.
- Relay gossip.
- Public bootstrap governance.
