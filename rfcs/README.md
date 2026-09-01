# Jolt RFCs

RFCs specify decisions that change Jolt's protocol or its application-facing
compatibility surface. They are implementable protocol documents, not project
essays: each RFC defines its scope, terms, wire or canonical encoding, required
behavior, errors, compatibility impact, and security/privacy considerations.

## Lifecycle

- **Draft:** open for review and still expected to change.
- **Accepted:** the direction is agreed; implementation may still be pending.
- **Implemented:** the accepted behavior exists and has verification coverage.
- **Superseded:** a newer RFC replaces the decision and links back to it.
- **Rejected:** considered but deliberately not adopted.

Acceptance is a maintainer decision recorded in the RFC and its review PR. An
accepted RFC is not evidence that implementation has landed. Cards and issues
track implementation; RFCs record the durable decision.

## When an RFC is needed

Use an RFC for compatibility-affecting wire or schema changes, identity and
device authority, signed-state semantics, encryption/access-grant semantics,
relay trust or availability policy, community-level protocol records, and the
daemon/app authorization boundary.

Small bug fixes, internal refactors, documentation corrections, and app-owned
product behavior normally do not need an RFC.

## Process

1. Copy `0000-template.md` and choose the next number.
2. Open a PR with status `Draft`; link the motivating docs and work cards.
3. Review the model, security properties, compatibility impact, alternatives,
   and unresolved questions.
4. Record the decision in the RFC. Open implementation cards separately.
5. Mark it `Implemented` only after the behavior and tests have landed.
6. Amend small clarifications in place; use a new RFC for a semantic change and
   mark the old document `Superseded`.

## Index

| RFC | Title | Status | Implementation |
|---|---|---|---|
| [0001](0001-core-protocol.md) | Jolt Signed Path Records and Resolution | Experimental Draft | Implemented with known gaps |
| [0002](0002-device-authority.md) | Device Authorization and Revocation | Experimental Draft | Implemented v1 |
| [0003](0003-device-writer-logs.md) | Per-Device Writer Logs and Deterministic Merge | Experimental Draft | Implemented v1 |
| [0004](0004-encrypted-device-access.md) | Encrypted Objects and Private Device Access | Experimental Draft | Envelope and daemon operations implemented; device-key custody experimental |
| [0005](0005-community-membership.md) | Community Identities and Membership | Experimental Draft | Design only |
| [0006](0006-community-app-indexes.md) | Community-Scoped App Indexes and Local Discovery | Experimental Draft | Design only |
| [0007](0007-app-sessions.md) | Capability-Scoped Local App Sessions | Experimental Draft | Implemented v0 |
| [0008](0008-device-writer-extensions.md) | Device Writer Tombstones, Causal Heads, and Delta Sync | Experimental Draft | Implemented operation levels 2/3 and sync level 2 |
