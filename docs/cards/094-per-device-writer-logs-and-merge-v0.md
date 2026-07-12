# 094: Per-Device Writer Logs and Deterministic Merge v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** Done
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

- [x] Two authorized devices can publish independent identity state while both
      are online.
- [x] `.jolt` resolution produces the same merged result regardless of device
      log discovery order. (Deterministic merge is order-independent both within
      the merge engine and across remote sync order; see
      `two_device_remote_identity_merges_deterministically_regardless_of_sync_order`.)
- [x] Concurrent append-style app records from different devices can coexist,
      including after remote sync.
- [x] Concurrent singleton path updates resolve deterministically.
- [x] Conflict history is inspectable enough for diagnostics (losing singleton
      and rejected device entries are retained, including for revoked devices
      observed after sync).
- [x] Writes from revoked devices are ignored after revocation, including after
      remote sync.
- [x] Tests cover two-device concurrent publishes and deterministic merge, plus
      remote-identity sync becoming enumerable, deterministic two-device remote
      merge, revoked-device exclusion after sync, and provider retry on failure.

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
    append publish and prefix enumeration over the app API.
- Added live remote-identity device-writer sync (this is the piece that was
  previously pending):
  - new `/jolt/device-writer/1.0.0` request/response protocol
    (`DeviceWriterSyncRequest { identity }` ->
    `DeviceWriterSyncResponse { authority_records, device_logs }`); the responder
    serves whatever device-writer state it has cached, and the requester
    re-verifies and re-merges locally so a hostile response cannot poison the
    cache;
  - discovery reuses the existing `jolt:update-log:<identity>` DHT/relay provider
    key, so device-writer sync rides the same provider-discovery path as the
    legacy update-log resolve. Append-only publishes now announce that provider
    key too (they never touch the update log, so they would otherwise be
    undiscoverable);
  - `store_verified_device_writer_logs` now accumulates per-device logs keyed by
    device id and keeps the highest-sequence verified authority chain, so device
    logs discovered from different providers or in any order converge on the same
    deterministic merged view, and a later sync carrying a revocation is honoured;
  - `EnumerateAppendRecords` for a remote identity with no cached state discovers
    providers, fetches and merges the identity's authorized device-writer logs,
    then answers from live merged state (an empty list when no remote state could
    be synced). `Resolve` of a remote identity opportunistically warms the
    device-writer cache in the background while the legacy update-log path answers
    the request;
  - device-writer sync peeks (does not consume) the shared provider pool so it
    never steals a provider the legacy resolve path expects; failed providers are
    retried, and a sync timeout safety net guarantees parked waiters are answered.
- Added append enumeration refresh and local device-writer persistence:
  - non-local `EnumerateAppendRecords` now attempts a live device-writer refresh
    even when cached state already exists, then falls back to the cached answer
    if no provider is reachable;
  - local device-writer logs are persisted through
    `ContentStore::save_device_writer_log` / `load_device_writer_log`;
  - `NetworkNode` rebuilds the local device-writer state on startup, so local
    append records survive daemon restart and later appends continue the
    existing per-device chain;
  - this fixed the Spoke-shaped case where a peer had already synced an author's
    post, then missed a later accepted-reply reference because enumeration read
    only from a stale cache.
- Still out of scope / deferred: durable storage for every remote identity's
  synced device-writer cache (remote state can be rebuilt by sync); periodic
  background re-sync independent of explicit enumerate/resolve calls; and
  per-device-log content addressing (device logs are served inline in the sync
  response rather than as pinnable CID snapshots).

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
- Red first for live remote-identity device-writer sync:
  - `cargo test -p jolt-network remote_append_records_become_enumerable_after_device_writer_sync`
    failed before the device-writer sync protocol, discovery wiring, and the
    `EnumerateAppendRecords` remote-sync path existed.
  - `cargo test -p jolt-network two_device_remote_identity_merges_deterministically_regardless_of_sync_order`
    failed before `store_verified_device_writer_logs` accumulated per-device logs
    across syncs.
  - `cargo test -p jolt-network revoked_device_append_records_are_excluded_after_sync`
    failed before remote sync verified/merged authority chains carrying revocations.
- Green for live remote-identity device-writer sync:
  - `cargo test -p jolt-network --lib device_writer` (device-writer sync unit tests)
  - `cargo test -p jolt-network --lib` (whole network lib suite)
  - `cargo test -p jolt-core`
  - `./scripts/test-local.sh`
- Red first for append enumeration refresh and local device-writer persistence:
  - `cargo test -p jolt-network remote_enumerate_refreshes_already_cached_device_writer_state`
    failed while non-local enumeration short-circuited from any existing cached
    state.
  - `cargo test -p jolt-network local_append_records_survive_node_restart`
    failed while local append records lived only in memory.
  - `cargo test -p jolt-network local_appends_continue_persisted_device_log_after_restart`
    failed while a post-restart append could not continue the persisted
    per-device chain.
- Green for append enumeration refresh and local device-writer persistence:
  - `cargo test -p jolt-network remote_enumerate_refreshes_already_cached_device_writer_state`
  - `cargo test -p jolt-network local_append_records_survive_node_restart`
  - `cargo test -p jolt-network local_appends_continue_persisted_device_log_after_restart`
  - Live three-daemon Bob/Alice/Carol verification: after a peer synced an
    author's post, the author appended an accepted-reply ref, and the peer's
    re-enumeration surfaced it; append records survived two daemon restarts and
    re-synced to peers.
- Known pre-existing flakiness (reproduced with this slice stashed, so unrelated):
  the network-dependent `two_nodes_dht_provider_announce_and_fetch`,
  `test_fetch_endpoint_distinguishes_unresolved_jolt_address`, and
  `test_resolve_endpoint_reports_no_update_log_provider_candidates` fail in this
  sandbox because no bootstrap relays / DHT providers are reachable.

Release hardening for append enumeration capabilities:

- Replaced the implicit `resolve:public` enumeration authority with explicit
  `enumerate:self:<path>` and `enumerate:any:<path>` grants.
- `self` binds the requested identity to the app session; `any` permits the
  cross-identity reads needed by social apps while remaining path-scoped.
- Existing sessions require reapproval rather than silently inheriting broad
  enumeration authority.
- Red: `cargo test -p jolt-server test_app_can_append_and_enumerate_records_by_prefix --test api_integration -- --nocapture`
  failed because the new capability vocabulary was not grantable.
- Green: focused API tests cover self-identity/path enforcement, rejection of
  `resolve:public` alone, and explicitly scoped cross-identity enumeration.
