# Relay Mesh Milestone Canary

This canary proves the M5 relay mesh path with the machines currently available:

```text
Hetzner:      R1 Alice home relay + R2 public anchor relay + R3 Bob home relay
Linux:        Alice + Tim
Mac:          Bob

R1 -> R2 <- R3
Tim -> R2 only
```

Tim must start with only `R2` configured. Tim should still resolve and fetch Alice and Bob content by `.jolt` address after `R2` learns `R1` and `R3` through relay record exchange, relay mesh exploration, identity provider forwarding, and identity-head hints.

Running all three relay processes on Hetzner keeps the relay side publicly reachable even when the two user machines are behind NAT or CGNAT. The user machines still publish from separate real networks and prove the external client path.

Use isolated `XDG_DATA_HOME` values so the canary does not touch a normal Jolt node.

## 2026-05-30 Result

The real-world canary passed with this topology:

```text
Hetzner: R1, R2, R3
Linux:   Alice, Tim
Mac:     Bob
```

Observed relay IDs:

```text
R1: 12D3KooWBH9yKkf4bCVXESvaq6fkcL9VX2Q5VppfLAf83z4Ed4XV
R2: 12D3KooWAKNmewb954P3iWL6jVVVRLEw9SqTbgFiiDP8JHP4HbY5
R3: 12D3KooWPJkQZK38ugi196xw4NS3zrDKEXRt73S31qPrUXUX8TuH
```

Tim started with only `R2` configured:

```text
effective_bootstrap_relays:
  /ip4/89.167.68.65/tcp/4102/p2p/12D3KooWAKNmewb954P3iWL6jVVVRLEw9SqTbgFiiDP8JHP4HbY5
known_relay_count: 3
```

Tim resolved and fetched Alice through the relay mesh:

```text
address: b5plrjonimk6qolmeh6djo4wq7yyws5ord24x47r47imyi36bila.jolt/canary/alice
content_id: bafkr4icerogli2i7vmono4fbpgoyjgxbkcnrnkumpewixgh6g25qh2ft3y
source: network
content: hello from alice through R1
```

Tim resolved and fetched Bob through the relay mesh:

```text
address: uulemazlw3wh6ne2ium6dgkpsirz5fubbufnmlhk4um7eqoquksq.jolt/canary/bob
content_id: bafkr4igft25f72d573st5yh7qjpzsd572uzl5lwpbtr4lctambyno774n4
latest_sequence: 2
source: network
content: hello from bob through R3
```

The controlled failure check used a fresh Tim profile with only stopped `R2` configured. The API returned:

```json
{
  "code": "relay_unreachable",
  "error": "No update-log provider found for jolt:update-log:b5plrjonimk6qolmeh6djo4wq7yyws5ord24x47r47imyi36bila: configured bootstrap relays are not reachable yet. A .jolt address is globally meaningful, but it is only reachable if this node can reach a relay mesh that knows where to find the identity."
}
```

The temporary Hetzner canary processes and firewall rules were removed after the run. The older single-relay canary on `4001/9862` was left untouched.

One operational caveat: when several local daemon processes run on the same host, `jolt status` can inspect the wrong process. For multi-process canaries, prefer the HTTP API status endpoint for the exact daemon port under test.

## Build

On every machine:

```sh
git checkout dev
git pull --ff-only
cargo build --release -p jolt-node
```

## Hetzner: R1, R2, And R3

Temporarily expose the canary TCP ports:

```sh
ufw allow 4101/tcp comment 'jolt mesh canary R1 p2p'
ufw allow 4102/tcp comment 'jolt mesh canary R2 p2p'
ufw allow 4103/tcp comment 'jolt mesh canary R3 p2p'
ufw allow 9961/tcp comment 'jolt mesh canary R1 API'
ufw allow 9962/tcp comment 'jolt mesh canary R2 API'
ufw allow 9963/tcp comment 'jolt mesh canary R3 API'
```

Create relay configs:

