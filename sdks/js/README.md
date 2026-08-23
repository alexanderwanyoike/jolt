# jolt-sdk

TypeScript SDK for building applications on the [Jolt](https://github.com/alexanderwanyoike/jolt) network.

Jolt applications do not own accounts. They ask the local Jolt daemon for a
capability-scoped session that the user approves in Jolt Console, then publish
signed content under the user's identity, read other identities' content, and
exchange encrypted objects through recipient-controlled ingress. This SDK is
the typed client for that contract, extracted from the seam proven in
[Spoke](https://github.com/alexanderwanyoike/spoke) and
[Pastey](https://github.com/alexanderwanyoike/pastey).

## Install

From a jolt release tarball (no registry required):

```sh
yarn add https://github.com/alexanderwanyoike/jolt/releases/latest/download/jolt-sdk.tgz
```

## Layers

| Import | What it is |
|---|---|
| `jolt-sdk` | Types, typed errors, and `createJoltClient`: tolerant, domain-shaped operations |
| `jolt-sdk/transport-http` | `fetch`-based transport for browsers and Node.js 18+ |
| `jolt-sdk/transport-tauri` | Tauri `invoke`-based transport for desktop shells |
| `jolt-sdk/testing` | `createFakeJolt`: a deterministic in-memory fake for tests |

## Quick start

```ts
import { createJoltClient } from "jolt-sdk";
import { HttpTransport } from "jolt-sdk/transport-http";

let token = "";
const jolt = createJoltClient({
  transport: new HttpTransport({ daemonUrl: "http://127.0.0.1:9862" }),
  getSessionToken: () => token,
});

// 1. Ask for a scoped session; the user approves it in Jolt Console.
const status = await jolt.getStatus();
const request = await jolt.requestSession({
  appId: "myapp.local",
  appName: "My App",
  appOrigin: window.location.origin,
  identity: status.identity_address,
  capabilities: ["publish:/myapp/*", "resolve:public", "fetch:public"],
});

// 2. Poll until approved.
for (;;) {
  const s = await jolt.getSessionRequestStatus(request.request_id);
  if (s.session_token) { token = s.session_token; break; }
  await new Promise((r) => setTimeout(r, 1000));
}

// 3. Publish signed content and read it back, versioned and decoded.
await jolt.publishJson("/myapp/profile", { name: "Alice" });
const profile = await jolt.read(
  { identity: status.identity_address, path: "/myapp/profile" },
  (v) => (typeof v === "object" && v && "name" in v ? (v as { name: string }) : null)
);
```

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
import { createFakeJolt } from "jolt-sdk/testing";

const { client, sent, deliverIngress } = createFakeJolt("alice.jolt", {
  appApi: 1,
  features: { "data.documents": 1 },
});
// client satisfies JoltClient and all of its sub-interfaces; sends are
// recorded in `sent`, and deliverIngress() injects incoming envelopes.
```

## Documentation

Full API reference is generated from the TSDoc comments with
`yarn docs` (typedoc) and published on the
[Jolt website](https://alexanderwanyoike.github.io/jolt/sdk/). The app
development guide walks through building a small social app with Tauri.

## Development

```sh
yarn install
yarn test        # vitest
yarn typecheck
yarn build       # emits dist/
yarn docs        # emits docs/api.json + docs-html/
```
