# Changelog

## 0.3.0

### Added

- Cache-first typed Data Subscriptions through
  `Subscription.create(resource.for(identity))`.
- Cancellable Materialized View Change Streams through
  `subscription.changes({ cursor? })`, including typed snapshot, record,
  freshness, resynchronization, cancellation, and revocation events.
- Automatic App API compatibility requirements for `data.subscriptions` and
  `data.change-streams` when an App Definition declares subscribable remote
  Collections.
- Matching deterministic `App.testWorld()` behavior and low-level fake/client
  transport support for Data Subscriptions.
- An advanced `createDataClient(...)` host seam for applications that already
  own an approved session and pass a client to `App.connect`.
- A compile-checked beginner Chirp guide based on Schema Classes,
  `App.create`, and generated Resource interfaces.

### Changed

- `EnumeratedRecord.createdAt` now correctly uses `number`, matching the
  daemon's unsigned integer wire value. Code that treated it as a string must
  be updated.
- Advanced custom clients passed to `App.connect({ client })` must implement
  the Data Subscription operations included in `DataSdkClient`. Ordinary
  applications using the generated host connection do not implement these
  operations themselves.

### Notes

- Raw Data Subscription operations remain an advanced/internal connection
  seam and are intentionally absent from the ordinary `createJoltClient`
  application surface.
- Change Streams are local bounded Materialized View streams. They do not add
  a peer-to-peer push protocol.