```sh
for relay in r1 r2 r3; do
  export XDG_DATA_HOME="/opt/jolt-mesh-canary/data/$relay"
  rm -rf "$XDG_DATA_HOME"
  mkdir -p "$XDG_DATA_HOME/jolt"
  cat > "$XDG_DATA_HOME/jolt/config.json" <<'JSON'
{
  "bootstrap_relays": [],
  "use_builtin_bootstrap_relays": false,
  "bootstrap_relay": true,
  "home_relay": null
}
JSON
done
```

Start `R2` first:

```sh
export XDG_DATA_HOME="/opt/jolt-mesh-canary/data/r2"
/opt/jolt-mesh-canary/bin/jolt start \
  --api-bind 0.0.0.0 \
  --api-port 9962 \
  --p2p-port 4102 \
  --transport tcp \
  --no-mdns
```

In another Hetzner terminal:

```sh
export XDG_DATA_HOME="/opt/jolt-mesh-canary/data/r2"
/opt/jolt-mesh-canary/bin/jolt status
```

Record:

```text
R2 peer: <r2-peer-id>
R2 bootstrap: /ip4/89.167.68.65/tcp/4102/p2p/<r2-peer-id>
R2 dashboard: http://89.167.68.65:9962/dashboard
```

Start `R1` and `R3` connected only to `R2`:

```sh
export R2_BOOTSTRAP="/ip4/89.167.68.65/tcp/4102/p2p/<r2-peer-id>"

export XDG_DATA_HOME="/opt/jolt-mesh-canary/data/r1"
/opt/jolt-mesh-canary/bin/jolt start \
  --api-bind 0.0.0.0 \
  --api-port 9961 \
  --p2p-port 4101 \
  --transport tcp \
  --bootstrap "$R2_BOOTSTRAP" \
  --no-mdns

export XDG_DATA_HOME="/opt/jolt-mesh-canary/data/r3"
/opt/jolt-mesh-canary/bin/jolt start \
  --api-bind 0.0.0.0 \
  --api-port 9963 \
  --p2p-port 4103 \
  --transport tcp \
  --bootstrap "$R2_BOOTSTRAP" \
  --no-mdns
```

Record:

```text
R1 bootstrap: /ip4/89.167.68.65/tcp/4101/p2p/<r1-peer-id>
R3 bootstrap: /ip4/89.167.68.65/tcp/4103/p2p/<r3-peer-id>
```

## Linux: Alice

Start Alice connected only to `R1`:

```sh
export R1_BOOTSTRAP="/ip4/89.167.68.65/tcp/4101/p2p/<r1-peer-id>"
export XDG_DATA_HOME="$HOME/.jolt-canary/alice"
rm -rf "$XDG_DATA_HOME"
mkdir -p "$XDG_DATA_HOME/jolt"
cat > "$XDG_DATA_HOME/jolt/config.json" <<'JSON'
{
  "bootstrap_relays": [],
  "use_builtin_bootstrap_relays": false,
  "bootstrap_relay": false,
  "home_relay": null
}
JSON

target/release/jolt start \
  --api-port 9872 \
  --p2p-port 4012 \
  --transport tcp \
  --bootstrap "$R1_BOOTSTRAP" \
  --no-mdns
```

Configure Alice's home relay and publish:

```sh
export XDG_DATA_HOME="$HOME/.jolt-canary/alice"
target/release/jolt home-relay set "$R1_BOOTSTRAP" \
  --capability pinning \
  --api-url http://89.167.68.65:9961

printf 'hello from alice through R1\n' > /tmp/jolt-alice-mesh-canary.txt
target/release/jolt publish /tmp/jolt-alice-mesh-canary.txt \
  --path /canary/alice \
  --pin-home-relay
target/release/jolt status
```

Record:

```text
Alice address: <alice-identity>.jolt/canary/alice
```

## Mac: Bob

Start Bob connected only to `R3`:

