# 028: Three-Node Canary Harness

**Type:** AFK
**Milestone:** M5
**Status:** Blocked by 024
**Blocked by:** 024

## Why

The project needs a repeatable way to prove the global discovery shape:

```text
Alice -> Relay -> Bob
```

Local deterministic tests should catch regressions in CI. A real-world canary should catch NAT, relay, and routing problems that local tests cannot model.

## What to Build

Add a documented harness for:

1. Deterministic local three-node test.
2. Manual real-world canary.

The local test should prove:

```text
Alice announces update-log provider.
Relay acts as bootstrap/discovery relay.
Bob starts with only relay config.
Bob discovers Alice through relay/DHT.
Bob requests and verifies Alice's update log.
```

The real-world canary should use:

- one public bootstrap/relay node
- Alice on one network
- Bob on another network
- at least one NAT/CGNAT environment when possible

## Acceptance Criteria

- [ ] Local deterministic three-node test is documented and runnable.
- [ ] Test starts Bob without Alice's peer ID, raw CID, direct multiaddr, or update log.
- [ ] Test proves Bob discovers Alice through configured relay/DHT path.
- [ ] Test verifies Alice's update log before success.
- [ ] Manual real-world canary steps are documented.
- [ ] Canary includes expected commands, expected status output, and failure hints.

## Non-Goals

- Fully automated Hetzner provisioning.
- Long-running public testnet operations.
- Performance benchmarking.
