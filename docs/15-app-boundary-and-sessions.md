# App Boundary and Sessions

## Status

Design proposal for card `042`.

This document defines the near-term daemon/app boundary for external Jolt apps such as Pastey and Drops. It does not define the future WASM lens/runtime model.

## Problem

Pastey proved that a separate app can use Jolt through the local daemon:

```text
Pastey -> localhost daemon API -> Jolt network
```

That is the right product direction, but the current daemon API is trusted and debug-oriented. Any local app that can reach the daemon can currently ask it to publish, fetch, pin, or inspect state. That is acceptable for development, but not for a platform where the daemon owns identities and private keys.

Jolt needs a local authorization boundary before external apps become real.

## Core Boundary

```text
Jolt daemon
  local authority
  owns identities, keys, settings, content store, cache, network access

Jolt Console
  privileged local control surface
  manages daemon, identities, relays, app grants, diagnostics

Jolt apps
  untrusted clients
  request scoped sessions before using daemon authority

Jolt network
  untrusted transport/storage/discovery layer
  verifies signatures, CIDs, provider hints, and encrypted bytes
```

The network layer must not know about Pastey, Drops, app sessions, prompts, or user interface permissions. Session checks happen before the daemon performs a network operation.

## Design Principles

- Apps never receive long-term private keys.
- Apps do not get ambient authority just because they run on localhost.
- Sessions are explicit and scoped to an app, identity, capability set, and optional path prefix.
- Prompt for authority expansion, not routine use.
- The daemon validates every app API request against the session grant.
- Relays provide availability, not authority.
- Signatures and CIDs remain the source of truth for network data.
- Jolt Console is privileged; normal apps are not.

## Trust Classes

### Admin / Console

Jolt Console is the trusted local control surface. It can manage daemon-level authority:

- view daemon state
- create/import identities
- configure relays
- approve/revoke app sessions
- inspect published content/cache
- perform diagnostics

Some actions still need explicit confirmation even in Console:

- export private key
- delete identity
- rotate root keys
- wipe local store
- revoke device keys

### Normal Apps

Normal apps receive scoped sessions. They can request capabilities such as:

- resolve public `.jolt` addresses
- fetch public content
- publish under a path prefix
- list inventory under a path prefix
- pin content they published
- later: encrypt or decrypt under approved paths

Normal apps must never receive capabilities for:

- export private keys
- delete identities
- approve other apps
- grant arbitrary signing
- rotate root identity keys
- change daemon security settings
- manage global trusted relays
- wipe local store
- read unrestricted daemon data

### Network Peers and Relays

Network peers and relays are untrusted. They may provide:

- content bytes
- encrypted bytes
- provider hints
- relay records
- identity-head hints
- cached or pinned availability

They cannot mutate a `.jolt` identity unless they produce a valid signature from that identity key or a future valid delegated authority.

## Session Lifecycle

```text
requested -> pending -> approved -> active -> revoked
                      -> rejected
                      -> expired
```

### Requested

An app submits a session request:

```json
{
  "app_id": "pastey.local",
  "app_name": "Pastey",
  "app_origin": "http://127.0.0.1:5174",
  "requested_identity": "ys7w...2jsa.jolt",
  "requested_capabilities": [
    "resolve:public",
    "fetch:public",
    "publish:/pastes/*",
    "inventory:/pastes/*",
    "pin:own:/pastes/*"
  ]
}
```

`app_id` is not a cryptographic proof in v0. It is an app-declared identifier used for local grants and display. A stronger installed-app identity can come later.

### Pending

The daemon stores the request and returns:

```json
{
  "request_id": "req_...",
  "status": "pending"
}
```

The app can poll request status. Jolt Console shows the pending request.

### Approved

The user approves through Jolt Console. Approval selects:

- identity to use
- exact granted capabilities
- expiry policy

The daemon creates a session:

