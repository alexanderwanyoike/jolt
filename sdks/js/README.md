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

Reads are tolerant: missing, unreachable, or undecodable content returns
`null` instead of throwing, so one bad record never poisons an app
projection. Failures from publishes and sends throw `JoltApiError` (the
daemon answered with an error) or `JoltTransportError` (the daemon was
unreachable); every operation accepts `{ signal, timeoutMs }`.

## Testing your app

```ts
import { createFakeJolt } from "jolt-sdk/testing";

const { client, sent, deliverIngress } = createFakeJolt("alice.jolt");
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
