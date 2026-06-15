# Work Cards

This folder tracks the next practical slices of Jolt work. On a fresh agent
session, read this file first, then open the next active card and
`.notes/current-context.md` if it exists.

## Where We Are

Jolt has proved the hard network path:

- Nodes can discover, connect, publish, fetch, verify, cache, and re-serve CID-addressed content.
- Identities expose signed `.jolt` paths through append-only update logs.
- Fresh nodes can resolve global `.jolt` addresses through bootstrap relays, DHT/provider discovery, relay records, relay exchange, relay mesh exploration, provider query forwarding, and signed identity-head gossip.
- Home relays can pin owner-authorized content and update logs so Bob can fetch Alice's content while Alice is offline.
- Relay discovery and `.jolt` resolution failures are now structured enough to explain.
- A real three-node relay mesh canary has passed.
- The daemon has a persistent app-session store and capability-checked app APIs.
- A Tauri-based Jolt Console exists under `apps/jolt-console`.

The project has moved past the first app/Console proof:

```text
Done:
  Network/discovery proof.
  Offline publisher through relay.
  App session store and capability-checked app API.
  Jolt Console shell, permission approval, realtime refresh, daemon lifecycle,
  network settings, diagnostics, and old dashboard removal.
  Pastey external app session integration.
  Private Pastey sharing proof.
  Basic headless relay status for operators.

Next:
  Freeze the product proof into something people can actually install:
  Jolt Console should install the user-callable `jolt` CLI, Pastey and Spoke
  should be desktop apps with release artifacts, and a simple bootstrap relay
  should be deployable on a cheap Linux VPS.
```

The first useful product/protocol bridge was:

> Pastey asks to act as Alice for `/pastes/*`. Jolt Console shows the request, Alice approves it, and Pastey can publish/fetch through scoped app APIs without receiving Alice's private keys.

There is also a product risk to keep visible while choosing technical slices:

> Why would anyone run or join this network before it has a strong first use case?

The answer should shape the next proof. This is a product discussion, not an implementation card. Jolt should not drift into a general storage-market project before it can demonstrate one concrete thing communities want that centralized platforms make awkward.

## Current Decision Point

Do not start with WASM apps, storage markets, payments, Drops, or storage-market
mechanics.

The v0 install/distribution path is far enough along that bootstrap relay
polish is no longer the next product risk. The next sprint should fix identity
semantics before more app surface area is added.

The next direction is:

1. Jolt identity becomes a durable user namespace, not just one local signing
   key.
2. Devices become authorized, revocable writers for a user identity.
3. True multi-writer identity state is built from per-device signed logs and a
   deterministic merge.
4. App grants are scoped to one app, one local device, and one user identity.
5. App indexes follow the identity; content bytes follow fetch/cache/pin policy.
6. Private app indexes and private content bodies are encrypted for authorized
   devices.
7. Private app data follows encryption grants and explicit rewrap, not
   automatic plaintext sync.
8. Spoke and Pastey remain the proving apps. Do not build a Jolt browser yet.
9. After identity/device semantics, community identities become the discovery
   layer: apps give purpose, communities give discovery, identities give
   ownership.

The immediate sequence is:

1. [091](091-true-multi-writer-identity-device-model.md): design the
   user-identity/device/app-session model.
2. [092](092-multiple-local-identities-v0.md): let one node manage multiple
   local user identities.
3. [093](093-device-authorization-and-revocation-v0.md): add authorized and
   revocable device writers.
4. [094](094-per-device-writer-logs-and-merge-v0.md): implement per-device
   writer logs and deterministic merged identity state.
5. [095](095-identity-scoped-app-grants-v0.md): make app grants explicitly
   identity-scoped.
6. [096](096-app-data-follows-identity-v0.md): make app indexes follow the
   identity while content fetch/cache/pin stays explicit.
7. [097](097-private-content-device-access-v0.md): make private content access
   work honestly across authorized devices.

The follow-on community discovery sequence is:

1. [098](098-community-identity-and-membership-model.md): design communities as
   Jolt identities with watch/join policy.
2. [099](099-community-membership-v0.md): implement generic community
   membership, join requests, open joins, and revocation.
3. [100](100-community-scoped-app-indexes-v0.md): let apps publish/search
   community-scoped signed indexes without relay-owned search.
4. [101](101-default-discoverable-communities-v0.md): ship default
   discoverable communities without auto-joining users.
5. [102](102-spoke-community-join-v0.md): make Spoke join/read/post through
   communities.
