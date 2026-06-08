# 072: Jolt v0 Scope and Freeze Criteria

**Type:** HITL  
**Milestone:** v0 Endgame  
**Status:** Decided in PR
**Blocked by:** 071

## Why

Jolt has enough infrastructure that the risk is now scope drift, not missing
technical ideas. v0 needs a hard boundary so the project can be used, judged,
and either continued or paused without becoming an endless platform build.

This card locks the v0 contract.

## v0 Bet

Jolt v0 is not "the decentralized web."

Jolt v0 is:

> A local runtime/control plane that lets separate apps use user-owned identity,
> scoped authority, private sharing, optional availability, and
> recipient-controlled communication.

The proof apps are separate from Jolt:

- Pastey: technical app-boundary/private-sharing proof.
- Spoke: human-facing social proof.

Jolt itself ships as:

```text
Jolt Console + daemon + CLI
```

Console stays focused on local control and trust decisions. It is not an app
store, app launcher, app catalog, social app, or marketing surface.

## Required v0 Scope

Jolt v0 must include:

- local identity and signing keys;
- app session request/approval/revocation;
- capability-checked app APIs;
- publishing and fetching identity-owned signed paths;
- public and private encrypted content;
- local daemon lifecycle through Console;
- network settings through Console;
- basic relay diagnostics already implemented;
- signed reachability sufficient for contact attempts;
- recipient-controlled two-way ingress;
- optional pinning that is not required for basic use;
- relay-side authorization for pinning;
- installable/runnable Console + daemon + CLI;
- docs for using Pastey and Spoke as separate apps.

## Required v0 Cards

Critical path:

1. [073](073-two-way-communication-design.md): design recipient-controlled
   two-way communication.
2. [074](074-reachability-and-rendezvous-clarification.md): clarify
   reachability/rendezvous only as much as v0 needs.
3. [075](075-recipient-ingress-v0.md): implement generic recipient ingress.
4. [078](078-spoke-social-poc.md): build Spoke as the human-facing PoC.
5. [080](080-v0-freeze-and-bugfix-window.md): freeze new features and fix only
   blocking bugs.
6. [081](081-launch-and-postmortem.md): publish, gather feedback, and decide
   continue/pause/bin.

Supporting but required before freeze:

- [076](076-optional-and-authorized-relay-pinning.md): optional and
  relay-authorized pinning.
- [077](077-jolt-distribution-v0.md): installable/runnable Jolt package.
- [079](079-pastey-final-compatibility-pass.md): Pastey compatibility check.

## Explicit Non-Goals

Do not build before v0 freeze:

- Console Apps page.
- App store.
- Decentralized app installation.
- WASM app runtime.
- Storage markets, payments, or storage-market mechanics.
- Drops.
- Global search.
- Global usernames.
- Full contacts/social graph system in Jolt protocol.
- Protocol-level inbox, message, contact, profile, feed, thread, or app
  semantics.
- OS service/autostart.
- System tray/menu bar presence.
- Relay structured logs.
- Relay metrics.
- Multi-identity wallet UX.
- Console identity import/export implementation unless it becomes a blocking
  setup/demo issue.

These may be valid later. They are not v0.

## Freeze Rule

After card 080 starts:

- no new protocol features;
- no new app capabilities except bug fixes;
- no new Console surfaces except bug fixes/setup fixes;
- no new relay/operator features;
- no new product surfaces;
- bug fixes, setup docs, demo docs, packaging fixes, and blocking UX fixes only.

Any proposed feature after freeze must answer:

> Does the v0 Pastey/Spoke demo fail without this?

If the answer is no, it waits.

## v0 Human Demo

The v0 demo must show:

- install/start Jolt;
- create/use one local identity;
- approve a scoped app session;
- use Pastey for public and private content;
- use Spoke with at least two local identities/nodes;
- publish a Spoke post;
- read another identity's Spoke post;
- send a reply/mention through recipient-controlled ingress;
- accept/reject incoming social objects;
- run without pinning;
- optionally pin when an authorized relay accepts it;
- explain failed pinning clearly when relay policy rejects it.

## Success Signals

Jolt has legs if, after v0:

- a user can explain what Jolt does without hearing a long protocol lecture;
- Spoke or Pastey feels like a real thing someone might try;
- the difference from "just run a server over Tailscale" is understandable;
- a developer can imagine building an app against Jolt's local authority model;
- feedback points toward a sharper v1 instead of more abstract infrastructure.

## Failure Signals

Jolt should be paused or binned if, after v0:

- users still do not understand why Jolt exists;
- the setup burden overwhelms the product value;
- two-way communication still feels too abstract to use;
- `.jolt` identities remain unusable without a clear app-level naming/invite
  flow;
- interest is only in the protocol implementation, not in using apps on it;
- the best argument remains "decentralized self-hosting."

## Decision

Proceed with one final v0 push only.

The next work is design-first:

1. settle recipient-controlled two-way communication;
2. implement the smallest generic ingress primitive;
3. build Spoke;
4. make Jolt distributable enough to demo;
5. freeze;
6. publish and judge honestly.

If v0 does not create product pull, stop.

## Verification

Docs-only scope decision. No code tests were run.
