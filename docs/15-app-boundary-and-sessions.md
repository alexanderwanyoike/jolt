# App Boundary and Sessions

## Status

Implemented. Originally the design proposal for card `042`; updated to describe
the v0 surface as built, including append records, enumeration, encrypted
object APIs (see doc 16), and recipient ingress.

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
requested -> pending -> active -> revoked
                     -> rejected
                     -> expired
```

The stored session states are `pending`, `active`, `rejected`, `revoked`, and
`expired`. Approval moves a session directly from `pending` to `active`; there
is no separate `approved` state.

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

### Active

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

### Implemented Capability Grammar

The full set of grantable capabilities as implemented:

```text
resolve:public
fetch:public
ingress:send
ingress:read
ingress:decide
enumerate:self:<path>
enumerate:any:<path>
publish:<path>
publish:encrypted:<path>
inventory:<path>
pin:own:<path>
encrypt:<path>
decrypt:<path>
```

`<path>` is either an exact path like `/pastes/note` or a single trailing
wildcard prefix like `/pastes/*`.

### v0 Pastey Capabilities

```text
resolve:public
fetch:public
enumerate:self:/pastes/*
publish:/pastes/*
inventory:/pastes/*
pin:own:/pastes/*
```

Meaning:

- `resolve:public`: app may resolve public `.jolt` addresses.
- `fetch:public`: app may fetch public content by CID or `.jolt` target.
- `enumerate:self:/pastes/*`: app may enumerate append records under
  `/pastes/*` only for the identity attached to its session.
- `enumerate:any:/spoke/*`: a social/discovery app may explicitly request
  cross-identity enumeration, still restricted to its approved path namespace.
- `publish:/pastes/*`: app may publish signed path updates under `/pastes/*` using the granted identity.
- `inventory:/pastes/*`: app may list local published content under `/pastes/*`.
- `pin:own:/pastes/*`: app may ask the home relay to pin content it published under `/pastes/*`.

`resolve:public` does not imply append-record enumeration. Existing sessions
must be reapproved with an explicit `enumerate:self:` or `enumerate:any:` scope.

### Private Content Capabilities

```text
encrypt:/pastes/*
decrypt:/pastes/*
publish:encrypted:/pastes/*
```

Meaning:

- `encrypt:/pastes/*`: app may ask the daemon to encrypt objects under `/pastes/*`.
- `decrypt:/pastes/*`: app may ask the daemon to decrypt fetched objects under `/pastes/*` when the local identity is an authorized recipient.
- `publish:encrypted:/pastes/*`: app may publish encrypted object bytes under `/pastes/*`.

These are implemented; the encrypted object envelope and its app APIs are
specified in doc 16.

A `share:<path>` capability (updating recipient/access metadata) is a planned
future grant. It is not part of the implemented capability grammar and cannot
be approved today.

### Ingress Capabilities

```text
ingress:send
ingress:read
ingress:decide
```

Meaning:

- `ingress:send`: app may send app-level objects to another identity's ingress.
- `ingress:read`: app may list pending ingress items and open accepted ones.
- `ingress:decide`: app may accept or reject pending ingress items.

Ingress capabilities are global to the session identity, not path-scoped.

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

One deliberate exception lives on the legacy surface: `POST /api/v1/ingress`
is the unauthenticated network-facing submission endpoint for recipient
ingress. Delivery into the pending queue is open by design; recipient-side
authority (listing, opening, accepting, rejecting) is capability-checked under
`/app/v1/ingress/*`.

### App API

External app endpoints live under `/app/v1/*` and require a session token,
except the two session bootstrap endpoints. The implemented surface, with the
capability each endpoint requires:

```text
POST /app/v1/sessions/request          (no token)
GET  /app/v1/sessions/{request_id}     (no token; returns the session token
                                        once, when the session becomes active)
GET  /app/v1/session
POST /app/v1/resolve                   resolve:public
POST /app/v1/fetch                     fetch:public
POST /app/v1/publish                   publish:<path>
POST /app/v1/append                    publish:<path>
POST /app/v1/enumerate                 enumerate:self:<path> or enumerate:any:<path>
POST /app/v1/encrypted/publish         encrypt:<path> + publish:encrypted:<path>
POST /app/v1/encrypted/append          encrypt:<path> + publish:encrypted:<path>
POST /app/v1/encrypted/decrypt         decrypt:<path>
POST /app/v1/encrypted/open            decrypt:<path>
POST /app/v1/encrypted/rewrap          decrypt:<path> + encrypt:<path> + publish:encrypted:<path>
GET  /app/v1/published                 inventory:<path>
GET  /app/v1/ingress/pending           ingress:read
POST /app/v1/ingress/send              ingress:send
POST /app/v1/ingress/{id}/accept       ingress:decide
POST /app/v1/ingress/{id}/reject       ingress:decide
POST /app/v1/ingress/{id}/open         ingress:read
POST /app/v1/home-relay/pins           pin:own:<path>
```

Append reuses the `publish:<path>` capability; there is no separate `append:`
grant. Session tokens have the form `jolt_app_<64 hex>` and are stored as a
`blake3` hash, never in plaintext.

Every endpoint checks:

- token exists
- token is active
- token is not expired
- capability allows operation
- path is inside granted scope
- identity matches the session grant where signing is needed

### Admin / Console API

Console/admin endpoints live under `/admin/v1/*`:

```text
GET  /admin/v1/app-requests
POST /admin/v1/app-requests/{request_id}/approve
POST /admin/v1/app-requests/{request_id}/reject
GET  /admin/v1/app-sessions
POST /admin/v1/app-sessions/{session_id}/revoke
GET  /admin/v1/identities
POST /admin/v1/identities
DELETE /admin/v1/identities/{identity}
POST /admin/v1/identities/export
POST /admin/v1/identities/import
POST /admin/v1/identities/active
GET  /admin/v1/device-authority
POST /admin/v1/device-authority/devices
POST /admin/v1/device-authority/devices/{device_id}/revoke
```

Device enrollment is caller-keyed. The joining installation generates and
retains its signing and encryption private keys, then submits only the matching
public material for approval:

```json
{
  "signing_public_key": [/* 32 Ed25519 public-key bytes */],
  "encryption_keys": [
    {
      "key_id": "enc_x25519_dev_..._v0",
      "suite_family": "x25519-hkdf-chacha20poly1305",
      "public_key": [/* 32 X25519 public-key bytes */],
      "created_at": 1788000000
    }
  ],
  "label": "Joining installation"
}
```

The daemon derives the canonical `dev_...` ID from the signing public key and
returns the complete root-signed `authority_records` chain. It never generates
or retains a private key for the joining installation. A label-only request or
an enrollment without an encryption public key fails with
`device_enrollment_invalid`.

plus network-settings, home-relay, relay status/diagnose, and reachability
endpoints.

App-request and app-session listing, approval, and revocation are scoped to
the daemon's active identity: `GET /admin/v1/app-requests` and
`GET /admin/v1/app-sessions` return entries for the currently active identity,
not a global list across identities.

In v0, admin endpoints are loopback-only, enforced by middleware that rejects
non-local requests. The Console is served by the daemon on localhost and uses
these admin endpoints without a separate auth mechanism. Before remote admin
access exists, this must remain localhost-first.

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

An app session is pinned to one identity, and in the implemented v0 that
identity must be the daemon's active identity.

At approval time, any write-authority grant (`publish:`, `inventory:`,
`pin:own:`, `encrypt:`, `decrypt:`) is refused unless the session's identity
is the daemon's active identity. At request time, write, encrypted, and
ingress calls re-check that the session identity still matches the active
identity.

Consequences:

```text
changing the daemon's active identity breaks existing write sessions (403)
apps must request a new session after an identity switch
only resolve:public / fetch:public sessions are identity-independent
```

Concurrent write sessions against different local identities are not
supported in v0. The original design goal, sessions that keep their granted
identity independently of a mutable global current identity, remains the
long-term direction; the multi-writer identity and device model (doc 20) is
the path there.

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

## Non-Goals

- WASM runtime design.
- App marketplace.
- Remote admin access.
- Payment or entitlements.
- Perfect installed-app identity.

Encrypted content and device-key delegation were non-goals of the original
card and have since been implemented; see doc 16 (encrypted object envelope)
and doc 20 (multi-writer identity and devices).

## Implementation Order

All delivered:

1. Session store and approval API.
2. Capability-checked `/app/v1` endpoints.
3. Jolt Console UI for pending requests and active sessions.
4. Pastey moved to app sessions.
5. Encrypted objects (doc 16), append records and enumeration, and recipient
   ingress added behind the same capability model.

## Open Questions

- v0 supports persistent grants plus explicit revoke first. `Once` and
  `Until Quit` can be added later because the data model already has expiry.
- v0 app session tokens are bearer tokens. `app_origin` remains display and
  audit metadata, not cryptographic proof of app identity.
- Console/admin endpoints remain localhost-first. Binding admin APIs away from
  localhost requires a separate admin-channel design.
- Path-scoped capabilities keep operation separation: `publish`, `inventory`,
  `pin:own`, `encrypt`, and `decrypt` are distinct grants. A future `share`
  grant (recipient/access mutation) should be distinct too.
- Capability names remain strings on the wire for v0, but the daemon parses
  them into strict internal capability values before approval and enforcement.
  Approval may grant exactly what the app requested or a narrower path scope,
  never broader authority.
