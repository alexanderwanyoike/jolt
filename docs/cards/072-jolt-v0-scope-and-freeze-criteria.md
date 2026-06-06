# 072: Jolt v0 Scope and Freeze Criteria

**Type:** HITL  
**Milestone:** v0 Endgame  
**Status:** Ready for discussion
**Blocked by:** 071

## Why

Jolt has enough infrastructure that the risk is now scope drift, not missing
technical ideas. v0 needs a hard boundary so the project can be used, judged,
and either continued or paused without becoming an endless platform build.

## v0 Product Shape

Jolt v0 is:

```text
Jolt Console + daemon + CLI
```

Jolt is the local runtime/control plane:

- local identity and keys;
- scoped app sessions and permission approval;
- publishing and fetching identity-owned signed paths;
- private encrypted content;
- optional relay-backed availability;
- recipient-controlled two-way communication;
- basic diagnostics.

Apps are separate products that integrate with Jolt:

- Pastey;
- Spoke.

Console stays focused on control and trust decisions. It is not an app store,
app launcher, app catalog, or marketing surface.

## What to Decide

- Which v0 features are mandatory before code freeze.
- Which features are explicitly postponed until after the Spoke/Pastey proof.
- What counts as a successful v0 human demo.
- What counts as a failed v0 product proof.

## Acceptance Criteria

- [ ] v0 scope is documented in one concise checklist.
- [ ] v0 non-goals are documented.
- [ ] Freeze criteria are documented.
- [ ] Success/failure criteria are documented.
- [ ] The plan names the exact implementation cards required before freeze.

## Non-Goals

- Console app store or Apps page.
- WASM runtime.
- Storage markets, payments, or storage-market mechanics.
- Drops.
- Protocol-level inbox, message, contact, profile, feed, or app semantics.
- Relay structured logs and metrics before product proof.

## Notes

This card is the guardrail for the final push. If a proposed feature does not
serve v0 use of Pastey or Spoke, it should wait.
