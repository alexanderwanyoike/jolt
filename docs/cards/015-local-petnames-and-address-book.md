# 015: Local Petnames and Address Book

**Type:** AFK
**Milestone:** Human addressing / M4.5
**Status:** Ready after 018
**Blocked by:** 018

## Why

CIDs and identity addresses are correct machine identifiers, but raw identity IDs are not humane addresses.

Users should not need to remember or paste values like:

```text
bafkr4i...
{identity}.jolt
```

The next web needs human-scale references. The first honest version is local petnames: names that only mean something on the user's own node.

Example:

```text
alice -> {identity}.jolt
bob-work -> {identity}.jolt
```

Then a user can resolve and navigate things like:

```text
alice/profile
alice/feed
alice/posts/2026-05-27
```

This avoids pretending Jolt has global usernames, DNS, or identity governance before those problems are designed.

## What to Build

Add a local address book for identity addresses.

The first version should support:

- Store a local alias for an identity address.
- List saved aliases.
- Remove or update an alias.
- Resolve an alias to an identity address anywhere the local resolver accepts an identity address.
- Show aliases in the dashboard peer list when known.
- Prefer aliases in profile/feed UI once that exists.

The address book is local-only:

- No global uniqueness.
- No username registration.
- No claims that `alice` means the same person on two different machines.
- No trust graph or social proof yet.

## Acceptance Criteria

- [ ] Local storage persists aliases across daemon restarts.
- [ ] CLI can add, list, update, and remove aliases.
- [ ] HTTP API exposes address-book operations for the dashboard.
- [ ] Dashboard can name a known identity.
- [ ] Dashboard peer list displays `alias` plus shortened identity address when an alias exists.
- [ ] Resolver accepts a canonical identity address and a known alias.
- [ ] Unknown aliases fail with a clear error.
- [ ] Docs explain petnames are local labels, not global usernames.

## Notes

This card should land after canonical identity addresses and global update-log discovery, because petnames are local shortcuts for identity addresses.

Keep the model deliberately simple:

```text
alias -> {identity}.jolt
```

Later work may add:

- imported/exported address books
- QR invite links
- signed profile display names
- social proof
- global or community naming

Do not add those in this card.
