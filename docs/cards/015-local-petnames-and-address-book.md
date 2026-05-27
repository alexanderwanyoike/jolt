# 015: Local Petnames and Address Book

**Type:** AFK
**Milestone:** Human addressing / M4.5
**Status:** Ready after 005
**Blocked by:** 005

## Why

CIDs and peer IDs are correct machine identifiers, but they are not humane addresses.

Users should not need to remember or paste values like:

```text
12D3KooW...
bafkr4i...
```

The next web needs human-scale references. The first honest version is local petnames: names that only mean something on the user's own node.

Example:

```text
alice -> 12D3KooW...
bob-work -> 12D3KooW...
```

Then a user can resolve and navigate things like:

```text
alice/profile
alice/feed
alice/posts/2026-05-27
```

This avoids pretending Jolt has global usernames, DNS, or identity governance before those problems are designed.

## What to Build

Add a local address book for peer identities.

The first version should support:

- Store a local alias for a peer ID.
- List saved aliases.
- Remove or update an alias.
- Resolve an alias to a peer ID anywhere the local resolver accepts an identity.
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
- [ ] Dashboard can name a connected peer.
- [ ] Dashboard peer list displays `alias` plus shortened peer ID when an alias exists.
- [ ] Resolver accepts a raw peer ID and a known alias.
- [ ] Unknown aliases fail with a clear error.
- [ ] Docs explain petnames are local labels, not global usernames.

## Notes

This card should land after latest-record resolution exists, because aliases become useful when they can be used to resolve a person's signed state.

Keep the model deliberately simple:

```text
alias -> peer_id
```

Later work may add:

- imported/exported address books
- QR invite links
- signed profile display names
- social proof
- global or community naming

Do not add those in this card.
