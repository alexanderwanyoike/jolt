# 088: Bootstrap Relay Deployment v0

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** In Progress
**Blocked by:** None

## Why

The local demos prove the protocol, but a public-ish Jolt demo needs a simple
bootstrap relay that others can configure. Running a relay should not require
understanding the source tree.

The same `jolt` binary should run both local user daemons and server-facing
relay mode.

## What to Build

- Add a documented headless relay install path using the released `jolt` binary.
- Add a relay setup command or script that creates a minimal config for:
  - bootstrap/discovery relay mode;
  - API bind/port;
  - P2P listen port;
  - data directory;
  - optional systemd unit.
- Add server setup docs for a cheap Linux VPS such as Hetzner.
- Add firewall guidance for the required ports.
- Add operator commands for:
  - start;
  - stop/restart if using systemd;
  - status;
  - logs;
  - printing the relay multiaddr users should configure.
- Keep Console optional for relay servers.

## Acceptance Criteria

- [x] A fresh Linux VPS can install `jolt` from a release without building.
- [ ] A documented command initializes relay config.
- [ ] A documented command starts the relay.
- [ ] `jolt relay status` reports useful operator state.
- [x] Docs show the bootstrap multiaddr users should add to Console/settings.
- [ ] Setup works without a GUI.
- [x] Manual smoke: local user daemon can add the VPS relay as bootstrap and
      report it in status.

## Non-Goals

- Kubernetes/Helm.
- Prometheus metrics.
- Advanced relay reputation.
- Payments or storage markets.
- Running Jolt Console on the VPS.

## Notes

Systemd is acceptable for the server relay path. The earlier "no OS service"
constraint was for desktop Console v0, not for a headless relay host.

## Progress Notes

- 2026-06-10: Deployed a first cost-optimized Hetzner bootstrap relay using the
  released `jolt-linux-x86_64` CLI asset. It runs as `jolt-bootstrap.service`
  under a dedicated `jolt` user with state in `/var/lib/jolt-bootstrap`, local
  API bind on `127.0.0.1:9862`, P2P on UDP `4001`, UFW allowing only SSH and
  `4001/udp`, and journald capped at 200M/14 days.
- 2026-06-10: Manual smoke verified a fresh local daemon can bootstrap over
  direct QUIC to:
  `/ip4/167.233.106.111/udp/4001/quic-v1/p2p/12D3KooWDmwLRmG4pZa7GcUM1P3CXM9TwMjtoM69QqTrwXD63tqi`.
- 2026-06-10: Added the Hetzner relay as the built-in default bootstrap peer so
  fresh installs can join the demo network without manual relay configuration.
  It is discovery/rendezvous only; public pinning remains blocked on relay
  policy/allowlisting.
