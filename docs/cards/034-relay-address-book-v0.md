# 034: Relay Address Book v0

**Type:** AFK  
**Milestone:** M5+  
**Status:** Done
**Blocked by:** 033

## Why

Nodes can currently know an arbitrary amount of relays, but relay knowledge is still mostly manual or cached as incidental peer hints.

Jolt needs an explicit relay address book so nodes and relays can remember verified relay records, expire stale entries, and prefer relays that have actually worked.

## What to Build

Persist verified relay records locally with operational metadata:

```text
StoredRelayRecord {
  relay_record
  first_seen
  last_seen
  last_success
  failure_count
}
```

Rules:

- Deduplicate by relay identity.
- Reject invalid or expired records.
- Bound total stored relay records.
- Track failures without immediately deleting records.
- Prefer configured relays first, then recently successful learned relays.

## Acceptance Criteria

- [x] Nodes can store verified relay records.
- [x] Relays can store verified relay records.
- [x] Expired records are ignored or removed.
- [x] Duplicate records update the existing entry when newer.
- [x] Stored relay records are bounded.
- [x] Status/API can report known relay count.

## Verification

Required:

- Automated tests for insert/update/expiry/deduplication/bounds.

Optional manual check:

- Start a node with a seeded relay record.
- Run status or inspect dashboard/API and see known relay count.

No Hetzner canary is required.

## Non-Goals

- Exchanging relay records with peers.
- Relay mesh exploration.
- Query forwarding.