6. [103](103-spoke-community-recommended-users-v0.md): recommend users from
   community membership and activity.

Supporting cards:

- [072](072-jolt-v0-scope-and-freeze-criteria.md): v0 boundary, non-goals, and
  freeze criteria are locked.
- [073](073-two-way-communication-design.md): recipient-controlled ingress
  design is locked.
- [074](074-reachability-and-rendezvous-clarification.md): direct-first
  reachability and optional relay-assisted delivery terms are clarified.
- [075](075-recipient-ingress-v0.md): generic direct/local recipient ingress is
  implemented for Spoke replies/mentions.
- [076](076-optional-and-authorized-relay-pinning.md): make pinning optional
  and relay-authorized.
- [077](077-jolt-distribution-v0.md): make Jolt realistically installable as
  Console + daemon + CLI.
- [083](083-console-auto-update-v0.md): close the packaged Console update loop
  with signed, user-approved updates.
- [079](079-pastey-final-compatibility-pass.md): keep Pastey working as a
  companion PoC.
- [084](084-install-jolt-cli-with-console.md): expose the user-callable `jolt`
  CLI alongside the installed Console.
- [085](085-pastey-distribution-v0.md): ship Pastey as a signed AppImage with a
  curl install/update path.
- [086](086-spoke-desktop-shell-v0.md): turn Spoke from Vite-only into a Tauri
  desktop app.
- [087](087-spoke-distribution-v0.md): ship Spoke as a signed AppImage with a
  curl install/update path.
- [088](088-bootstrap-relay-deployment-v0.md): make a headless bootstrap relay
  easy to install and operate from the released `jolt` binary.
- [089](089-relay-pinning-whitelist-v0.md): keep discovery open but authorize
  relay pinning with a simple identity allowlist.
- [090](090-v0-demo-and-launch-writeup.md): write the demo/launch post and
  record the continue/pause/bin decision.
- [091](091-true-multi-writer-identity-device-model.md): define user identity,
  device identity, app session, and true multi-writer merge semantics.
- [092](092-multiple-local-identities-v0.md): support multiple local user
  identities in daemon and Console.
- [093](093-device-authorization-and-revocation-v0.md): authorize and revoke
  device writers for a user identity.
- [094](094-per-device-writer-logs-and-merge-v0.md): resolve identity state from
  per-device writer logs.
- [095](095-identity-scoped-app-grants-v0.md): scope app grants by identity and
  device.
- [096](096-app-data-follows-identity-v0.md): support app indexes that follow a
  user identity without automatic byte sync.
- [097](097-private-content-device-access-v0.md): extend private content
  encryption to authorized devices and explicit historical rewrap.
- [098](098-community-identity-and-membership-model.md): define community
  identities, watch/join policy, membership, and discovery boundaries.
- [099](099-community-membership-v0.md): implement generic community membership
  and revocation.
- [100](100-community-scoped-app-indexes-v0.md): let apps use community-scoped
  signed indexes for discovery and local search.
- [101](101-default-discoverable-communities-v0.md): provide default
  discoverable communities without automatic membership.
- [102](102-spoke-community-join-v0.md): use communities as Spoke's first
  discovery surface.
- [103](103-spoke-community-recommended-users-v0.md): recommend users from
  signed community membership and activity.
- [062](062-console-native-presence-and-permission-focus-v0.md): native Console
  presence and focus permission prompts, still deferred until product pressure
  justifies cross-platform OS integration.
- Relay structured logs and metrics from
   [066](066-relay-operator-diagnostics-v0.md): useful for operators, but now
   last behind product/use-case work.

## Completed Sprint: App Boundary and Private Sharing Foundations

Pastey proved that a separate app can consume Jolt through the daemon. That moves the next useful work away from more raw protocol plumbing and toward the daemon/app boundary:

```text
Jolt daemon = local authority, identities, keys, network access
Jolt Console = privileged local control surface
Jolt apps = untrusted clients with scoped sessions
```

The completed app-boundary sequence is:

1. [046](046-app-permission-approval-ui.md): add Console approval UI for app permissions.
2. [047](047-pastey-app-session-integration.md): move Pastey onto app sessions in `jolt-apps`.
3. [054](054-pastey-two-node-local-demo-harness.md): make the Alice/Bob/Pastey demo repeatable on one machine.
4. [056](056-app-capability-grant-hardening.md): tighten grant validation before private app authority.
5. [049](049-crypto-agility-encrypted-object-envelope.md): design encrypted object envelopes and suite IDs.
6. [050](050-identity-encryption-key-records.md): publish and resolve signed public encryption keys.
7. [051](051-encrypted-object-implementation-v0.md): encrypt content once and wrap content keys for recipients.
8. [052](052-daemon-encrypt-decrypt-api.md): let app sessions request daemon-owned encrypt/decrypt without exposing keys.
9. [053](053-pastey-private-paste-v0.md): prove Alice can share an encrypted Pastey paste with Bob.
10. [057](057-pastey-private-open-performance-and-self-private-ux.md): make private Pastey open fast and self-only private paste creation natural.

Keep Drops out of this sprint. Pastey is already enough pressure for the daemon/app boundary and private sharing model.

The completed Console-native daemon UX sequence is:

1. [059](059-console-realtime-state-v0.md): make Console state update without manual refresh.
2. [064](064-jolt-distribution-packaging-design.md): decide the installable product shape.
3. [060](060-console-daemon-lifecycle-v0.md): let Console start/manage the local daemon honestly.
4. [061](061-console-network-settings-v0.md): move bootstrap and home relay config into Console Settings.
5. [063](063-debug-dashboard-retirement.md): remove or demote the old daemon-served debug dashboard.
6. [065](065-console-diagnostics-and-dashboard-removal.md): move remaining debug dashboard diagnostics into Console and remove the old daemon HTML dashboard.

[062](062-console-native-presence-and-permission-focus-v0.md) remains deferred
because tray/native presence and OS integration should wait until the simple
cross-platform lifecycle shape settles.

The relay-operator diagnostics sequence is:

1. [067](067-relay-cli-admin-status-v0.md): add SSH-friendly relay status.
2. [070](070-relay-diagnose-identity-v0.md): diagnose update-log provider
   discovery for one identity.

Structured logs and metrics remain valid relay-operator follow-ups from
[066](066-relay-operator-diagnostics-v0.md), but they are deliberately parked
behind product/use-case work.

Before messaging/email/realtime application work, use the direction in
[058](058-bidirectional-communication-and-signed-reachability-design.md):
Jolt should provide signed reachability metadata and generic identity-authenticated
transport primitives without pulling app concepts such as inboxes or contacts
into the protocol layer.

## Cards

