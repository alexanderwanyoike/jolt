# Advanced app development with Chirp

```meta
Guide: A1
App: Chirp
Stack: Tauri 2 · React
SDK: jolt-sdk
Kicker: JOLT ADVANCED APP DEVELOPMENT GUIDE
Description: Use Jolt's low-level SDK to build Chirp with explicit compatibility checks, sessions, paths, append records, ingress, relay availability, and transport setup.
```

This is the advanced, low-level Chirp guide. It is for applications that need explicit control over compatibility checks, sessions, paths, append records, ingress, transports, encryption, or relay availability. If you want ordinary typed application data, start with the [beginner Chirp guide](app-development.html); it lets the Data SDK own this machinery.

By the end you will have a running desktop app that checks the generic App API behavior available through Jolt, borrows the user's identity through a capability-scoped session, publishes signed posts, assembles a timeline from other people's nodes, exchanges follow requests through recipient-controlled ingress, and tests all of it against an in-memory fake. Requires a running [Jolt Console](../#download) and the [Jolt SDK](../sdk/).

Every code file on this page is lifted verbatim from [`sdks/js/guide`](https://github.com/alexanderwanyoike/jolt/tree/dev/sdks/js/guide) in the Jolt repository, where it is type-checked and unit-tested against the SDK on every change. If you follow along file by file, you end up with the same app.

## Why every Jolt app is social

Chirp has no backend, no user table, and no signup screen, and it does not need them. On Jolt, **identity comes from the network**: the local daemon holds the user's keys and signs everything Chirp publishes, so a "Chirp account" is just the user's existing Jolt identity wearing a different interface. And **distribution comes from the network**: anything Chirp publishes is resolvable and fetchable by any other node, and anything other identities publish under Chirp's paths is readable by your app.

That makes Jolt apps social by nature. The moment Chirp writes its first post, that post has a stable address any other app can read, and Chirp can read everyone else's. You are not building a silo with a network attached; you are building a lens over a network that already exists. The flip side is honest too: public publications are public. "Following" in Chirp is not permission to read (nobody needs permission to read public posts); it is a subscription list that decides whose posts your timeline assembles.

Chirp exercises the whole app surface specified in [JOLT-RFC-0007](../rfcs/0007-app-sessions.html): session bootstrap, signed publication, append records and enumeration, encrypted objects, and ingress.

## 1 · Scaffold a Tauri + React app

Start from the standard Tauri scaffold with the React and TypeScript template:

```bash
yarn create tauri-app chirp --template react-ts
cd chirp
yarn
yarn tauri dev
```

You should see the template window open. Everything Jolt-specific happens in two places: `src-tauri/` (one plugin registration) and `src/` (the SDK calls). By the end of this guide you will have touched exactly ten files:

```text
chirp/
├── src-tauri/
│   ├── Cargo.toml                    # add one dependency
│   ├── capabilities/default.json     # add one permission
│   └── src/lib.rs                    # add one line
└── src/
    ├── compatibility.ts              # app requirements  (section 3)
    ├── jolt.ts                       # client + session   (section 4)
    ├── chirp.ts                      # posts + timeline   (section 5)
    ├── follows.ts                    # ingress handshake  (section 6)
    ├── App.tsx                       # the UI             (section 7)
    ├── App.css                       # replace the scaffold's styles
    └── chirp.test.ts                 # executable fixtures (section 8)
```

## 2 · Add jolt-sdk and tauri-plugin-jolt

Add the SDK:

```bash
yarn add jolt-sdk
```

In a desktop shell the webview should not talk to the daemon directly; daemon calls go through audited Rust proxy commands. The [`tauri-plugin-jolt`](https://crates.io/crates/tauri-plugin-jolt) crate ships those commands so you never write them yourself. One dependency in `src-tauri/Cargo.toml`, next to the tauri dependencies the template generated:

```toml src-tauri/Cargo.toml
[dependencies]
tauri-plugin-jolt = "0.1"
```

One line in the builder in `src-tauri/src/lib.rs`:

```rust src-tauri/src/lib.rs
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_jolt::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

And one permission in `src-tauri/capabilities/default.json`, next to the defaults the template generated:

```json src-tauri/capabilities/default.json
{
  "identifier": "default",
  "windows": ["main"],
  "permissions": ["core:default", "jolt:default"]
}
```

On the TypeScript side, the Tauri transport pairs with the plugin when you pass `{ plugin: true }`, which you will do in the next section. The plugin reaches the daemon at `http://127.0.0.1:9862`; set `JOLT_DAEMON_URL` to override it.

## 3 · Declare what this Chirp release needs

Jolt and Chirp release independently, so Chirp checks behavior rather than comparing daemon release numbers. This release needs App API v1 and no new required features: all of its core publishing, reading, enumeration, ingress, and session operations belong to the long-lived Legacy App API v1 Baseline. Home-relay pinning is separate and optional. If Jolt advertises that behavior, Chirp may show a "keep available" control; otherwise the control stays hidden and chirps retain their honest default of local-node availability.

@include sdks/js/guide/src/compatibility.ts as src/compatibility.ts

There are three runtime outcomes:

- `ready` means the reachable daemon satisfies every required behavior. A reachable older daemon without feature discovery is still `ready` through the Legacy App API v1 Baseline; the SDK does not guess that it supports the optional feature.
- `incompatible` means Jolt answered, but its advertised App API behavior cannot run this Chirp release. Chirp can now accurately ask the user to upgrade Jolt before it requests a session or writes data.
- `unavailable` means the SDK could not reach Jolt. That is not evidence of incompatibility, so the app says to start or reconnect Jolt rather than making an upgrade claim.

The declaration uses only public `jolt-sdk` exports and stable App API contract levels. It neither constructs daemon endpoints nor reads daemon SemVer. `checkCompatibility()` caches discovery for the connection; pass `{ refresh: true }` when reconnecting to a replaced daemon rather than polling it before every operation.

Notice that the optional App API Feature and `pin:own:/chirp/*` are deliberately separate. The feature says the connected Jolt implements the generic home-relay pin contract. The capability says the user authorized this particular Chirp session to pin its own `/chirp/*` publications. Chirp shows the control only when both are true. Supporting an operation never grants an application permission to use it.

## 4 · Request a scoped session

Only after compatibility succeeds does Chirp declare who it is and exactly what it wants to do. Capabilities follow the grammar of [RFC 0007](../rfcs/0007-app-sessions.html): an action, optionally narrowed to a path scope with at most one trailing wildcard. Chirp asks for the baseline set it uses and conditionally requests home-relay pin permission when the optional behavior exists. The daemon refuses any call outside the granted set, and the user can narrow the grant further at approval time.

@include sdks/js/guide/src/jolt.ts as src/jolt.ts

Run the app and call `connect()`. The request now sits pending on the daemon: open **Jolt Console → Apps**, find the pending "Chirp" request, review the capability list, and approve it. The poll loop picks up the token and Chirp is connected. The token is a bearer secret scoped to exactly these capabilities; if the user revokes the session in Console, every call starts failing with `JoltApiError` and `connect()` will request a fresh session on next launch.

An existing active session does not gain new authorization merely because an upgraded daemon starts advertising the optional feature. Chirp keeps the relay control hidden until the user revokes that old session in Jolt Console, reconnects, and approves a new request containing `pin:own:/chirp/*`.

## 5 · Publish chirps, assemble timelines

A chirp is an append record: `publishAppend` writes coexisting records that never overwrite each other, which is exactly what a feed of posts wants (and it keeps concurrent devices safe). Each chirp gets its own path under `/chirp/posts/`, and readers list them back with `enumerate`, never with resolve. The follow list is the opposite kind of data: a singleton settings-like object at `/chirp/follows`, updated with `publishJson` where last-writer-wins is fine. It is published under your identity, so any Chirp instance on any device sees the same list.

@include sdks/js/guide/src/chirp.ts as src/chirp.ts

Note the shape of these functions: each takes the narrow interface it needs (`JoltAppendSdk`, `JoltAvailabilitySdk`, `JoltSdk`) rather than the whole client, and the clock is injectable. `postAvailableChirp` composes publication with an explicit app-owned availability request; ordinary `postChirp` keeps the existing local-only behavior. Publication and relay pinning are not atomic: if the relay request fails, the chirp remains successfully published with local availability. Every client sub-interface is intentionally small so features declare exactly the contract they use; this pays off in section 8.

Reads are tolerant: in `loadTimeline`, a record that is missing, unreachable, or not a valid chirp simply comes back `null` from `readContent` and is skipped, so one bad record never breaks the feed.

## 6 · Follow requests over ingress

Following someone requires no permission: their posts are public, and `follow()` above is enough to read them. What ingress adds is the social handshake, telling someone you exist without spam. On Jolt, one identity cannot write into another identity's state; the only way to hand an object to someone else is the **recipient-controlled ingress door**: the sender encrypts an object to the recipient and delivers it to the recipient's daemon, where it waits in a pending queue until the recipient's app opens it and decides.

`sendObject` does the sender's half in one call: it encrypt-publishes the object at the given path (that is the sender's own copy, under `publish:encrypted:/chirp/*`), then delivers the envelope to the recipient's daemon. On the receiving side, `listFollowRequests` lists the pending queue, opens each envelope to see what it is, and classifies it; the transport layer does not know or care what a "follow request" is, classifying payloads is the app's job. Envelopes that are not Chirp objects are left alone for whatever app they belong to, and envelopes whose claimed sender does not match the envelope's actual sender are rejected on sight. Deciding is deliberately left to the UI: accept and reject are one SDK call each.

@include sdks/js/guide/src/follows.ts as src/follows.ts

## 7 · The UI

One file ties it together. `App.tsx` checks compatibility and connects on mount, then renders four things: a composer that calls `postChirp` or the optional `postAvailableChirp`, a follow form that subscribes and says hello, the pending follow requests with accept and ignore buttons, and the timeline. Unavailable and incompatible Jolt states get different recovery copy and a **Check again** action before the social UI can mount. Every action ends by re-running `refresh`, so the UI is always a projection of daemon state; when Bob accepts Alice's request, Chirp accepts the envelope and follows back, so nothing lands in Bob's world without Bob's daemon holding it at the door first.

@include sdks/js/guide/src/App.tsx as src/App.tsx

Replace the scaffold's `src/App.css` with a small stylesheet (the scaffold's `main.tsx` already renders `<App />`, so no other file changes):

@include sdks/js/guide/src/App.css as src/App.css

Run `yarn tauri dev` again. Approve the session in Jolt Console when the window says it is waiting, and post your first chirp.

## 8 · Test it all with the fake

Because every Chirp function takes a client interface instead of reaching for a global, all of the flows above run against `createFakeJolt`: a deterministic in-memory implementation of the full `JoltClient` with no daemon and no network. Publishes land in an in-memory store, enumeration lists them back, sends are recorded, and `deliverIngress` injects incoming envelopes as if a remote sender delivered them. Add `vitest` (`yarn add -D vitest`) and drop this next to the code:

@include sdks/js/guide/src/chirp.test.ts as src/chirp.test.ts

These tests exercise Chirp's schemas and flows, not cryptography: the fake simulates encryption by recording recipients and storing plaintext, which is exactly the right level for app tests. The same code runs unchanged against the real daemon because `createFakeJolt` satisfies `JoltClient` and every sub-interface.

The compatibility fixtures are executable too. `featureDiscovery: "legacy"` models a reachable pre-discovery daemon and confirms that baseline Chirp stays usable with the optional control hidden. The default fake models current advertised discovery, and its `features` map can explicitly advertise home-relay pinning. A throwing `JoltTransportError` fixture proves that unavailability does not turn into an incompatible result. These are SDK results, not prose approximations or daemon-version guesses.

## 9 · Run it with a friend

The whole point of a Jolt app is that two installs of it form a network with no server in between. To see Chirp actually be social you need a second identity, either a friend running Jolt Console on their machine or your own second machine.

1. Both sides launch Jolt Console, run Chirp with `yarn tauri dev`, and approve the session request.
2. Swap `.jolt` addresses (each of you sees your own at the top of the Chirp window).
3. Each of you posts a chirp.
4. You type their address into the follow form. Two things happen at once: their existing posts appear in your timeline on the next refresh, because following is just reading public records, and a follow request lands at their daemon's door.
5. Their Chirp shows "Follow requests" with your address. When they hit **Accept**, Chirp follows back, and both timelines now interleave both authors, newest first.

If the other side is offline, nothing breaks: chirps are served by whichever nodes hold them (the author's devices, and relays if the author pins there), and the follow request waits in the daemon's ingress queue and retries. Delivery, retries, and queue persistence are the daemon's job, not yours.

## Where to go next

- The [SDK reference](../sdk/reference.html) documents every exported class, interface, function, and type, including the typed `operations` layer for endpoints the client does not wrap (binary publish, raw ingress send).
- [JOLT-RFC-0007](../rfcs/0007-app-sessions.html) specifies the session and capability model Chirp just used, including the grants this guide did not need: `inventory`, `pin:own`, `encrypt`, and `decrypt`.
- [JOLT-RFC-0003](../rfcs/0003-device-writer-logs.html) explains why append records from several devices merge deterministically, which is what made `publishAppend` safe to call from anywhere.
- For private content between chirpers, look at `publishEncryptedJson` and `readEncrypted` in the reference, backed by [JOLT-RFC-0004](../rfcs/0004-encrypted-device-access.html).
- Want your content reachable while your machines sleep? [Run your own relay](run-a-relay.html); it takes ten minutes and a small VPS.
