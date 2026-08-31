# Test without a daemon

```meta
Guide: 07
Kicker: JOLT DATA SDK GUIDE
Level: Fundamentals
SDK: jolt-sdk/data
Description: Test typed application behavior in memory before running collaborative checks against real Jolt daemons.
```

An App Definition provides an in-memory test interface with the same typed
Resources and Items as `App.connect()`. Tests can exercise application rules
without starting Jolt Console, approving a session, or waiting for a network.

## Test one identity

Use `App.test()` for isolated application behavior. Each call starts with
fresh state, even when it uses the same identity string.

@include sdks/js/guide/src/fundamentals/testing-isolated.test.ts as src/task-list.test.ts

Schemas, migrations, access-shaped methods, immutable Items, lifecycle states,
and conflict policies all remain active. Test through `app.tasks` or another
generated Resource instead of arranging data through a separate fixture API.

## Test Alice and Bob

Use `App.testWorld()` when identities should share deterministic state. Views
from `world.as(identity)` represent application users in one synchronized test
world.

@include sdks/js/guide/src/fundamentals/testing-world.test.ts as src/feed.test.ts

Bob can read Alice's post because the Collection declares
`Read.AnyIdentity`. His remote view remains read-only, exactly as it does in a
connected application.

## Model separate installations only when needed

`world.as(identity)` is the simple choice for application journeys. It does
not model offline installations.

When a concurrency test genuinely needs independent copies, use
`world.device(identity, name)`. Named devices keep separate histories until
the test calls `world.sync()`. The advanced [Manual conflicts](data-sdk-manual-conflicts.html)
guide shows that shape with a workstation and laptop.

Do not introduce devices and synchronization into ordinary component or domain
tests. They exist to test concurrent-edit policy, not as general fixture setup.

## Know what the test interface proves

The in-memory interface is good for:

- schema validation and migrations;
- Resource method availability and application access shape;
- Item create, read, update, replace, delete, and restore behavior;
- automatic and Manual conflict policies; and
- Data Subscription and Change Stream application logic.

It does not prove Jolt Console approval, daemon feature compatibility,
capability enforcement or revocation, durable restart behavior, provider
discovery, or real multi-node networking. Keep a small real-daemon test for
those boundaries, as Jolt does for Chirp and Spoke.

## Run the tests

The guide examples use Vitest:

```bash
yarn vitest run
```

No special Jolt test runner or fixture language is required.

## Continue learning

- Return to [Data SDK fundamentals](data-sdk.html).
- Test retained remote views with [Data Subscriptions](data-sdk-subscriptions.html).
- Build the complete [beginner Chirp app](app-development.html).
