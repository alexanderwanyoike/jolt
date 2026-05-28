# 023: Bootstrap Management UX

**Type:** AFK
**Milestone:** M5
**Status:** Blocked by 019
**Blocked by:** 019

## Why

Persistent bootstrap config is useful only if a user or operator can inspect and change it without manually editing files.

The current `--bootstrap` flag is fine for one-off demos, but a real node needs persistent bootstrap relay management:

```text
dweb bootstrap list
dweb bootstrap add <multiaddr>
dweb bootstrap remove <multiaddr>
```

This is configuration UX, not relay discovery itself.

## What to Build

Add CLI commands for managing configured bootstrap relay multiaddrs.

The commands should operate on the same node config introduced in Card 019.

Suggested shape:

```text
dweb bootstrap list
dweb bootstrap add /dns4/bootstrap.example/udp/4001/quic-v1/p2p/12D3...
dweb bootstrap remove /dns4/bootstrap.example/udp/4001/quic-v1/p2p/12D3...
```

The command output should distinguish:

- configured relays
- built-in defaults, if any
- effective relays used at startup

## Acceptance Criteria

- [ ] CLI can list configured bootstrap relays.
- [ ] CLI can add a bootstrap relay multiaddr.
- [ ] CLI rejects malformed bootstrap relay multiaddrs with a clear error.
- [ ] CLI does not add duplicate relay addresses.
- [ ] CLI can remove a configured bootstrap relay multiaddr.
- [ ] CLI list output distinguishes configured relays from built-in defaults.
- [ ] Tests cover add, list, remove, duplicates, and malformed addresses.

## Non-Goals

- Dialing or proving bootstrap connectivity.
- Dashboard management UI.
- Relay gossip.
- Public bootstrap governance.
