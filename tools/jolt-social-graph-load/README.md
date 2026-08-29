# Jolt social-graph load harness

This tool produces the repeatable follower-scale workload for Cards 110 and
118. It
runs one reader and a small configurable number of provider daemons as real
`NetworkNode` daemon loops. The providers host many deterministic but real
Ed25519 identities, signed authority chains, signed writer logs, paths, CIDs,
and content.

The default `cache_first` path models the Data Subscription flow used by
Spoke:

1. publish posts as stable singleton records;
2. cold-refresh each followed identity's Materialized View and fetch its post
   content;
3. reopen the unchanged timeline from the local view and content cache; and
4. synchronize one new record per followed identity and fetch only that delta.

The retained `legacy_refresh` path reproduces the Card 110 pre-change flow for
comparison:

1. enumerate `/spoke/posts/` append records;
2. resolve the identity's `/profile` singleton; and
3. fetch the resolved profile content and every enumerated post CID.

## Run one workload

From the Jolt repository root:

```bash
cargo run --release -p jolt-social-graph-load -- \
  --timeline-path cache-first \
  --daemons 3 \
  --identities 100 \
  --follows 100 \
  --records-per-identity 1 \
  --publish-rate-per-second 0 \
  --concurrency 32 \
  --json-output /tmp/jolt-followers-100.json
```

The temporary work directory must be empty. When `--workdir` is omitted the
tool creates and removes one automatically. Pass `--keep-workdir` only when
the stores and generated content are needed for diagnosis.

Pass `--timeline-path legacy-refresh` to reproduce the old append/enumerate,
resolve, and full-fetch behavior. Each JSON artifact records the selected
path, and cache-first output uses result schema version 4.

`--one-way-latency-ms`, `--bandwidth-kbps`, and `--loss-percent` shape every
reader/provider TCP link. `--churn-percent` deterministically stops a
percentage of provider daemons after active publishing, verifies cached and
explicit-refresh behavior while they are absent, then restarts the same
identities and stores on the same listen ports. The loss control
drops encrypted TCP chunks and is intentionally a harsh fault injector, not an
IP packet-loss simulator.

`--provider-record-capacity` explicitly raises the in-memory DHT store's local
provided-key limit for high-scale load runs. Zero preserves the production
libp2p default. The matrix uses 50,000 so all three scales exercise timeline
work rather than stopping at the default provider-key ceiling; every artifact
records this override and its limitation.

`--reader-cache-max-bytes` sets only the reader's content-cache budget. A tiny
value makes cache-pressure behavior reproducible without constraining provider
storage. The default is the production 2 GiB budget.

## Run the baseline matrix

```bash
./scripts/run-social-graph-baselines.sh \
  ../jolt-development-docs/experiments/follower-scale-baseline/pre-change
```

The script records 100, 1,000, and 10,000-follow JSON artifacts. Environment
variables tune the shared matrix settings: `JOLT_LOAD_DAEMONS`,
`JOLT_LOAD_TIMELINE_PATH`, `JOLT_LOAD_RECORDS`, `JOLT_LOAD_CONCURRENCY`,
`JOLT_LOAD_PUBLISH_RATE`, `JOLT_LOAD_READER_CACHE_BYTES`,
`JOLT_LOAD_LATENCY_MS`, `JOLT_LOAD_BANDWIDTH_KBPS`, `JOLT_LOAD_LOSS_PERCENT`,
`JOLT_LOAD_CHURN_PERCENT`, `JOLT_LOAD_CHURN_DURATION_MS`, and
`JOLT_LOAD_PROVIDED_KEYS`.

## Result contract

Each JSON artifact contains:

- the complete workload and network configuration plus a deterministic plan
  digest;
- cold, no-change warm, and new-record success-only latency percentiles; the
  cache-first warm phase uses only the Last Verified View and local content
  cache;
- reader restart time and the first post-restart local view, so lost durable
  state is reported rather than hidden by a network refresh;
- sampled status-API latency plus sync-worker queue peaks, overload counters,
  full/delta responses, and received entry/byte totals for every phase;
- successful and failed identity refresh counts with classified failures;
- enumerate-sync, resolve, fetch, provider-announcement, content-announcement,
  and churn counts;
- bytes forwarded in each direction and bytes deliberately dropped;
- aggregate CPU time, RSS, and virtual memory, plus reader-only cache and
  on-disk store growth; and
- per-author time from new-record publication to successful refreshed view,
  with old-count responses polled for up to 75 seconds and first-attempt misses
  reported separately.

When `--churn-percent` selects providers, cache-first runs add three focused
phases after active publishing: an offline local open, one explicit refresh
while those providers are offline, and a refresh after reconnection. Refresh
outcome counters distinguish ready, network-unavailable, verification-failed,
and overloaded cached responses.

CPU and memory cover all daemon loops because they share one process. Provider
activity records harness requests and announcements, not internal Kademlia
packet counts. The artifact repeats these and the remaining local-network
limitations so results cannot be mistaken for Internet-wide capacity claims.
Status API latency is sampled every 50 ms so the observer does not become a
hostile 200-request-per-second client. Active visibility polling deliberately
uses the daemon refresh command directly; each retry therefore bypasses the
App API cooldown and represents a more aggressive workload than Spoke.

The fixed-seed unit test covers plan generation and result accounting only.
Wall-clock performance is deliberately measured, not asserted in CI.
