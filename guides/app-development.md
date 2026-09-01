# Build Chirp with the Data SDK

```meta
Guide: 01
App: Chirp
Stack: Tauri 2 · React
Level: Beginner
SDK: jolt-sdk/data
Description: Build a small social app with profiles, follows, a live timeline, editing, deletion, restore, and an Alice/Bob test.
```

Chirp is a small social app. You can choose a nickname, publish a post, follow
another Jolt identity, see who you follow beside the timeline, edit your own
posts, and undo a deletion. There is no application server, account table, path
design, decoder, session code, or network polling to write.

This is a complete application tutorial. Every TypeScript file shown here is
compiled and tested against the public SDK in Jolt's repository. Follow it from
the top and you will finish with a working desktop app, not an isolated API
example.

> **Before you start:** Chirp requires Jolt **0.4.0 or newer**. Jolt 0.3.22
> does not yet provide the live Data SDK behavior used by this tutorial. This
> guide must remain on the development site until 0.4.0 is released.

## 1 · Create the app

Start with Tauri's React and TypeScript template:

```bash
yarn create tauri-app chirp --template react-ts
cd chirp
yarn
yarn add jolt-sdk
```

Schema Classes use TypeScript decorators. Add this option to the generated
`tsconfig.json`:

```json tsconfig.json
{
  "compilerOptions": {
    "experimentalDecorators": true
  }
}
```

## 2 · Let the desktop app talk to Jolt

The webview should not make direct requests to the local daemon. Add Jolt's
small Tauri plugin to `src-tauri/Cargo.toml`:

```toml src-tauri/Cargo.toml
[dependencies]
tauri-plugin-jolt = "0.1"
```

Register it in the builder Tauri generated:

```rust src-tauri/src/lib.rs
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_jolt::init())
        .run(tauri::generate_context!())
        .expect("error while running Chirp");
}
```

Then allow the plugin in `src-tauri/capabilities/default.json`:

```json src-tauri/capabilities/default.json
{
  "identifier": "default",
  "windows": ["main"],
  "permissions": ["core:default", "jolt:default"]
}
```

That is all the connection plumbing Chirp owns. `Chirp.connect()` will find
Jolt, check that the required Data SDK behavior is available, request the exact
permissions declared by the app, reuse an existing approval, and wait when a
new approval is required.

## 3 · Describe Chirp's data

Create `src/chirp.ts`:

@include sdks/js/guide/src/beginner/chirp.ts as src/chirp.ts

`Post`, `Profile`, and `Following` are ordinary classes and runtime schemas at
the same time. `Collection.create` gives every post a stable reference. The
public `profile` Document stores one nickname, while the private `following`
Document stores one identity list for the signed-in person.

The access declarations are also the complete permission declaration:

- anyone may read posts;
- anyone may read a Chirp nickname, but only its identity may change it;
- the local identity may create, edit, delete, and restore its posts; and
- only the local identity may read or change its following list.

`App.create` derives the paths and session request. There are no path prefixes,
capability strings, revision tokens, mutation IDs, or decoders in application
code. Automatic conflict handling is used unless the application explicitly
chooses a different policy.

## 4 · Add profiles and remember who the user follows

Create `src/profiles.ts`:

@include sdks/js/guide/src/beginner/profiles.ts as src/profiles.ts

`saveNickname()` creates the local profile once and updates the same typed
Document afterward. `getProfiles()` reads each public profile through its Jolt
identity. A nickname is friendly and deliberately non-unique, so Chirp always
keeps the canonical `.jolt` identity next to it on posts and in the Following
list. Missing profiles simply fall back to that identity.

Create `src/following.ts`:

@include sdks/js/guide/src/beginner/following.ts as src/following.ts

`getOrCreate` means a new Chirp user starts with an empty list while a returning
user receives the Document already stored under their Jolt identity. Updating
returns a new immutable Item, so React can replace its old state directly.

## 5 · Build the live timeline

Chirp's timeline reads the signed-in person's posts and the posts of every
identity they follow. Each identity gets one cache-first Data Subscription.

Think of a subscription as one local, verified window onto one person's Posts
Collection. It does not expose networking to Chirp. Jolt discovers that
person's nodes, verifies their signed records, retains the last good view, and
refreshes it in the background.

The Change Stream has a deliberate order:

1. It begins with a **Snapshot** containing the complete Last Verified View.
   Chirp can render that immediately, even while the other person is offline.
2. Later **Changed** events patch that view with verified additions, edits,
   deletions, and restores.
3. **ResyncRequired** means Chirp missed part of the stream, so it asks the
   subscription for a fresh complete view instead of guessing.

Create `src/timeline.ts`:

@include sdks/js/guide/src/beginner/timeline.ts as src/timeline.ts

`Timeline.open()` waits for that first Snapshot before it returns. This avoids
racing an older view read against a newer streamed change. One `Map` holds the
current Items for each followed identity; `publish()` combines those Maps and
sorts their typed `postedAt` values for React. The exhaustive `switch` makes
resynchronization and terminal events explicit instead of turning them into
strings for the UI to guess about.

Keep the React boundary small with `src/use-timeline.ts`:

@include sdks/js/guide/src/beginner/use-timeline.ts as src/use-timeline.ts

