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

Recommended commands:

```text
jolt relay status [--json]
jolt relay peers [--json]
jolt relay records [--json]
jolt relay pins [--json]
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

Recommended endpoints:

```text
GET  /admin/v1/relay/status
GET  /admin/v1/relay/peers
GET  /admin/v1/relay/records
GET  /admin/v1/relay/pins
POST /admin/v1/relay/diagnose/identity
GET  /admin/v1/relay/metrics
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

## Structured Logs

Relay logs should use stable event names and fields so operators can grep them
and future metrics can be derived without parsing prose.

Minimum event families:

| Event | Important Fields |
|---|---|
| `relay.started` | `peer_id`, `identity`, `listen_addrs`, `bootstrap_relay` |
| `relay.bootstrap.connected` | `peer_id`, `multiaddr`, `duration_ms` |
| `relay.bootstrap.failed` | `multiaddr`, `error_code`, `error` |
| `relay.record.published` | `relay_id`, `expires_at`, `capabilities` |
| `relay.record.learned` | `relay_id`, `source_peer`, `expires_at` |
| `relay.record.rejected` | `relay_id`, `source_peer`, `reason` |
| `relay.exchange.completed` | `peer_id`, `records_sent`, `records_received` |
| `identity_provider.query.received` | `identity`, `source_peer` |
| `identity_provider.query.forwarded` | `identity`, `target_relay` |
| `identity_provider.query.result` | `identity`, `candidate_count`, `outcome` |
| `identity_head.gossip.sent` | `target_relay`, `hint_count` |
| `identity_head.gossip.received` | `source_peer`, `accepted`, `rejected` |
| `relay.pin.accepted` | `owner_identity`, `content_id`, `size` |
| `relay.pin.rejected` | `owner_identity`, `content_id`, `reason` |
| `relay.pin.served` | `content_id`, `requester_peer` |

Logs must not include private plaintext content, private keys, session tokens,
or decrypted payloads.

## Counters And Metrics

V0 can expose simple JSON counters through `GET /admin/v1/relay/metrics`.
Prometheus text format can come later without changing the internal metric
names.

Recommended counters/gauges:

```text
jolt_relay_connected_peers
jolt_relay_known_relays
jolt_relay_bootstrap_connected_peers
jolt_relay_pinned_items
jolt_relay_pinned_bytes
jolt_relay_identity_head_hints
jolt_relay_provider_queries_received_total
jolt_relay_provider_queries_forwarded_total
jolt_relay_provider_queries_failed_total{code}
jolt_relay_relay_records_accepted_total
jolt_relay_relay_records_rejected_total{reason}
jolt_relay_pins_accepted_total
jolt_relay_pins_rejected_total{reason}
jolt_relay_content_requests_served_total
```

Counters should be best-effort observability, not consensus state. Restarting a
process may reset in-memory counters in v0.

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

1. **Relay CLI/Admin Status v0**
   Add `jolt relay status --json` and `GET /admin/v1/relay/status` using
   existing daemon status, peer, cache, relay-record, and pin information.

2. **Relay Diagnose Identity v0**
   Add `jolt relay diagnose identity <identity>` and
   `POST /admin/v1/relay/diagnose/identity` to trace local cache/hint/provider
   lookup and relay forwarding outcomes.

3. **Relay Structured Logs v0**
   Normalize tracing event names and fields for bootstrap, relay exchange,
   identity provider forwarding, identity-head gossip, and pins.

4. **Relay Metrics v0**
   Add lightweight relay counters and expose them as JSON at
   `GET /admin/v1/relay/metrics`.

## Non-Goals

- Browser dashboard for headless relays.
- Public unauthenticated remote admin APIs.
- Full Prometheus/OpenTelemetry integration.
- Relay scoring, payments, storage markets, or operator reputation.
- Application-level diagnostics.