```json
{
  "session_id": "sess_...",
  "session_token": "secret random token",
  "app_id": "pastey.local",
  "identity": "ys7w...2jsa.jolt",
  "capabilities": [
    "resolve:public",
    "fetch:public",
    "publish:/pastes/*",
    "inventory:/pastes/*",
    "pin:own:/pastes/*"
  ],
  "expires_at": null
}
```

The app sends the token on app API calls:

```text
Authorization: Bearer <session_token>
```

### Rejected

The user rejects the request. The app receives a rejected status and should explain that it cannot operate without permission.

### Revoked

The user revokes an active session through Console. The daemon rejects future calls for that token.

### Expired

Sessions may expire if the grant has an expiry. v0 can support non-expiring local grants plus explicit revocation, but the data model should include expiry from the start.

## Capability Vocabulary

Capabilities are deliberately coarse. They should model workflows, not every individual operation.

### v0 Pastey Capabilities

```text
resolve:public
fetch:public
publish:/pastes/*
inventory:/pastes/*
pin:own:/pastes/*
```

Meaning:

- `resolve:public`: app may resolve public `.jolt` addresses.
- `fetch:public`: app may fetch public content by CID or `.jolt` target.
- `publish:/pastes/*`: app may publish signed path updates under `/pastes/*` using the granted identity.
- `inventory:/pastes/*`: app may list local published content under `/pastes/*`.
- `pin:own:/pastes/*`: app may ask the home relay to pin content it published under `/pastes/*`.

### Private Content Capabilities

```text
encrypt:/pastes/*
decrypt:/pastes/*
publish:encrypted:/pastes/*
share:/pastes/*
```

Meaning:

- `encrypt:/pastes/*`: app may ask the daemon to encrypt objects under `/pastes/*`.
- `decrypt:/pastes/*`: app may ask the daemon to decrypt fetched objects under `/pastes/*` when the local identity is an authorized recipient.
- `publish:encrypted:/pastes/*`: app may publish encrypted object bytes under `/pastes/*`.
- `share:/pastes/*`: app may update recipient/access metadata for objects under `/pastes/*`.

Sharing and access-grant mutation require later cards.

### Future Drops Capabilities

```text
publish:/drops/*
inventory:/drops/*
pin:own:/drops/*
```

Drops should not require new daemon authority categories if the path-scoped model is correct.

## Path Scope Rules

Path scopes must be strict.

If an app has:

```text
publish:/pastes/*
```

It may publish:

```text
/pastes/hello
/pastes/folder/note
```

It may not publish:

```text
/drops/game
/profile
/pastes
/pastes-evil/foo
/../profile
```

Paths should be parsed and normalized by structured path logic, not ad hoc string checks.

## API Split

### Existing Trusted API

The existing `/api/v1/*` endpoints remain the trusted development/admin API for now:

```text
/api/v1/status
/api/v1/publish
/api/v1/fetch
/api/v1/resolve
/api/v1/published
/api/v1/home-relay/pins
```

This keeps current CLI, dashboard, tests, and canaries stable.

Long term, these endpoints should either become Console/admin-only or require a local admin channel.

Private operations must not be added to this legacy trusted surface. Encryption,
decryption, and sharing authority should go through capability-checked
`/app/v1/*` APIs or explicit Console/admin APIs.

### App API

New external app endpoints should live under `/app/v1/*` and require a session token:

```text
POST /app/v1/sessions/request
GET  /app/v1/sessions/{request_id}
GET  /app/v1/session
POST /app/v1/resolve
POST /app/v1/fetch
POST /app/v1/publish
GET  /app/v1/published
POST /app/v1/home-relay/pins
```

Every endpoint checks:

- token exists
- token is active
- token is not expired
- capability allows operation
- path is inside granted scope
- identity matches the session grant where signing is needed

### Admin / Console API

Console/admin endpoints should live under `/admin/v1/*`:

```text
GET  /admin/v1/app-requests
POST /admin/v1/app-requests/{request_id}/approve
POST /admin/v1/app-requests/{request_id}/reject
GET  /admin/v1/app-sessions
POST /admin/v1/app-sessions/{session_id}/revoke
GET  /admin/v1/identities
POST /admin/v1/identities/import
```

