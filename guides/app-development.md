# Build Chirp with the Data SDK

```meta
Guide: 01
App: Chirp
Level: Beginner
SDK: jolt-sdk/data
Description: Build a typed social app with Schema Classes, App.create, automatic connection setup, and an in-memory test. No paths, decoders, sessions, or protocol knowledge required.
```

Chirp is a tiny social app: people publish posts under their Jolt identities and other people can read them. In this beginner guide you will describe a post, create the app, change a post, delete and restore it, and test two people sharing data. Jolt handles identity, signed storage, paths, compatibility checks, and permissions.

Every TypeScript file on this page comes from [`sdks/js/guide/src/beginner`](https://github.com/alexanderwanyoike/jolt/tree/dev/sdks/js/guide/src/beginner). The repository compiles and tests them against the public SDK on every change.

## 1 · Install the SDK

Create any TypeScript application and add Jolt:

```bash
yarn add jolt-sdk
```

Schema Classes use standard TypeScript decorators. Enable them in `tsconfig.json`:

```json tsconfig.json
{
  "compilerOptions": {
    "experimentalDecorators": true
  }
}
```

You need a running [Jolt Console](../#download) only when connecting the app. Tests run entirely in memory.

## 2 · Describe Chirp's data

Create `src/chirp.ts`:

@include sdks/js/guide/src/beginner/chirp.ts as src/chirp.ts

That one file gives Chirp a typed `posts` Collection:

- `Post` is both the TypeScript type and the runtime schema. There is no decoder or separate inferred type.
- `Collection.create` declares what the app may do. `Read.AnyIdentity` makes other people's posts readable.
- `App.create` derives Chirp's storage paths and required permissions from the declaration.
- Concurrent edits use Jolt's automatic defaults. Beginner code does not need a conflict policy.

Arrays, nested Schema Classes, and optional fields use the same decorators: `@Field.array(...)`, `@Field.schema(...)`, and `{ optional: true }`.

## 3 · Create, read, update, delete, and restore

Create `src/posts.ts`:

@include sdks/js/guide/src/beginner/posts.ts as src/posts.ts

Call `exerciseConnectedPostLifecycle()` from your application. `Chirp.connect()` finds the local Jolt host, checks compatibility, derives the exact permissions from the app definition, reuses an approved session when possible, and waits for approval in Jolt Console when needed.

Each operation returns an immutable Item. Updating a post returns a new Item with the same stable `ref`. The old Item does not change. Compare `item.state` with `State.Present` or use helpers such as `isPresent()` and `isDeleted()` to narrow the Item before accessing state-specific fields and methods.

## 4 · Test without a daemon

Install Vitest and create `src/chirp.test.ts`:

```bash
yarn add --dev vitest
```

@include sdks/js/guide/src/beginner/chirp.test.ts as src/chirp.test.ts

`Chirp.test()` creates a fresh isolated app for a fast unit test. `Chirp.testWorld()` gives several identities shared deterministic state, so Alice can publish and Bob can read through the same typed interface. Neither test needs Jolt Console, a daemon, or a network.

Run it:

```bash
yarn vitest run
```

The connected app and the in-memory app expose the same Posts API. Move from the test to a real Jolt connection without rewriting your application code.

## 5 · Advanced topics

Keep the first app small. These features are available when the application actually needs them:

- [Migrations](../sdk/reference.html#data.Migrations) upgrade older stored Schema values through explicit, deterministic steps.
- [Manual conflicts](../sdk/reference.html#data.UpdateConflict) let an advanced Resource expose concurrent alternatives instead of using the automatic defaults.
- **Content References** identify one exact immutable content version. Ordinary Data SDK relationships use a stable logical [`Ref`](../sdk/reference.html#data.Ref); exact verified `contentId` handling lives in the [advanced low-level guide](advanced-app-development.html).
- [bulk mutations](../sdk/reference.html#data.BulkMutationResult) run independent itemwise creates, updates, deletions, or restores and report indexed partial success. They are not transactions.

For explicit paths, raw content IDs, transports, compatibility Features, session Capabilities, ingress, encryption, or relay pinning, continue with [Advanced app development with Chirp](advanced-app-development.html). The low-level `jolt-sdk` entry point remains public; it simply is not required for this beginner app.
