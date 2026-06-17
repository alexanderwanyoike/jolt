# 094: Per-Device Writer Logs and Deterministic Merge v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** In progress
**Blocked by:** 091, 093

## Why

True multi-writer identity cannot rely on one global identity log sequence
shared by every device. That recreates a single-writer bottleneck and makes
offline/device races brittle.

Each authorized device should be able to publish its own signed writer log. The
resolved user identity state is then a deterministic materialized view over
authorized, non-revoked device logs.

## What to Build

Implement the minimal true multi-writer identity state path:

- publish a per-device signed writer log;
- discover writer logs from authorized device records;
- verify log entries against device authorization state;
- merge multiple device logs deterministically;
- expose merged identity path state through existing resolve APIs;
- preserve losing/conflicting records for diagnostics;
- make same-identity multi-device reads deterministic.

## Acceptance Criteria

- [ ] Two authorized devices can publish independent identity state while both
      are online.
- [ ] `.jolt` resolution produces the same merged result regardless of device
      log discovery order.
- [ ] Concurrent append-style app records from different devices can coexist.
- [ ] Concurrent singleton path updates resolve deterministically.
- [ ] Conflict history is inspectable enough for diagnostics.
- [ ] Writes from revoked devices are ignored after revocation.
- [ ] Tests cover two-device concurrent publishes and deterministic merge.

## Non-Goals

- Generic CRDT support for arbitrary app payloads.
- Automatic merging of app-specific documents.
- Global total ordering across all Jolt identities.
- Protocol-level knowledge of profiles, posts, feeds, or pastes.

## Notes

Singleton paths such as `/profile` need deterministic winner selection.
Append-style app records should normally coexist. Apps remain responsible for
interpreting their own object schemas.

## Implementation Notes

- Added `jolt-core::device_writer_log` primitives for signed per-device writer
  log entries.
- Added deterministic merge over verified identity authority state:
  - singleton path winner selection is stable across discovery order;
  - losing singleton entries are retained as conflict diagnostics;
  - append-style records from multiple devices coexist in deterministic order;
  - revoked-device entries after the accepted device sequence are ignored and
    preserved as rejected-entry diagnostics.
- Added merged-state `.jolt` path resolution for singleton paths.
- Device writer entries require canonical signed paths with a leading `/`;
  user-facing address normalization is not applied to signed writer records.
- Added local verification hardening for hostile inputs:
  - wrong device signatures are rejected;
  - broken per-device hash chains are rejected;
  - out-of-order per-device sequences are rejected;
  - unknown-device entries are ignored and retained as rejected diagnostics.
- Added daemon/server resolve integration for verified device-writer state:
  - `NetworkNode` can cache verified authority records plus per-device writer
    logs as merged device-writer state;
  - `DaemonCommand` and `DaemonHandle` can store verified device-writer logs;
  - daemon `Resolve` prefers the merged device-writer cache before legacy
    single-writer update logs;
  - `/api/v1/resolve` returns `source: "device_writer_cache"` when resolving
    from that state.
- Added local daemon publishing into device-writer state:
  - normal `Publish { path: ... }` creates/appends a local device-writer log
    entry for the daemon's legacy root device;
  - path publish refreshes the merged device-writer cache immediately;
  - `/api/v1/publish` followed by `/api/v1/resolve` now resolves from
    `source: "device_writer_cache"`.
- Added the append-record app-API surface (Spoke card J1):
  - `MergedDeviceIdentityState::append_records_under(prefix)` enumerates the
    merge engine's `append_records` map by path prefix in deterministic order;
  - `publish_file_appending_path` publishes a device-writer Append entry and
    never writes the last-writer-wins update log (append records must coexist);
  - `DaemonCommand::PublishAppend` / `DaemonHandle::publish_append` and
    `DaemonCommand::EnumerateAppendRecords` / `DaemonHandle::enumerate_append_records`;
  - `POST /app/v1/append` (capability `publish:<path>`, local identity only) and
    `POST /app/v1/enumerate` (capability `resolve:public`, any identity) expose
    append publish and prefix enumeration over the app API. Enumeration reads
    cached merged device-writer state; remote-identity device-writer sync stays
    out of scope (still pending below).
