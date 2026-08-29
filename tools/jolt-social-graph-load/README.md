# Jolt social-graph load baseline

This tool produces the repeatable pre-optimization workload for Card 110. It
runs one reader and a small configurable number of provider daemons as real
`NetworkNode` daemon loops. The providers host many deterministic but real
Ed25519 identities, signed authority chains, signed writer logs, paths, CIDs,
and content.

The reader performs a Spoke-shaped refresh for every followed identity:

1. enumerate `/spoke/posts/` append records;
2. resolve the identity's `/profile` singleton; and
3. fetch the resolved profile content and every enumerated post CID.

## Run one workload

From the Jolt repository root:

```bash
cargo run --release -p jolt-social-graph-load -- \
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

`--one-way-latency-ms`, `--bandwidth-kbps`, and `--loss-percent` shape every
reader/provider TCP link. `--churn-percent` deterministically interrupts a
percentage of provider links while new records are published, then reconnects
them after `--churn-duration-ms`. The loss control
drops encrypted TCP chunks and is intentionally a harsh fault injector, not an
IP packet-loss simulator.

`--provider-record-capacity` explicitly raises the in-memory DHT store's local
provided-key limit for high-scale load runs. Zero preserves the production
libp2p default. The matrix uses 50,000 so all three scales exercise timeline
work rather than stopping at the default provider-key ceiling; every artifact
records this override and its limitation.

## Run the baseline matrix

```bash
./scripts/run-social-graph-baselines.sh \
  ../jolt-development-docs/experiments/follower-scale-baseline/pre-change
```

The script records 100, 1,000, and 10,000-follow JSON artifacts. Environment
variables tune the shared matrix settings: `JOLT_LOAD_DAEMONS`,
`JOLT_LOAD_RECORDS`, `JOLT_LOAD_CONCURRENCY`, `JOLT_LOAD_PUBLISH_RATE`,
`JOLT_LOAD_LATENCY_MS`, `JOLT_LOAD_BANDWIDTH_KBPS`, `JOLT_LOAD_LOSS_PERCENT`,
`JOLT_LOAD_CHURN_PERCENT`, `JOLT_LOAD_CHURN_DURATION_MS`, and
`JOLT_LOAD_PROVIDED_KEYS`.

## Result contract

Each JSON artifact contains:

- the complete workload and network configuration plus a deterministic plan
  digest;
- cold, no-change warm, and new-record success-only latency percentiles;
- successful and failed identity refresh counts with classified failures;
- enumerate-sync, resolve, fetch, provider-announcement, content-announcement,
  and churn counts;
- bytes forwarded in each direction and bytes deliberately dropped;
- aggregate CPU time, RSS, virtual memory, and reader cache growth; and
- per-author time from new-record publication to successful refreshed view,
  with old-count responses polled for up to 75 seconds and first-attempt misses
  reported separately.

CPU and memory cover all daemon loops because they share one process. Provider
activity records harness requests and announcements, not internal Kademlia
packet counts. The artifact repeats these and the remaining local-network
limitations so results cannot be mistaken for Internet-wide capacity claims.

The fixed-seed unit test covers plan generation and result accounting only.
Wall-clock performance is deliberately measured, not asserted in CI.