```sh
export R3_BOOTSTRAP="/ip4/89.167.68.65/tcp/4103/p2p/<r3-peer-id>"
export XDG_DATA_HOME="$HOME/.jolt-canary/bob"
rm -rf "$XDG_DATA_HOME"
mkdir -p "$XDG_DATA_HOME/jolt"
cat > "$XDG_DATA_HOME/jolt/config.json" <<'JSON'
{
  "bootstrap_relays": [],
  "use_builtin_bootstrap_relays": false,
  "bootstrap_relay": false,
  "home_relay": null
}
JSON

target/release/jolt start \
  --api-port 9874 \
  --p2p-port 4014 \
  --transport tcp \
  --bootstrap "$R3_BOOTSTRAP" \
  --no-mdns
```

Configure Bob's home relay and publish:

```sh
export XDG_DATA_HOME="$HOME/.jolt-canary/bob"
target/release/jolt home-relay set "$R3_BOOTSTRAP" \
  --capability pinning \
  --api-url http://89.167.68.65:9963

printf 'hello from bob through R3\n' > /tmp/jolt-bob-mesh-canary.txt
target/release/jolt publish /tmp/jolt-bob-mesh-canary.txt \
  --path /canary/bob \
  --pin-home-relay
target/release/jolt status
```

Record:

```text
Bob address: <bob-identity>.jolt/canary/bob
```

## Linux: Tim Starts Cold With Only R2

Tim must not be configured with `R1`, `R3`, Alice, Bob, raw CIDs, or direct peer addresses.

```sh
export R2_BOOTSTRAP="/ip4/89.167.68.65/tcp/4102/p2p/<r2-peer-id>"
export XDG_DATA_HOME="$HOME/.jolt-canary/tim"
rm -rf "$XDG_DATA_HOME"
mkdir -p "$XDG_DATA_HOME/jolt"
cat > "$XDG_DATA_HOME/jolt/config.json" <<'JSON'
{
  "bootstrap_relays": [],
  "use_builtin_bootstrap_relays": false,
  "bootstrap_relay": false,
  "home_relay": null
}
JSON

target/release/jolt start \
  --api-port 9875 \
  --p2p-port 4015 \
  --transport tcp \
  --bootstrap "$R2_BOOTSTRAP" \
  --no-mdns
```

In another Linux terminal:

```sh
export XDG_DATA_HOME="$HOME/.jolt-canary/tim"
target/release/jolt status
target/release/jolt resolve <alice-identity>.jolt/canary/alice
target/release/jolt fetch <alice-identity>.jolt/canary/alice -o /tmp/jolt-tim-alice.txt
cat /tmp/jolt-tim-alice.txt

target/release/jolt resolve <bob-identity>.jolt/canary/bob
target/release/jolt fetch <bob-identity>.jolt/canary/bob -o /tmp/jolt-tim-bob.txt
cat /tmp/jolt-tim-bob.txt
```

Expected content:

```text
hello from alice through R1
hello from bob through R3
```

Expected Tim status should show at least one connected bootstrap peer and learned relay records greater than zero after the mesh has had time to exchange records.

## Controlled Failure Check

Stop `R3` on Hetzner, then try Bob again from Tim:

```sh
export XDG_DATA_HOME="$HOME/.jolt-canary/tim"
target/release/jolt resolve <bob-identity>.jolt/canary/bob
target/release/jolt fetch <bob-identity>.jolt/canary/bob -o /tmp/jolt-tim-bob-after-r3-stop.txt
```

Expected: a structured error such as `content_fetch_failed`, `content_provider_not_found`, `relay_unreachable`, or `identity_provider_not_found`. The exact code depends on which hint Tim already cached before the relay outage.

## Cleanup

Stop every canary daemon with `Ctrl-C`, or run `target/release/jolt stop` under each canary `XDG_DATA_HOME`.

Remove isolated homes when finished:

```sh
rm -rf "$HOME/.jolt-canary"
```

Remove the temporary Hetzner UFW rules when the canary is complete:

```sh
ufw delete allow 4101/tcp
ufw delete allow 4102/tcp
ufw delete allow 4103/tcp
ufw delete allow 9961/tcp
ufw delete allow 9962/tcp
ufw delete allow 9963/tcp
```