- This work does not yet wire device writer logs into provider discovery,
  network sync, or persisted store format.

## Verification

- Red first:
  - `cargo test -p jolt-core --test device_writer_log -- --nocapture` failed on
    the missing device-writer public API.
  - `cargo test -p jolt-core resolves_jolt_address_from_merged_device_state --test device_writer_log -- --nocapture`
    failed on the missing merged-state resolver.
  - `cargo test -p jolt-core preserves_append_records_from_multiple_devices_in_deterministic_order --test device_writer_log -- --nocapture`
    failed on the missing append-record constructor.
  - `cargo test -p jolt-core ignores_revoked_device_entries_after_accepted_sequence --test device_writer_log -- --nocapture`
    failed because rejected diagnostics did not preserve the rejected CID.
  - `cargo test -p jolt-core rejects_malformed_device_writer_paths --test device_writer_log -- --nocapture`
    failed until device writer entries required canonical paths.
- Green:
  - `cargo test -p jolt-core rejects_malformed_device_writer_paths --test device_writer_log -- --nocapture`
  - `cargo test -p jolt-core --test device_writer_log -- --nocapture` covers 9
    device-writer tests, including hostile-input verification.
  - `cargo test -p jolt-core`
  - `./scripts/test-local.sh`
- Red first for daemon/server integration:
  - `cargo test -p jolt-network daemon_resolution_uses_cached_device_writer_state -- --nocapture`
    failed because the daemon had no `store_verified_device_writer_logs` cache
    API.
  - `cargo test -p jolt-network daemon_store_device_writer_logs_command_updates_resolve_cache -- --nocapture`
    failed because `DaemonCommand::StoreDeviceWriterLogs` did not exist.
- Green for daemon/server integration:
  - `cargo test -p jolt-network device_writer -- --nocapture`
  - `cargo test -p jolt-server test_resolve_endpoint_uses_verified_device_writer_cache -- --nocapture`
- Red first for daemon publish integration:
  - `cargo test -p jolt-network daemon_publish_path_populates_device_writer_resolve_cache -- --nocapture`
    failed because normal daemon path publish still resolved through the legacy
    update-log cache.
- Green for daemon publish integration:
  - `cargo test -p jolt-network daemon_publish_path_populates_device_writer_resolve_cache -- --nocapture`
  - `cargo test -p jolt-server test_publish_endpoint_can_bind_content_to_jolt_path -- --nocapture`
  - `cargo test -p jolt-network`
  - `cargo test -p jolt-server`
  - `./scripts/test-local.sh`
- Red first for the append-record app-API slice (Spoke card J1):
  - `cargo test -p jolt-core enumerates_append_records_under_a_path_prefix --test device_writer_log`
    failed on the missing `append_records_under` helper.
  - `cargo test -p jolt-network daemon_append_publish_records_coexist_under_prefix`
    failed on the missing `DaemonCommand::PublishAppend`.
  - `cargo test -p jolt-server test_app_can_append_and_enumerate_records_by_prefix`
    failed on the missing `/app/v1/append` and `/app/v1/enumerate` routes.
- Green for the append-record app-API slice:
  - `cargo test -p jolt-core --test device_writer_log` (10 tests)
  - `cargo test -p jolt-network --lib` (56 tests)
  - `cargo test -p jolt-server --test api_integration test_app_can_append_and_enumerate_records_by_prefix`
- Known pre-existing flakiness (reproduced with this slice stashed, so unrelated):
  the network-dependent `two_nodes_dht_provider_announce_and_fetch`,
  `test_fetch_endpoint_distinguishes_unresolved_jolt_address`, and
  `test_resolve_endpoint_reports_no_update_log_provider_candidates` fail in this
  sandbox because no bootstrap relays / DHT providers are reachable.
