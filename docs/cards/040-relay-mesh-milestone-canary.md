# 040: Relay Mesh Milestone Canary

**Type:** AFK  
**Milestone:** M5+  
**Status:** Ready after 033-039  
**Blocked by:** 033, 034, 035, 036, 037, 038, 039

## Why

The relay gossip slices should be proven with local process demos first. We should not need a Hetzner canary for every card.

Once the relay mesh work lands, run one real-world canary to prove the milestone:

```text
Alice has home relay R1
Tim starts knowing only R2
Bob has home relay R3
R1, R2, and R3 can discover each other through the relay mesh
```

The goal is to prove that a node can join through one reachable relay and still find identities whose home relays were not manually configured.

## What to Build

Add a documented canary script or checklist for the relay mesh milestone.

The canary should cover:

- Relay record exchange.
- Relay mesh exploration.
- Identity provider query forwarding.
- Identity head hint usage when available.
- Clear failure messages when a relay or provider is unreachable.

## Acceptance Criteria

- [ ] A fresh Tim node starts with only R2 configured.
- [ ] Alice publishes and pins content through R1.
- [ ] Bob publishes and pins content through R3.
- [ ] Tim resolves Alice and Bob without manually configuring R1 or R3.
- [ ] Tim fetches Alice and Bob content by `.jolt` address.
- [ ] Tim's status/API/dashboard show learned relay count.
- [ ] A controlled relay outage produces a structured failure reason.
- [ ] The canary instructions are documented step by step.

## Verification

Required:

- One real-world canary using the available Hetzner relay plus local machines.

Do not run this canary for every relay gossip card. Use local process demos for the implementation cards, then run this once at the milestone boundary.

## Non-Goals

- Stress testing.
- Large relay networks.
- Payment or storage-market behaviour.
- Production SLOs.
