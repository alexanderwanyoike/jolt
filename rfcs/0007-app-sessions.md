# Jolt Request for Comments 0007

## Capability-Scoped Local App Sessions

```text
Jolt Project                                                JOLT-RFC-0007
Request for Comments: 0007                                  August 2026
Category: Experimental
Status: Internet-Draft
Updates: none
Obsoletes: none
```

### Status of This Memo

This document specifies an experimental local authorization contract between
Jolt applications and a Jolt daemon. It is not an IETF publication.
Distribution of this memo is unlimited.

The v0 session store, capability parser, and HTTP surface are implemented. This
memo remains a draft because app identity and device binding are transitional.

### Abstract

This document defines capability-scoped sessions for untrusted local Jolt
applications. An app requests authority for one Jolt identity. The trusted Jolt
Console approves an equal or narrower set of capabilities. The daemon returns a
bearer token and checks its status, expiry, identity, operation, and path scope
before performing any action involving keys, signed state, storage, encryption,
ingress, or the network.

Applications never receive long-term identity or device private keys. Localhost
reachability alone grants no app authority.

### Table of Contents

1. Introduction
2. Conventions and Requirements Language
3. Scope
4. Trust Classes and Terminology
5. Session Data Model
6. Session Lifecycle
7. Capability Grammar
8. Path and Identity Scope
9. HTTP API Contract
10. Request Authorization
11. Token Generation and Storage
12. Identity and Device Binding
13. Revocation and Expiry
14. Error Conditions
15. Compatibility and Versioning
16. Security Considerations
17. Privacy Considerations
18. IANA Considerations
19. Implementation Status
20. References
Appendix A. Spoke Grant Example

## 1. Introduction

The Jolt daemon owns identities, signing keys, encryption keys, signed state,
content storage, cache policy, and network access. A desktop or web application
that can reach the daemon must not inherit all that authority merely because it
runs on the same machine.

Jolt therefore separates three local trust classes:

- Console is the privileged, loopback-only control surface;
- normal apps are untrusted clients with explicit grants;
- network peers and relays are untrusted data and transport sources.

Session checks occur before daemon operations. The protocol networking layer
does not know app names, UI concepts, or session prompts.

## 2. Conventions and Requirements Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**,
and **MAY** are interpreted as BCP 14 [RFC2119] [RFC8174]. HTTP method and
status semantics follow [RFC9110]. JSON member names use `snake_case`.

## 3. Scope

This memo defines:

- app session requests, approval, rejection, expiry, and revocation;
- identity/device/app binding;
- bearer-token creation and hashed persistence;
- the v0 capability vocabulary and strict path scopes;
- app, admin, and legacy API separation;
- operation-by-operation authorization checks.

This memo does not define:

- app installation, app stores, or cryptographic app packages;
- browser-origin authentication;
- remote Console administration;
- network peer authentication beyond the underlying Jolt protocol;
- application schemas or application-specific permissions.

## 4. Trust Classes and Terminology

**App session**
: A local grant binding an app ID, user identity, local device, capabilities,
  status, and optional expiry to a bearer token.

**App ID**
: A caller-declared local identifier. In v0 it is not cryptographic proof of
  an installed application.

**Requested capability**
: Authority proposed by the app for user review.

**Granted capability**
: Authority approved by Console. It MUST be equal to or narrower than a
  requested capability.

**Admin API**
: The privileged loopback-only `/admin/v1/*` surface used by Console.

**App API**
: The capability-checked `/app/v1/*` surface used by normal apps.

## 5. Session Data Model

The persistent v0 record contains:

```text
AppSessionRecord {
  request_id: string,
  session_id: optional string,
  app_id: string,
  app_name: string,
  app_origin: optional string,
  requested_identity: optional IdentityId,
  identity: optional IdentityId,
  device_id: optional string,
  requested_capabilities: [string],
  granted_capabilities: [string],
  status: pending | active | rejected | revoked | expired,
  created_at: uint64,
  approved_at: optional uint64,
  rejected_at: optional uint64,
  revoked_at: optional uint64,
  expires_at: optional uint64,
  last_used_at: optional uint64,
  token_hash: optional hex(BLAKE3-256(token))
}
```

Secret bearer tokens MUST NOT appear in list or view representations. App name,
origin, and app ID are display and policy inputs, not proof of code identity.

## 6. Session Lifecycle

The state machine is:

```text
pending -> active -> revoked
        -> rejected
active  -> expired
```

An app creates a pending request. If no identity is supplied, the daemon binds
the request to the currently selected local identity at request time.

Console may reject a pending request or approve it for one identity, an equal
or narrower capability set, and an optional expiry. Approval creates a session
ID and token and moves directly to `active`; there is no separate `approved`
state.

Only `active`, unexpired sessions authenticate. Rejected, revoked, and expired
records remain useful for identity-scoped history and diagnostics.

## 7. Capability Grammar

