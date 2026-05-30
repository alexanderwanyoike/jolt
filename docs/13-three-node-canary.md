# Three-Node Canary

This canary proves the current global discovery shape:

```text
Alice -> Relay -> Bob
```

Bob must not start with Alice's peer ID, raw CID, direct multiaddr, or update log. Bob should know only a relay/bootstrap address, discover Alice's update-log provider through the relay-backed DHT path, fetch Alice's signed update log, verify it, and then resolve/fetch Alice's content.

## Local Deterministic Canary

Run:

```sh
./scripts/test-three-node-canary.sh
```

The script runs:

```sh
cargo test --locked -p jolt-network bob_discovers_alice_update_log_provider_through_bootstrap_relay -- --nocapture
```

Expected result:

```text
test bob_discovers_alice_update_log_provider_through_bootstrap_relay ... ok
```

This test starts three TCP nodes with mDNS disabled:

- Relay listens and acts as the only bootstrap contact.
- Alice stores a signed update log and announces herself as the provider.
- Bob bootstraps through only the relay.
- Bob finds Alice through DHT provider discovery.
- Bob requests Alice's update log and stores it only after verification.

## Manual Real-World Canary

Use three machines or networks when possible:

- Relay: public server, ideally with stable uptime.
- Alice: home or mobile network.
- Bob: different home, mobile, office, or CGNAT network.

Build first:

```sh
cargo build --release -p jolt-node
```

Use separate data homes so the canary does not touch your normal Jolt node.

### Relay

On the public relay host:

```sh
export XDG_DATA_HOME="$HOME/.jolt-canary/relay"
mkdir -p "$XDG_DATA_HOME/jolt"
cat > "$XDG_DATA_HOME/jolt/config.json" <<'JSON'
{
  "bootstrap_relays": [],
  "use_builtin_bootstrap_relays": false,
  "bootstrap_relay": true,
  "home_relay": null
}
JSON

target/release/jolt start --api-bind 0.0.0.0 --api-port 9862 --p2p-port 4001
```

In another relay terminal:

```sh
export XDG_DATA_HOME="$HOME/.jolt-canary/relay"
target/release/jolt status
```

Record:

```text
Peer ID: <relay-peer-id>
Bootstrap address: /p2p/<relay-peer-id>
Dashboard: http://<relay-public-ip>:9862/dashboard
```

For the default iroh transport, `/p2p/<relay-peer-id>` is the bootstrap address. If running a TCP-only canary, use `/ip4/<relay-public-ip>/tcp/<p2p-port>/p2p/<relay-peer-id>` instead.

### Alice

On Alice's machine:

```sh
export XDG_DATA_HOME="$HOME/.jolt-canary/alice"
target/release/jolt start --api-port 9863 --bootstrap /p2p/<relay-peer-id>
```

In another Alice terminal:

```sh
export XDG_DATA_HOME="$HOME/.jolt-canary/alice"
printf 'hello from alice canary\n' > /tmp/jolt-alice-canary.txt
target/release/jolt publish /tmp/jolt-alice-canary.txt --path /canary/profile
target/release/jolt status
```

Record Alice's Jolt address from status, then build the content address:

```text
<alice-identity>.jolt/canary/profile
```

Expected Alice status:

```text
Status:      Running
Bootstrap:  connected (1 connected / 1 effective / 0 configured)
Peers:      1 or more
```

### Bob

On Bob's machine:

```sh
export XDG_DATA_HOME="$HOME/.jolt-canary/bob"
target/release/jolt start --api-port 9864 --bootstrap /p2p/<relay-peer-id>
```

In another Bob terminal:

```sh
export XDG_DATA_HOME="$HOME/.jolt-canary/bob"
target/release/jolt status
target/release/jolt resolve <alice-identity>.jolt/canary/profile
target/release/jolt fetch <alice-identity>.jolt/canary/profile -o /tmp/jolt-bob-canary.txt
cat /tmp/jolt-bob-canary.txt
```

Expected Bob status:

```text
Status:      Running
Bootstrap:  connected (1 connected / 1 effective / 0 configured)
Peers:      1 or more
```

Expected fetch output:

```text
hello from alice canary
```

## Failure Hints

`No provider found for: jolt:update-log:<identity>`

Bob reached the network but did not find Alice's update-log provider. Check that Alice is still running, Alice shows the relay as a peer, Alice published with `--path`, and Alice's address was copied correctly. Wait a few seconds and retry; provider discovery is asynchronous.

`Failed to dial the requested peer`

Bob found a provider but could not connect. Check that the relay and Alice are both online, the relay peer ID is correct, and the network allows outbound UDP/QUIC for the iroh canary. For a TCP-only canary, confirm the relay TCP port is open.

`Bootstrap: bootstrapping` for more than a few seconds

The node has a bootstrap address but has not connected to it. Verify the relay process is running, the bootstrap address includes `/p2p/<peer-id>`, and the relay dashboard shows incoming peers.

`Bootstrap: degraded`

A bootstrap attempt failed. Run `jolt status` and inspect the bootstrap error. Remove stale cached hints if needed by deleting the canary data home, then retry from the configured relay.

## Cleanup

Stop each daemon with `Ctrl-C` in the terminal where it is running, or:

```sh
export XDG_DATA_HOME="$HOME/.jolt-canary/alice"
target/release/jolt stop
```

Repeat with the relay and Bob `XDG_DATA_HOME` values.
