# Domain documentation

Jolt uses a multi-context domain-documentation layout in the canonical private sibling repository at `../jolt-development-docs/`.

## Before starting work

Read:

1. This repository's `AGENTS.md`.
2. `../jolt-development-docs/current-context.md`.
3. `../jolt-development-docs/CONTEXT-MAP.md`.
4. The relevant documents under `../jolt-development-docs/contexts/`.
5. Any active or related cards under `../jolt-development-docs/cards/`.

## Context ownership

- `contexts/data-sdk/` owns the developer-facing Data SDK language and contracts.
- `contexts/app-sdk-compatibility/` owns App SDK and node compatibility boundaries.
- `contexts/follower-scale-data-plane/` owns follower-scale data-plane design.
- The Jolt source repository remains implementation-focused.
- Do not create or use `jolt/.notes`.

## Working with domain language

- Use the terminology defined by the relevant context documents.
- Flag contradictions between code, cards, and domain documents before silently choosing one.
- Update the owning context when a reviewed decision changes its contract.
- Keep protocol concepts application-agnostic.

In particular, protocol code may understand identities, paths, CIDs, signed operations, tombstones, revision context, features, and authorization records. It must not hardcode application concepts such as posts, feeds, profiles, galleries, or timelines.