The v0 grantable grammar is:

```text
resolve:public
fetch:public
ingress:send
ingress:read
ingress:decide
enumerate:self:<path-scope>
enumerate:any:<path-scope>
publish:<path-scope>
publish:encrypted:<path-scope>
inventory:<path-scope>
pin:own:<path-scope>
encrypt:<path-scope>
decrypt:<path-scope>
```

No other capability string is grantable. In particular, apps cannot receive
private-key export, identity deletion, root/device rotation, arbitrary signing,
grant approval, relay administration, security settings, or store-wipe
authority.

Capability meanings are:

- `resolve:public`: resolve public `.jolt` addresses;
- `fetch:public`: fetch public CID-addressed content;
- `enumerate:self`: enumerate append records only for the session identity;
- `enumerate:any`: enumerate any identity within the granted path namespace;
- `publish`: publish singleton or append state under the path scope;
- `publish:encrypted`: publish daemon-created encrypted envelopes;
- `inventory`: list locally published items under the path scope;
- `pin:own`: ask the configured home relay to pin session-owned publications;
- `encrypt` and `decrypt`: use daemon-owned cryptographic authority under the
  path scope;
- ingress capabilities send, inspect/open, or decide pending recipient ingress.

## 8. Path and Identity Scope

A path scope is either an exact absolute path or one trailing wildcard prefix:

```text
/spoke/profile
/spoke/*
```

Scopes MUST begin with `/`, MUST NOT be `/`, contain whitespace, query,
fragment, `.` or `..` segments, and MAY contain at most one `*` only as the
final `/*` suffix.

`/spoke/*` contains `/spoke/posts/1` but not `/spoke`, `/spoke-evil/x`, or
`/profile`. An approved scope must be contained by one requested scope of the
same action. A requested prefix can be narrowed to an exact path or a narrower
prefix; an exact request can only grant that exact path.

A requested `enumerate:any` may be narrowed to `enumerate:self`. A requested
`enumerate:self` MUST NOT be broadened to `enumerate:any`.

Signing, inventory, pinning, encryption, and decryption operations use the
session identity. An app MUST NOT substitute a different local identity in an
operation body.

## 9. HTTP API Contract

### 9.1 Session Bootstrap

The following routes do not require an existing bearer token:

```text
POST /app/v1/sessions/request
GET  /app/v1/sessions/{request_id}
```

The request body contains app ID, app name, optional origin, requested identity,
and requested capabilities. Status polling returns state and, while locally
available after approval, the newly issued token. Apps SHOULD store the token
securely when first received.

### 9.2 Capability-Checked App API

```text
GET  /app/v1/session                     active token
POST /app/v1/resolve                     resolve:public
POST /app/v1/fetch                       fetch:public
POST /app/v1/publish                     publish:<path>
POST /app/v1/append                      publish:<path>
POST /app/v1/enumerate                   enumerate:self|any:<path>
POST /app/v1/encrypted/publish           encrypt + publish:encrypted
POST /app/v1/encrypted/append            encrypt + publish:encrypted
POST /app/v1/encrypted/decrypt           decrypt:<path>
POST /app/v1/encrypted/open              decrypt:<path>
POST /app/v1/encrypted/rewrap            decrypt + encrypt + publish:encrypted
GET  /app/v1/published                   inventory:<path>
GET  /app/v1/ingress/pending             ingress:read
POST /app/v1/ingress/send                ingress:send
POST /app/v1/ingress/{id}/accept         ingress:decide
POST /app/v1/ingress/{id}/reject         ingress:decide
POST /app/v1/ingress/{id}/open           ingress:read
POST /app/v1/home-relay/pins             pin:own:<path>
```

When several capabilities are listed, every listed authority is required.

### 9.3 Admin API

Console lists, approves, rejects, and revokes sessions through
`/admin/v1/app-requests` and `/admin/v1/app-sessions`. Listings and revocation
are scoped to the currently selected local identity. Admin routes MUST remain
loopback-only unless a future authenticated remote-admin RFC replaces that
assumption.

## 10. Request Authorization

For every protected call, the daemon MUST:

1. parse exactly one `Authorization: Bearer <token>` value;
2. hash the token and find the matching persistent session record;
3. require status `active`;
4. expire the record if `expires_at <= now`;
5. parse the required operation capability;
6. require a matching granted action and containing scope;
7. enforce `self` identity where applicable;
8. require the session identity to be locally signable for local-authority
   operations;
9. update `last_used_at` only after successful token authentication;
10. perform the daemon operation without returning private key material.

An endpoint MUST NOT rely on UI behavior or app-supplied labels as an
authorization check.

## 11. Token Generation and Storage

Request and session IDs contain 16 random octets rendered as lowercase hex with
`req_` and `sess_` prefixes. Tokens contain 32 random octets rendered as:

```text
jolt_app_<64 lowercase hexadecimal characters>
```

