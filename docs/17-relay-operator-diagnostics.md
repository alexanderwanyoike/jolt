# Relay Operator Diagnostics

## Purpose

Server-facing relays are headless infrastructure. Operators should be able to
debug them over SSH, logs, and admin-only HTTP APIs without running Jolt
Console or exposing a browser dashboard.

This is separate from local desktop diagnostics:

| Surface | Audience | Shape |
|---|---|---|
| Jolt Console Diagnostics | Local user running a desktop daemon | Native UI over the local daemon API |
| Relay Operator Diagnostics | Person operating a VPS/home/community relay | CLI, logs, admin-only APIs, counters |

The relay operator surface must stay protocol/operator focused. It must not
introduce app concepts such as inboxes, contacts, feeds, Pastey, or profiles.

## V0 Operator Questions

Relay diagnostics v0 should answer:

- Is this node intentionally running as a bootstrap/discovery relay?
- What peer ID, identity address, relay record, and listen addresses describe
  this relay?
- Which peers and known relays is it connected to?
- Is bootstrap healthy, degraded, or empty?
- How many relay records and identity-head hints are known?
- Is identity/provider query forwarding happening?
- Is the relay accepting, storing, and serving owner-authorized pins?
- Why did a `.jolt` identity/provider lookup fail from this relay's view?

## CLI Surface

Keep the CLI usable over SSH and scriptable. The v0 commands should call the
local daemon API unless the command explicitly accepts a remote admin URL.

Implemented commands:

```text
jolt relay status [--json]
jolt relay diagnose identity <identity> [--json]
```

### `jolt relay status`

Human output should be compact and stable:

```text
Relay: enabled
Peer: 12D3...
Identity: abc123.jolt
API: 127.0.0.1:9862
Listen: /ip4/0.0.0.0/tcp/4001
Bootstrap: connected (2 connected / 2 configured)
Known relays: 3
Connected peers: 8 (direct 8 / relayed 0)
Pins: 14 items / 42.1 MB
Identity-head hints: 37 identities
Last error: none
```

Note: the field labels above are illustrative and differ slightly from the shipped output, which uses labels such as `Peer ID:`, `Jolt:`, `API port:`, `Peers: N connected (X direct / Y relayed)`, `Bootstrap: connected (a connected / b effective / c configured)`, plus `Cache:`, `Relay record:`, `Home relay:`, and `Listening:` lines, and does not include `Identity-head hints` or `Last error` lines.

`--json` should return the same data as typed JSON for scripts and future
operator tooling.

### `jolt relay diagnose identity <identity>`

This is the most important troubleshooting command. It should trace the relay's
answer to:

```text
Who can provide jolt:update-log:<identity>?
```

V0 output should show:

- local verified update-log cache hit/miss;
- local identity-head hint hit/miss and expiry;
- local provider candidates;
- known relay forwarding attempts;
- forwarding responses by relay peer ID;
- final structured outcome using existing failure codes where possible.

Example:

```text
Identity: abc123
Provider key: jolt:update-log:abc123
Local cache: miss
Identity-head hint: miss
Forwarded queries:
  12D3RelayA: no candidates
  12D3RelayB: 1 candidate
Outcome: provider candidates found
Candidates:
  12D3Provider /ip4/...
```

For a failure:

```text
Outcome: identity_provider_not_found
Relay mesh reachable, but no known relay reported an update-log provider for this identity.
```

## Admin HTTP API Surface

The existing `/api/v1/status`, `/api/v1/peers`, `/api/v1/cache/*`, and
`/api/v1/relay/pins/*` routes are useful but not an operator contract. V0
should add relay-specific admin endpoints rather than teaching operators to
scrape product/user APIs.

Implemented endpoints:

```text
GET  /admin/v1/relay/status
POST /admin/v1/relay/diagnose/identity
```

The endpoints should be read-only for v0. Mutating relay operations already
exist for pins and config elsewhere and should not be duplicated until there is
a clear operator workflow.

### Security Constraints

Default binding must remain localhost-only for admin diagnostics.

If the daemon is started with `--api-bind 0.0.0.0`, admin diagnostics are not
safe to expose as unauthenticated public HTTP. Before remote admin diagnostics
are documented as supported, Jolt needs at least one of:

- explicit admin token authentication;
- reverse-proxy guidance with TLS and authentication;
- SSH tunnel as the recommended access pattern.

For v0, the supported remote workflow is:

```text
ssh relay-host
jolt relay status
```

or:

```text
ssh -L 9862:127.0.0.1:9862 relay-host
curl http://127.0.0.1:9862/admin/v1/relay/status
```

Do not document public unauthenticated admin endpoints as acceptable.

## Failure Outcomes

Relay diagnostics should reuse existing structured failure vocabulary where it
fits:

- `no_bootstrap_relays`
- `relay_unreachable`
- `relay_mesh_empty`
- `identity_provider_not_found`
- `identity_head_invalid`
- `content_provider_not_found`
- `content_fetch_failed`
- `content_hash_mismatch`

Relay-specific diagnostics may add operator-only reasons, such as:

- `relay_mode_disabled`
- `admin_diagnostics_unavailable`
- `relay_record_expired`
- `relay_forwarding_timeout`
- `pin_store_unavailable`

## Implementation Slices

1. **Relay CLI/Admin Status v0** (delivered)
   Add `jolt relay status --json` and `GET /admin/v1/relay/status` using
   existing daemon status, peer, cache, relay-record, and pin information.

2. **Relay Diagnose Identity v0** (delivered)
   Add `jolt relay diagnose identity <identity>` and
   `POST /admin/v1/relay/diagnose/identity` to trace local cache/hint/provider
   lookup and relay forwarding outcomes.

3. **Relay Structured Logs v0** (not started)
   Normalize tracing event names and fields for bootstrap, relay exchange,
   identity provider forwarding, identity-head gossip, and pins.

4. **Relay Metrics v0** (not started)
   Add lightweight relay counters and expose them as JSON at
   `GET /admin/v1/relay/metrics`.

## Non-Goals

- Browser dashboard for headless relays.
- Public unauthenticated remote admin APIs.
- Full Prometheus/OpenTelemetry integration.
- Relay scoring, payments, storage markets, or operator reputation.
- Application-level diagnostics.