Opening, cancelling, and replacing timeline sources now follows the normal
React effect lifecycle. The component receives one immutable snapshot.

To keep this tutorial small, changing the follow list closes and rebuilds its
few subscriptions. A large production feed would reconcile long-lived
subscriptions instead; Chirp deliberately does not teach that optimisation.

## 6 · Add a post card

Create `src/PostCard.tsx`:

@include sdks/js/guide/src/beginner/PostCard.tsx as src/PostCard.tsx

Remote posts are read-only. Each card uses the separately loaded public profile
for its friendly nickname and keeps the post reference's Jolt identity visible
underneath. Chirp shows edit and delete controls only when that reference
belongs to `chirp.identity`. The callback receives the stable post reference;
the application asks its local Collection for the current Item before mutating
it.

## 7 · Build the screen

Replace the generated `src/App.tsx`:

@include sdks/js/guide/src/beginner/App.tsx as src/App.tsx

This is the whole product flow:

1. `Chirp.connect()` connects and exposes the local Jolt identity.
2. `getFollowing()` loads the user's saved follows.
3. `getProfiles()` loads public nicknames without replacing canonical IDs.
4. `useTimeline()` opens subscriptions for the user and their friends.
5. The profile form and composer save typed Documents and Posts.
6. Following somebody updates one typed Document and the sidebar.
7. Edit, delete, and restore call methods on typed Items.

Each button action uses the small `run()` helper. It clears the previous
message, runs the typed operation, and shows a dismissible error without
throwing away the rest of the screen. Connection and timeline startup errors
still stop the app because Chirp cannot work without those foundations.

The startup boundary catches `AppIncompatibleError` by type. Instead of showing
SDK terminology, Chirp asks the person to update Jolt and offers **Check again**.
Compatibility is checked before approval or data access, so this failure cannot
partly create the application.

Present Items carry `State.Present`; deleted Items carry `State.Deleted` and
offer `restore(...)` only when the Resource declaration allows it.

There is deliberately no transport setup, compatibility Feature map, App
Session Capability list, content identifier, or manual refresh loop in this
component.

Replace `src/App.css` as well:

@include sdks/js/guide/src/beginner/App.css as src/App.css

The scaffold's existing `src/main.tsx` already renders `<App />`, so there are
no other frontend files to change.

## 8 · Run Chirp

Confirm Jolt Console is version 0.4.0 or newer, start it, then run:

```bash
yarn tauri dev
```

The first launch waits at **Approve Chirp in Jolt Console**. Open Jolt Console,
review the generated request, and approve it. Chirp then shows the identity
owned by that Jolt installation.

Save a nickname and publish a post. Close and reopen Chirp: the profile, post,
and following Document remain under the same identity, and the timeline starts
from its retained verified view rather than waiting for the network.

## 9 · Test Alice and Bob without two daemons

Install Vitest:

```bash
yarn add --dev vitest
```

Create `src/chirp.test.ts`:

@include sdks/js/guide/src/beginner/chirp.test.ts as src/chirp.test.ts

`Chirp.test()` gives one isolated typed app. `Chirp.testWorld()` gives Alice and
Bob two identity-bound views of shared deterministic state. The tests use the
same public profiles, private following list, posts, Item mutations, Data
Subscription, and Change Stream interfaces as the desktop application; no
daemon or network is needed.

Run them:

```bash
yarn vitest run
```

## 10 · Run it with a friend

To see the real network path, run Chirp on two laptops that can discover one
another through Jolt:

1. Alice and Bob each start Jolt Console and Chirp.
2. Each approves Chirp's generated request.
3. Alice saves a nickname and publishes a chirp.
4. Bob enters Alice's `.jolt` identity in the follow form; Alice appears in his
   Following sidebar by nickname and canonical identity.
5. Alice's existing posts appear from Bob's retained verified view, and later
   verified changes arrive through the same timeline subscription.
6. Alice can follow Bob in the same way.

Following does not grant read permission: Chirp posts are public. It records
which public identities Bob wants in his timeline. Jolt remains responsible for
signed storage, provider discovery, verification, caching, and bounded refresh.

## 11 · Where to go next

The beginner app is complete. Reach for these only when a real requirement
appears:

- [Data SDK fundamentals](data-sdk.html) explains Schema Classes, Resources,
  Apps, Items, access, and testing without the React screen around them.
- [Item mutations](data-sdk-mutations.html) explains immutable Item snapshots,
  lifecycle states, and expected failure types outside the React UI.
- [Schema migrations](data-sdk-migrations.html) shows how to upgrade older
  stored values without adding historical model classes to the application.
- [Manual conflicts](data-sdk-manual-conflicts.html) expose concurrent
  alternatives instead of using the automatic defaults.
- [Data Subscriptions](data-sdk-subscriptions.html) explain retained remote
  views, freshness, Change Streams, and cleanup without React.
- [Data SDK testing](data-sdk-testing.html) separates fast in-memory app tests
  from the smaller set of checks that require real Jolt daemons.
- **Content References** identify one exact immutable content version. Normal
  relationships use stable logical [`Ref`](../sdk/reference.html#data.Ref)
  values.
- [bulk mutations](../sdk/reference.html#data.BulkMutationResult) perform
  independent itemwise operations with indexed partial-success results; they
  are not transactions.