Randomness MUST come from a cryptographically secure operating-system source.
The persistent store contains BLAKE3-256 of the full token string, never the
plaintext token. Token comparison SHOULD avoid avoidable timing leakage.

The current daemon retains newly issued plaintext tokens only in process memory
for status polling. Restarting does not invalidate a token already saved by an
app, because its hash remains persistent, but may prevent the app from
retrieving an uncollected plaintext token.

## 12. Identity and Device Binding

Every active session is bound to exactly one identity and one local device
identifier. The same app requires distinct grants for different identities.

Capabilities involving signing, local inventory, owner pinning, encryption, or
decryption may be approved only for an identity the daemon can currently act
for. Public resolve/fetch authority does not imply local signing authority.

Device revocation MUST revoke active sessions bound to that device for future
writes. The v0 implementation binds sessions to `dev_legacy_root`; binding to
separately generated local device writers remains incomplete and is called out
in Section 19.

## 13. Revocation and Expiry

Console revocation changes a matching active session to `revoked` and records
the time. It applies only to the selected identity's session.

Device revocation invokes session revocation for records with the same device
ID. Expiry is evaluated during token authentication; an expired token changes
the persistent state to `expired` and is rejected.

Revocation stops later daemon authorization. It cannot undo already published
signed state, remove cached content, or erase plaintext previously returned to
an app.

## 14. Error Conditions

The HTTP surface SHOULD distinguish missing/invalid bearer token (`401`),
unknown request/session (`404`), malformed or ungrantable request (`400`),
identity not locally signable (`400`), capability/path denial (`403`), and
daemon/network failure (`5xx`).

Authentication errors SHOULD avoid revealing whether a guessed token hash
exists in another non-active state.

## 15. Compatibility and Versioning

Adding a new capability does not grant it to existing sessions. Apps MUST
request it and users MUST approve it. Broadening the meaning of an existing
capability is a compatibility and security change requiring RFC review.

Existing sessions do not silently inherit newly introduced enumeration or
encryption authority. Path parsing changes MUST preserve strict containment or
require reapproval.

## 16. Security Considerations

Bearer tokens are secrets with the authority of their grant. Apps and daemon
logs MUST avoid exposing them. Local malware running as the user remains a
threat; localhost is not a complete sandbox boundary.

The v0 app ID and origin are self-declared. Users SHOULD evaluate the visible
capability request, not treat the app name as authenticated provenance.

Admin routes have no independent bearer authentication and therefore MUST be
unreachable from non-loopback clients. Public ingress is intentionally separate
and requires rate, size, and abuse controls.

## 17. Privacy Considerations

Session records reveal installed/used app names, origins, identity association,
capability scopes, and activity timing to anyone who can read the local store or
admin API. The store SHOULD use restrictive filesystem permissions.

Capability scoping reduces accidental cross-app and cross-identity disclosure
but does not prevent an authorized app from retaining data it legitimately
received.

## 18. IANA Considerations

This document requests no IANA actions. HTTP paths, token prefixes, and
capability strings are project-local experimental identifiers.

## 19. Implementation Status

The persistent session state, lifecycle, identity-scoped Console listings,
random bearer tokens, hashed storage, expiry, strict path capability parser,
endpoint checks, encryption/ingress capabilities, and selected-identity
isolation are implemented in `jolt-server`; Spoke exercises the app boundary.

Known gaps include self-declared app identity, no browser-origin proof, no
remote-admin authentication, in-memory delivery of newly issued tokens, and
transitional `dev_legacy_root` binding. In particular, generated device IDs and
session device IDs are not yet universally aligned, so device revocation must
be audited before this memo advances beyond Draft. The identity-scoped revoke
operation does not require the session to be active first, so it also rewrites
rejected, expired, or already revoked records to `Revoked`. Stored token hashes
are not compared in constant time during bearer-token lookup. Work was tracked
by cards 042, 052, and 095 and architecture document 15.

## 20. References

### 20.1 Normative References

- JOLT-RFC-0001, “Signed Path Records and Resolution.”
- JOLT-RFC-0002, “Device Authorization and Revocation.”
- JOLT-RFC-0004, “Encrypted Objects and Private Device Access.”
- [RFC9110] Fielding, R., et al., “HTTP Semantics.”
- [RFC2119] Bradner, S., RFC 2119.
- [RFC8174] Leiba, B., RFC 8174.

### 20.2 Informative References

- Jolt document 15, “App Boundary and Sessions.”
- Jolt card 095, “Identity-Scoped App Grants v0.”

## Appendix A. Spoke Grant Example

A Spoke session requests public resolve/fetch, `/spoke/*` publish, encrypted
publish, inventory, pinning, encryption/decryption, self/any enumeration, and
recipient ingress capabilities. Console may approve all or a narrower subset
for one selected identity. Spoke receives only a bearer token. Signing and
private keys remain inside the local daemon, and every later request is checked
against that identity and capability set.
