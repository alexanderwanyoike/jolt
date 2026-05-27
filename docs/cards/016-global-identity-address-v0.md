# 016: Global Identity Address v0

**Type:** AFK
**Milestone:** Human addressing / M4.5
**Status:** Ready
**Blocked by:** 005

## Why

Jolt needs a canonical way to address a person globally before local petnames can be useful.

Raw CIDs identify immutable content. Peer IDs identify reachable network nodes. Neither is the right human-facing address for Alice's long-lived web presence.

The v0 canonical address should be identity-based:

```text
{identity}.jolt
{identity}.jolt/profile
{identity}.jolt/feed
{identity}.jolt/posts/hello
```

Where `{identity}` is derived from Alice's long-lived public identity key.

For now, the same key is also used to derive the libp2p `PeerId`. That is acceptable for v0, but the address should still be modeled as an identity address so future work can separate "Alice" from "Alice's current device or node."

## What to Build

Add a canonical identity address type and parser.

The first version should support:

- Derive a stable identity ID from a public identity key.
- Format a canonical identity host as `{identity}.jolt`.
- Parse `{identity}.jolt`.
- Parse `{identity}.jolt/path`.
- Normalize paths consistently.
- Reject malformed identity addresses with clear errors.
- Expose the node's own canonical Jolt address in CLI status, HTTP status, and the dashboard.
- Keep raw peer IDs available for debug/network connection workflows.

This card does not need to resolve content from the network yet. It creates the address shape that later resolver, petname, profile/feed, and relay cards can use.

## Acceptance Criteria

- [ ] Identity ID derivation is deterministic for a public identity key.
- [ ] `{identity}.jolt` round-trips through parse and display.
- [ ] `{identity}.jolt/profile` parses into identity plus path.
- [ ] Empty or missing path normalizes to `/`.
- [ ] Invalid domains, invalid identity strings, and malformed paths fail with clear errors.
- [ ] Node status output shows the canonical identity address.
- [ ] HTTP status API returns the canonical identity address.
- [ ] Dashboard identity panel shows the canonical identity address.
- [ ] Tests cover valid, invalid, and path-normalization cases.
- [ ] Docs explain that identity addresses are canonical, while peer IDs are transport/debug identifiers.

## Notes

Do not add global usernames in this card.

Do not add petnames in this card. Petnames should build on top of this model:

```text
alice -> {identity}.jolt
```

Do not add distributed resolution, relay lookup, or DNS-like governance here. Those belong in later resolver and relay cards.
