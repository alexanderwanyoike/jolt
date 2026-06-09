# 088: Bootstrap Relay Deployment v0

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Ready after 084
**Blocked by:** 084

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

- [ ] A fresh Linux VPS can install `jolt` from a release without building.
- [ ] A documented command initializes relay config.
- [ ] A documented command starts the relay.
- [ ] `jolt relay status` reports useful operator state.
- [ ] Docs show the bootstrap multiaddr users should add to Console/settings.
- [ ] Setup works without a GUI.
- [ ] Manual smoke: local user daemon can add the VPS relay as bootstrap and
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
