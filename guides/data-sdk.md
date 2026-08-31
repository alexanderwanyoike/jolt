# Data SDK fundamentals

```meta
Guide: 02
Kicker: JOLT DATA SDK GUIDE
Level: Beginner
SDK: jolt-sdk/data
Description: Understand the small set of Data SDK building blocks before using them in an application.
```

The Data SDK lets an application describe its data and use it through one typed
interface. Jolt handles identities, signed storage, paths, permissions, and
network synchronization underneath.

If you want to build something immediately, start with
[Build Chirp](app-development.html). Come back here when you want the concepts
without the React screen around them.

## Pick the page you need

- **Build a first app:** follow the complete [Chirp tutorial](app-development.html).
- **Understand the building blocks:** continue on this page.
- **Change an Item:** read [Item mutations](data-sdk-mutations.html).
- **Change stored data safely:** read [Schema migrations](data-sdk-migrations.html).
- **Look up one exact method:** use the [generated API reference](../sdk/reference.html#module-data).

## Three building blocks

A Jolt data application has three parts:

1. A **Schema Class** says what one value looks like. The class is both the
   TypeScript type and the runtime validator.
2. A **Resource** says how values are stored and who may read or change them.
   A Collection stores many Items; a Document stores one Item per identity.
3. An **App** gives those Resources names and produces the interface your code
   connects to or tests.

This small Notebook app contains all three:

@include sdks/js/guide/src/fundamentals/data-sdk.ts as src/notebook.ts

There is no decoder, inferred type alias, path prefix, capability string, or
revision token to maintain. `Notebook.connect()` gives the real daemon-backed
interface. `Notebook.test()` gives the same typed interface in memory.

## Values and Items are different

`Note` is the application value: its text and creation time. An Item wraps that
value with the stable reference and state Jolt needs to update, delete, or
restore it safely.

@include sdks/js/guide/src/fundamentals/data-sdk-usage.ts as src/create-note.ts

Application code changes data through the Item methods. Each successful change
returns a new immutable Item, which fits naturally into React or another state
container.

## Access is part of the Resource

The `access` object is both the application rule and the permission declaration
Jolt derives for approval. In Notebook, only the signed-in identity can read
Notes, and that identity may create, update, delete, and restore them.

Use `Read.AnyIdentity` only for genuinely public data. A public read rule does
not make writes public; create and mutation authority remain explicit.

## Test without a daemon

Use `App.test()` for one isolated identity. Use `App.testWorld()` when two or
more identities should share deterministic state. The fake uses the same
schemas, migrations, access rules, Item states, errors, and conflict policies
as the connected interface.

The fake is for fast application tests. It does not replace real-daemon tests
for persistence, restart behavior, authorization, or multi-node networking.

## Where to go next

- Build the [beginner Chirp application](app-development.html).
- Update, replace, delete, and restore with [Item mutations](data-sdk-mutations.html).
- Learn how to evolve a Schema Class with [deterministic migrations](data-sdk-migrations.html).
- Use the [Data SDK API reference](../sdk/reference.html#module-data) for exact signatures.
