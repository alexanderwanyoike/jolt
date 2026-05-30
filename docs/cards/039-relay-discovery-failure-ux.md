# 039: Relay Discovery Failure UX

**Type:** AFK  
**Milestone:** M5+  
**Status:** Ready  
**Blocked by:** 037, 038

## Why

When `.jolt` resolution fails, users need to know what failed:

```text
No relays configured?
Relays unreachable?
Relay mesh reachable but identity unknown?
Provider found but signature invalid?
Content provider unavailable?
```

Without this, the network feels broken even when the failure is expected.

## What to Build

Expose structured failure reasons through CLI, API, and dashboard for `.jolt` resolution and fetch:

```text
no_bootstrap_relays
relay_unreachable
relay_mesh_empty
identity_provider_not_found
identity_head_invalid
content_provider_not_found
content_fetch_failed
content_hash_mismatch
```

The wording should make the difference clear:

```text
alice.jolt is globally meaningful.
It is only reachable if this node can reach a relay mesh that knows where to find Alice.
```

## Acceptance Criteria

- [ ] CLI errors distinguish bootstrap, mesh, identity, verification, and content failures.
- [ ] API returns structured error codes.
- [ ] Dashboard renders useful failure text.
- [ ] Tests cover the main failure modes.
- [ ] Existing successful resolution/fetch behavior is unchanged.

## One-Machine Process Demo

Required for review.

Run local failure scenarios:

```text
Tim with no relays
Tim with unreachable relay
Tim with R2 reachable but Alice unknown
Tim with invalid identity-head hint
Tim with valid identity but unavailable content
```

Verifier should be able to see distinct errors for each case.

No Hetzner canary is required.

## Non-Goals

- Implementing relay gossip itself.
- Automatic repair.
- Relay scoring.