| Card | Type | Status | Summary |
|---|---|---|---|
| [001](001-current-state-and-test-harness.md) | AFK | Done | Stabilize current test/dev ground and remove stale drift. |
| [002](002-local-node-dashboard-v0.md) | AFK | Superseded by 045 | Original localhost dashboard card; continue through Jolt Console work. |
| [014](014-local-multi-node-demo-mode.md) | AFK | Superseded by 054 | Original local dashboard demo; continue through Pastey two-node harness. |
| [003](003-testing-strategy-and-harness.md) | AFK | Done | Define and automate the test layers. |
| [004](004-update-log-core.md) | AFK | Done | Add signed append-only update log primitives. |
| [005](005-resolve-latest-record.md) | AFK | Done | Resolve latest signed state for an identity. |
| [016](016-global-identity-address-v0.md) | AFK | Done | Add canonical `{identity}.jolt` addresses before local petnames. |
| [017](017-global-jolt-resolution-v0.md) | AFK | Done | Define global `.jolt` resolution through signed reachability records. |
| [018](018-global-update-log-discovery-v0.md) | AFK | Done | Discover, fetch, verify, and cache signed update logs for global `.jolt` lookup. |
| [019](019-bootstrap-config-and-relay-mode-v0.md) | AFK | Done | Add persistent bootstrap config and explicit bootstrap/discovery relay mode. |
| [023](023-bootstrap-management-ux.md) | AFK | Done | Add CLI UX for listing, adding, and removing bootstrap relay addresses. |
| [024](024-dht-bootstrap-discovery-path.md) | AFK | Done | Prove Bob can discover Alice's update-log provider through a configured relay and DHT. |
| [025](025-discovered-relay-peer-cache.md) | AFK | Done | Cache useful discovered relay/node addresses for future starts. |
| [026](026-bootstrap-observability.md) | AFK | Done | Expose bootstrap state through status/API/dashboard. |
| [027](027-relay-gossip-v0.md) | HITL | Split into 033-039 | Umbrella design card for relay discovery, relay gossip, and identity/provider hints. |
| [028](028-three-node-canary-harness.md) | AFK | Done | Document/run Alice -> Relay -> Bob local test and real-world canary. |
| [020](020-jolt-resolve-api-cli-dashboard.md) | AFK | Done | Let users resolve `.jolt` addresses through CLI, API, and dashboard. |
| [021](021-fetch-by-jolt-address.md) | AFK | Done | Fetch content from a `.jolt` address instead of a raw CID. |
| [022](022-offline-publisher-through-relay-smoke-test.md) | AFK | Done | Prove Bob can fetch Alice's content while Alice is offline. |
| [015](015-local-petnames-and-address-book.md) | AFK | Deferred | Add local aliases for identity addresses after app sessions/identity UX settle. |
| [006](006-signed-path-publishing-v0.md) | AFK | Done | Publish and resolve generic signed path bindings. |
| [030](030-persistent-update-log-store.md) | AFK | Done | Persist the owner's update log so `.jolt` paths survive daemon restarts. |
| [007](007-home-relay-configuration.md) | AFK | Done | Configure a user's home relay. |
| [029](029-home-relay-publish-pinning-ux.md) | AFK | Done | Let users pin published content to their configured home relay from API, CLI, and dashboard. |
| [031](031-published-content-inventory-dashboard.md) | AFK | Done | Show local published content, relay pin state, stale paths, and repin actions in the dashboard. |
| [032](032-built-in-space-lens-demo.md) | HITL | Superseded by external app prototypes | Pastey/Drops should pressure-test app shape outside the protocol repo. |
| [033](033-relay-records-v0.md) | AFK | Done | Define signed relay records so relays can describe how they are reached. |
| [034](034-relay-address-book-v0.md) | AFK | Done | Persist verified relay records with expiry, deduplication, and bounds. |
| [035](035-relay-record-exchange-v0.md) | AFK | Done | Let nodes and relays exchange bounded sets of verified relay records. |
| [036](036-relay-mesh-exploration-v0.md) | AFK | Done | Let a relay with one known relay discover more of the relay mesh. |
| [037](037-identity-provider-query-forwarding-v0.md) | AFK | Done | Let relays forward identity/update-log provider queries across known relay neighbours. |
| [038](038-identity-head-gossip-v0.md) | AFK | Done | Exchange signed, expiring identity-head hints for common `.jolt` lookups. |
| [041](041-identity-head-gossip-fair-batching.md) | AFK | Done | Keep identity-head gossip batches fair across identities and requested limits. |
| [039](039-relay-discovery-failure-ux.md) | AFK | Done | Return clear failure reasons when relay discovery or `.jolt` resolution fails. |
| [040](040-relay-mesh-milestone-canary.md) | AFK | Done | Run one real-world relay-mesh canary after local process demos pass. |
| [008](008-owner-signed-pin-protocol.md) | AFK | Done | Define and implement owner-signed pin requests. |
| [009](009-relay-pinning-and-provider-announcement.md) | AFK | Done | Relay accepts pins, stores content, announces providers. |
| [010](010-offline-fetch-through-home-relay.md) | AFK | Superseded by 022 | End-to-end offline publisher flow. |
| [011](011-availability-checks-v0.md) | AFK | Done | Node checks whether home relay still serves pinned content. |
| [012](012-crypto-agility-spike.md) | HITL | Superseded by 049 | Original crypto-agility spike; continue through encrypted object envelope work. |
| [013](013-wasm-runtime-parking-lot.md) | HITL | Later | Park app runtime work until relay/mutable content lands. |
| [042](042-app-boundary-session-design.md) | HITL | Reviewed in PR | Define daemon/app sessions, capabilities, console/admin boundary, and forbidden app powers. |
| [043](043-app-session-store-approval-api.md) | AFK | Done | Persist pending/approved/revoked app sessions and approval APIs. |
| [044](044-capability-checked-app-api-v0.md) | AFK | Done | Add session-token app APIs for resolve/fetch/publish/inventory/pin. |
| [045](045-jolt-console-shell-v0.md) | AFK | Done | Turn the dashboard into a local daemon console with sidebar sections. |
| [046](046-app-permission-approval-ui.md) | AFK | Done | Let Console approve/reject/revoke app permission requests. |
| [047](047-pastey-app-session-integration.md) | AFK | Done | Move Pastey from trusted daemon APIs to session-token app APIs. |
| [048](048-identity-import-v0.md) | HITL | Designed in PR | Define admin-only identity import/export v0 and shared-key risks. |
| [049](049-crypto-agility-encrypted-object-envelope.md) | HITL | Done | Define encrypted object envelope, suite IDs, wrapping, and PQ-hybrid direction. |
| [050](050-identity-encryption-key-records.md) | AFK | Done | Publish and resolve signed public encryption keys for identities. |
| [051](051-encrypted-object-implementation-v0.md) | AFK | Implemented in PR | Encrypt content once and wrap content keys for recipients. |
| [052](052-daemon-encrypt-decrypt-api.md) | AFK | Implemented in PR | Let the daemon encrypt/decrypt for app sessions without exposing keys. |
| [053](053-pastey-private-paste-v0.md) | AFK | Implemented in Pastey PR | Prove Alice can share an encrypted Pastey paste with Bob. |
| [054](054-pastey-two-node-local-demo-harness.md) | AFK | Done | Add a repeatable local Alice/Bob/Pastey demo harness. |
| [055](055-jolt-console-native-daemon-ux-debt.md) | HITL | Split into follow-up cards | Umbrella for Console realtime, daemon lifecycle, native presence, settings, debug dashboard retirement, and distribution. |
| [056](056-app-capability-grant-hardening.md) | AFK | Implemented in PR | Tighten app capability grant validation before private app authority. |
| [057](057-pastey-private-open-performance-and-self-private-ux.md) | AFK | Implemented in PR | Make private Pastey open fast and self-only private paste creation natural. |
| [058](058-bidirectional-communication-and-signed-reachability-design.md) | HITL | Designed in PR | Decide how Jolt supports secure bidirectional communication through signed reachability without protocol-level inbox semantics. |
| [059](059-console-realtime-state-v0.md) | AFK | Implemented in PR | Make Console state update without manual refresh. |
| [060](060-console-daemon-lifecycle-v0.md) | AFK | Implemented in PR | Define and implement honest Console-owned daemon startup/lifecycle behavior. |
| [061](061-console-network-settings-v0.md) | AFK | Implemented in PR | Manage bootstrap and home relay configuration from Console Settings. |
| [062](062-console-native-presence-and-permission-focus-v0.md) | AFK | Deferred after simple lifecycle | Add tray/native presence and focus Console for new app permission requests. |
| [063](063-debug-dashboard-retirement.md) | AFK | Implemented in PR | Remove, gate, or demote the old daemon-served debug dashboard. |
| [064](064-jolt-distribution-packaging-design.md) | HITL | Designed in PR | Decide the installable Jolt product shape: Console, daemon sidecar, and CLI. |
| [065](065-console-diagnostics-and-dashboard-removal.md) | AFK | Implemented in PR | Move remaining debug dashboard diagnostics into Console and remove the old daemon HTML dashboard. |
| [066](066-relay-operator-diagnostics-v0.md) | HITL | Designed in PR | Define CLI/admin/logging diagnostics for headless server-facing relays. |
| [067](067-relay-cli-admin-status-v0.md) | AFK | Implemented in PR | Add SSH-friendly relay status through CLI and admin API. |
| [068](068-work-map-reset.md) | AFK | Implemented in PR | Refresh the card index after Console, private sharing, and relay-operator slices landed. |
| [069](069-signed-reachability-endpoints-v0.md) | AFK | Implemented in PR | Add signed reachability endpoint metadata without messaging/inbox semantics. |
| [070](070-relay-diagnose-identity-v0.md) | AFK | Implemented in PR | Diagnose update-log provider discovery for one identity through CLI and admin API. |
| [071](071-product-use-case-selection.md) | HITL | Discussion next | Choose the next product/use-case proof before more infrastructure polishing. |
| [072](072-jolt-v0-scope-and-freeze-criteria.md) | HITL | Decided in PR | Lock the v0 boundary, non-goals, freeze criteria, and success/failure signals. |
| [073](073-two-way-communication-design.md) | HITL | Designed in PR | Design recipient-controlled two-way communication without cross-identity namespace writes. |
| [074](074-reachability-and-rendezvous-clarification.md) | HITL | Clarified in PR | Clarify reachability, rendezvous, ingress, and app protocol terms for v0. |
| [075](075-recipient-ingress-v0.md) | AFK after design | Implemented in PR | Implement generic recipient-controlled ingress for Spoke replies/mentions. |
| [076](076-optional-and-authorized-relay-pinning.md) | AFK | Ready after 072 | Make pinning optional and relay-policy-authorized. |
| [077](077-jolt-distribution-v0.md) | AFK | Ready after 072 | Make Jolt realistically installable as Console, daemon, and CLI. |
| [078](078-spoke-social-poc.md) | HITL then AFK | Ready after 073/075 | Build the small social PoC that tests Jolt's product bet. |
| [079](079-pastey-final-compatibility-pass.md) | AFK | Ready near freeze | Verify Pastey still works against final v0 APIs and docs. |
| [080](080-v0-freeze-and-bugfix-window.md) | HITL | Ready after 078/079 | Hard stop new features and run the v0 bugfix/docs/demo pass. |
| [081](081-launch-and-postmortem.md) | HITL | Ready after 080 | Publish the project, gather feedback, and decide continue/pause/bin. |
| [082](082-network-node-module-refactor.md) | AFK | In Progress | Refactor `NetworkNode` into focused internal modules while preserving behavior. |
| [083](083-console-auto-update-v0.md) | AFK after design | Ready after 077 | Add signed, user-approved packaged Console updates with installer fallback. |
| [084](084-install-jolt-cli-with-console.md) | AFK | Ready | Install the user-callable `jolt` CLI alongside Jolt Console. |
| [085](085-pastey-distribution-v0.md) | AFK | Ready | Add Pastey AppImage CI, curl install, and signed updates. |
| [086](086-spoke-desktop-shell-v0.md) | AFK | Ready | Add a Tauri desktop shell for Spoke. |
| [087](087-spoke-distribution-v0.md) | AFK | Ready after 086 | Add Spoke AppImage CI, curl install, and signed updates. |
| [088](088-bootstrap-relay-deployment-v0.md) | AFK | Parked; bootstrap node working for now | Make a bootstrap relay easy to deploy from the released `jolt` binary. |
| [089](089-relay-pinning-whitelist-v0.md) | AFK | Ready after 088 | Add a simple relay pinning allowlist while keeping discovery open. |
| [090](090-v0-demo-and-launch-writeup.md) | HITL | Ready after 085/087/088/089 | Draft the demo/write-up and decide continue, pause, or bin. |
| [091](091-true-multi-writer-identity-device-model.md) | HITL | Discussion next | Define user identity, device writers, revocation, and true multi-writer merge semantics. |
| [092](092-multiple-local-identities-v0.md) | AFK after design | Ready after 091 | Let one node manage and switch between multiple local user identities. |
| [093](093-device-authorization-and-revocation-v0.md) | AFK after design | Ready after 091 | Add authorized and revocable device writers for a user identity. |
| [094](094-per-device-writer-logs-and-merge-v0.md) | AFK after design | Ready after 091 and 093 | Resolve identity state from per-device signed writer logs. |
| [095](095-identity-scoped-app-grants-v0.md) | AFK after design | Ready after 092 and 093 | Scope app grants to one app, one device, and one user identity. |
| [096](096-app-data-follows-identity-v0.md) | AFK after design | Ready after 094 and 095 | Let app indexes follow the identity while content bytes remain fetch/cache/pin policy. |
| [097](097-private-content-device-access-v0.md) | AFK after design | Ready after 093 and 096 | Extend private content access to authorized devices with explicit historical rewrap. |
| [098](098-community-identity-and-membership-model.md) | HITL | Ready after 091 | Define communities as Jolt identities with watch/join policy and signed membership. |
| [099](099-community-membership-v0.md) | AFK after design | Ready after 098 | Implement generic community join, open/request policies, grants, and revocation. |
| [100](100-community-scoped-app-indexes-v0.md) | AFK after design | Ready after 099 | Let apps publish and search community-scoped signed indexes locally. |
| [101](101-default-discoverable-communities-v0.md) | AFK after design | Ready after 099 | Ship default discoverable communities without auto-joining users. |
| [102](102-spoke-community-join-v0.md) | AFK after design | Ready after 100 and 101 | Let Spoke join communities and read/post through community indexes. |
| [103](103-spoke-community-recommended-users-v0.md) | AFK after design | Ready after 102 | Recommend users from signed community membership and activity. |

## Card Format

Each card uses:

- **Type:** AFK means an agent should be able to implement it without new product decisions. HITL means the card needs human direction first.
- **Blocked by:** Cards that should land first.
- **What to build:** End-to-end behavior, not just a layer.
- **Acceptance criteria:** Concrete checks for completion.

## Relay Gossip Verification

Relay discovery and gossip cards should not each require a real internet canary.

Use this rule:

- Automated deterministic tests are required for every implementation card.
- One-machine multi-process demos are required when the card changes visible relay/network behaviour.
- The Hetzner/local-machine canary should happen once at the milestone boundary, not after every small slice.