In v0, the Console is served by the daemon on localhost and can use these admin endpoints without a separate auth mechanism. Before remote admin access exists, this must remain localhost-first.

## Session Storage

The daemon should persist session state under its data directory:

```text
<data-dir>/
  app-sessions.json
```

The stored form should include:

- request ID
- session ID
- app ID
- app name
- origin if known
- identity
- requested capabilities
- granted capabilities
- status
- created time
- approved/rejected/revoked time
- expiry
- last used time

Session tokens should not be stored in plaintext if avoidable. Store a token hash and compare presented tokens by hash.

## Identity Selection

An app session is pinned to one identity.

The daemon may have a default identity for convenience, but apps should not silently follow a mutable global current identity. If the user changes the daemon default identity later:

```text
existing app sessions keep their granted identity
new session requests may default to the new identity
```

Apps that need multiple identities should create multiple sessions.

Example:

```text
Pastey session A -> alice-public.jolt -> /pastes/*
Pastey session B -> alice-private.jolt -> /pastes/*
```

## Pastey v0 Grant

Pastey should request:

```text
app_id: pastey.local
identity: selected by user in Console
capabilities:
  resolve:public
  fetch:public
  publish:/pastes/*
  inventory:/pastes/*
  pin:own:/pastes/*
```

That lets Pastey:

- publish public pastes
- list local public pastes
- fetch public `.jolt` paste addresses
- pin its own pastes if the daemon has a home relay

It does not let Pastey:

- export keys
- delete identities
- publish profile data
- publish drops
- decrypt private content
- approve other apps
- change relay settings

## UX Rules

Bad:

```text
Pastey wants to publish /pastes/foo. Allow?
Pastey wants to fetch alice.jolt/pastes/bar. Allow?
Pastey wants to pin bafk... Allow?
```

Good:

```text
Pastey wants to use Alice Public Space to:
- read public Jolt content
- create and edit pastes under /pastes/*
- pin pastes it publishes

Allow:
[Once] [Until Quit] [Always]
```

v0 can implement `Always until revoked` first. The data model should allow shorter durations later.

## Threat Model

### Protected Against

- A random localhost app publishing under arbitrary Jolt paths.
- A Pastey-like app publishing Drops or profile data without approval.
- App authority silently changing when the daemon default identity changes.
- Apps using daemon keys directly.
- Revoked app sessions continuing to work.

### Not Protected Against Yet

- Malware with full local machine access.
- Browser origin spoofing for manually run local dev apps.
- A malicious app that misuses already-granted capabilities.
- A user approving a malicious app.
- Private content leakage; encrypted objects are not implemented yet.

## Non-Goals

- WASM runtime design.
- App marketplace.
- Remote admin access.
- Payment or entitlements.
- Encrypted content implementation.
- Device-key delegation.
- Perfect installed-app identity.

## Implementation Order

1. Implement session store and approval API.
2. Add capability-checked `/app/v1` endpoints.
3. Add Jolt Console UI for pending requests and active sessions.
4. Move Pastey to app sessions.
5. Design encrypted objects before private Pastey.

## Open Questions

- v0 supports persistent grants plus explicit revoke first. `Once` and
  `Until Quit` can be added later because the data model already has expiry.
- v0 app session tokens are bearer tokens. `app_origin` remains display and
  audit metadata, not cryptographic proof of app identity.
- Console/admin endpoints remain localhost-first. Binding admin APIs away from
  localhost requires a separate admin-channel design.
- Path-scoped capabilities keep operation separation: `publish`, `inventory`,
  and `pin:own` are distinct grants. Future private grants should be distinct
  too: `encrypt`, `decrypt`, and `share`.
- Capability names remain strings on the wire for v0, but the daemon parses
  them into strict internal capability values before approval and enforcement.
  Approval may grant exactly what the app requested or a narrower path scope,
  never broader authority.
