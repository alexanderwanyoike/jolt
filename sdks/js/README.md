# jolt-sdk

TypeScript SDK for building applications on the [Jolt](https://github.com/alexanderwanyoike/jolt) network.

Jolt applications do not own accounts. Define typed application data with
Schema Classes, compose it through `App.create`, and connect or test it through
one generated interface. Jolt handles identity, signed storage, paths,
compatibility, and scoped approval. The low-level client remains available for
applications that need explicit protocol control. The SDK is extracted from
the seam proven in
[Spoke](https://github.com/alexanderwanyoike/spoke) and
[Pastey](https://github.com/alexanderwanyoike/pastey).

## Install

From npm:

```sh
yarn add jolt-sdk
```

Or from a Jolt release tarball:

```sh
yarn add https://github.com/alexanderwanyoike/jolt/releases/latest/download/jolt-sdk.tgz
```

## Layers

| Import | What it is |
|---|---|
| `jolt-sdk/data` | High-level Schema Classes and typed application data APIs |
| `jolt-sdk` | Types, typed errors, and `createJoltClient`: tolerant, domain-shaped operations |
| `jolt-sdk/transport-http` | `fetch`-based transport for browsers and Node.js 18+ |
| `jolt-sdk/transport-tauri` | Tauri `invoke`-based transport for desktop shells |
| `jolt-sdk/testing` | `createFakeJolt`: a deterministic in-memory fake for tests |

Advanced hosts that already own an approved session can construct the
subscription-capable client required by `App.connect({ identity, client })`
without exposing those operations on the ordinary application client:

```ts
import { createDataClient } from "jolt-sdk";

const client = createDataClient({ transport, getSessionToken });
const app = await MyApp.connect({ identity, client });
```

Beginner applications use `MyApp.connect()` and do not construct either client.
`createJoltClient` intentionally omits raw Data Subscription operations.

Schema Classes use NestJS-style TypeScript decorators. Enable
`experimentalDecorators` in the application's `tsconfig.json` when using the
`jolt-sdk/data` surface.

## Quick start

```ts
import { App, Collection, Field, Read, Schema, State } from "jolt-sdk/data";

@Schema({ version: 1 })
class Post {
  @Field.string()
  text!: string;

  @Field.dateTime()
  postedAt!: Date;
}

const Posts = Collection.create(Post, {
  access: {
    read: Read.AnyIdentity,
    create: true,
    update: true,
    delete: true,
    restore: true,
  },
});

const Chirp = App.create({
  id: "chirp.example",
  name: "Chirp",
  namespace: "chirp",
  data: { posts: Posts },
});

const chirp = await Chirp.connect();
console.log(`Connected as ${chirp.identity}`);
const created = await chirp.posts.create({
  text: "Hello, Jolt!",
  postedAt: new Date(),
});
const updated = await created.update({ text: "Hello, everyone!" });
const deleted = await updated.delete();

if (deleted.state === State.Deleted && deleted.isDeleted()) {
  await deleted.restore({ text: "Hello again!", postedAt: new Date() });
}
```

`Chirp.connect()` checks compatibility, selects the local host, derives the
exact access request, reuses an approved session, and waits for approval when
needed. Use `Chirp.test()` for the same typed interface in memory.

See the [beginner Chirp guide](https://alexanderwanyoike.github.io/jolt/guides/app-development.html)
for the complete compile-checked walkthrough.

## Low-level API reference

The package still exports protocol-facing primitives for Jolt runtime and SDK
work. They are documented in the generated API reference; application
tutorials use `jolt-sdk/data`.

## App API compatibility

Applications declare the generic App API behavior they require; they do not
compare Jolt daemon release versions. Check before activating an app update and
whenever the app establishes a daemon connection:

```ts
const compatibility = await jolt.checkCompatibility({
  appApi: 1,
  requiredFeatures: {},
  optionalFeatures: { "data.subscriptions": 1 },
});

if (compatibility.status === "incompatible") {
  // Keep the installed app version and direct the user to upgrade Jolt.
}

if (!compatibility.optionalFeatures["data.subscriptions"]?.supported) {
  // Use an explicit app-owned fallback or hide the optional feature.
}
```

Feature discovery is unauthenticated and connection-scoped. Pass
`{ refresh: true }` after daemon reconnection. A reachable older daemon without
feature discovery is reported as the Legacy App API v1 Baseline; connection
failure remains a `JoltTransportError`, not an incompatibility result. App API
Features describe implemented behavior and remain separate from app-session
authorization capabilities.

Signed update manifests carry the declaration in its JSON wire shape. Decode
that untrusted metadata before checking it; invalid App API levels or feature
maps fail closed:

```ts
import { decodeAppCompatibilityDeclaration } from "jolt-sdk";

const declaration = decodeAppCompatibilityDeclaration(
  update.rawJson.app_compatibility
);
const prospectiveCompatibility = await jolt.checkCompatibility(declaration, {
  refresh: true,
});
```

Compatibility checks throw when no honest result is available. Use the shared
classifier to keep an unavailable attempt distinct from incompatibility while
leaving recovery wording and presentation in the application:

```ts
import { isJoltUnavailableError } from "jolt-sdk";

try {
  await jolt.checkCompatibility(declaration, { refresh: true });
} catch (error) {
  if (isJoltUnavailableError(error)) {
    // Show the app's unavailable state and offer its own retry flow.
  } else {
    throw error;
  }
}
```

The classifier accepts typed transport failures and browser host-gateway
responses with status 500 or 502. It describes the failed attempt, not proof
that a daemon process is offline, and does not hide arbitrary `TypeError`s.

Reads are tolerant: missing, unreachable, or undecodable content returns
`null` instead of throwing, so one bad record never poisons an app
projection. Failures from publishes and sends throw `JoltApiError` (the
daemon answered with an error) or `JoltTransportError` (the daemon was
unreachable); every operation accepts `{ signal, timeoutMs }`.

Encrypted applications can use `openEncrypted()` when a ciphertext-only result
must remain visible instead of collapsing to `null`. Delegated availability is
an explicit app choice through `pinHomeRelay()`; the in-memory fake implements
both contracts, including local-only to relay-backed inventory transitions.

## Testing your app

```ts
const chirp = Chirp.test({ identity: "alice.jolt" });
const post = await chirp.posts.create({
  text: "No daemon needed",
  postedAt: new Date(),
});

console.log(post.value.text);
```

Advanced low-level applications can still use `createFakeJolt` from
`jolt-sdk/testing`.

## Documentation

Full API reference is generated from the TSDoc comments with
`yarn docs` (typedoc) and published on the
[Jolt website](https://alexanderwanyoike.github.io/jolt/sdk/). The app
development guide builds Chirp through `jolt-sdk/data`.

## Development

```sh
yarn install
yarn test        # vitest
yarn typecheck
yarn build       # emits dist/
yarn docs        # emits docs/api.json + docs-html/
```
