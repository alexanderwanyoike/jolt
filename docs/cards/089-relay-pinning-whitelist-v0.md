# 089: Relay Pinning Whitelist v0

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Ready after 088
**Blocked by:** 076, 088

## Why

Relays should be able to participate in public discovery without accepting
unbounded storage from everyone. For v0, a simple allowlist of `.jolt`
identities that may pin to a relay is easier to understand and operate than a
general permissions system.

Discovery can remain open. Pinning should be relay-policy controlled.

## What to Build

Add relay-side pinning policy:

```json
{
  "pinning": {
    "enabled": true,
    "allow_public": false,
    "allowed_identities": [
      "exampleidentity.jolt"
    ]
  },
  "discovery": {
    "enabled": true,
    "allow_public": true
  }
}
```

The exact config shape can follow existing Jolt config conventions, but the
behavior should be:

- discovery/provider routing remains allowed for all identities by default;
- pinning is disabled or private by default unless explicitly configured;
- pin requests verify the owner identity;
- relay accepts pins only when the owner identity is allowlisted or public
  pinning is explicitly enabled;
- rejection errors are structured and explain relay policy.

## Acceptance Criteria

- [ ] Relay config can list identities allowed to pin.
- [ ] Unauthorized pin requests are rejected.
- [ ] Allowlisted identity pin requests succeed.
- [ ] Public pinning requires explicit config.
- [ ] Discovery continues to work for non-allowlisted identities.
- [ ] `jolt relay status` reports pinning policy mode without leaking secrets.
- [ ] Tests cover unauthorized, allowlisted, public, and discovery-open cases.
- [ ] Docs show how to add/remove an allowed identity.

## Non-Goals

- Per-app pinning permissions.
- Quotas beyond any existing relay bounds.
- Payment or storage markets.
- Complex groups/roles.

## Notes

This is the concrete v0 implementation slice for the broader direction in card
076.
