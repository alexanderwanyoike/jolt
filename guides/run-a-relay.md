# Running a Relay

```meta
Guide: 02
Kicker: JOLT OPERATIONS GUIDE
Role: Bootstrap relay
Host: Any Linux box
Footprint: One binary, one UDP port
Description: Run your own Jolt relay on any Linux server: install the binary, one systemd unit, one open UDP port, and point your nodes at it.
```

A relay is an always-on Jolt node on a public address. It helps other nodes find each other (discovery), carries traffic between peers whose NATs refuse to hole-punch (availability), and can hold pinned content so your stuff stays fetchable while your laptop sleeps. Every step on this page is taken from the relay that bootstraps the Jolt network today, a single small VPS.

Two things a relay can never do, by construction: it cannot read encrypted content (envelopes are sealed to their recipients, per [JOLT-RFC-0004](../rfcs/0004-encrypted-device-access.html)), and it cannot impersonate anyone (every record is signed by its owner's key, per [JOLT-RFC-0001](../rfcs/0001-core-protocol.html)). Running a relay is infrastructure, not authority.

## 1 · What you need

- A Linux server with a public IPv4 address. The production relay is the smallest Hetzner cloud box; anything with ~512 MB of RAM is comfortable.
- One inbound UDP port (this guide uses 4001).
- Ten minutes.

## 2 · Install the binary

Grab the latest release binary and verify it:

```bash
curl -sLO https://github.com/alexanderwanyoike/jolt/releases/latest/download/jolt-linux-x86_64
curl -sLO https://github.com/alexanderwanyoike/jolt/releases/latest/download/jolt-linux-x86_64.sha256
sha256sum -c <(awk '{print $1"  jolt-linux-x86_64"}' jolt-linux-x86_64.sha256)
sudo install -m 755 jolt-linux-x86_64 /usr/local/bin/jolt
jolt --version
```

## 3 · A user and a data directory

The relay runs as an unprivileged user with its own state directory:

```bash
sudo useradd --system --home /var/lib/jolt-relay --shell /usr/sbin/nologin jolt
sudo mkdir -p /var/lib/jolt-relay
sudo chown jolt:jolt /var/lib/jolt-relay
```

## 4 · The systemd unit

This is the production unit, verbatim apart from the name. The flags that matter: `--p2p-port 4001` fixes the port so your firewall rule and your multiaddr stay true, `--no-mdns` turns off LAN discovery (meaningless in a datacenter), and the API stays bound to localhost so only you can talk to it.

```ini /etc/systemd/system/jolt-relay.service
[Unit]
Description=Jolt relay node
Wants=network-online.target
After=network-online.target
StartLimitIntervalSec=300
StartLimitBurst=10

[Service]
Type=simple
User=jolt
Group=jolt
Environment=XDG_DATA_HOME=/var/lib/jolt-relay
WorkingDirectory=/var/lib/jolt-relay
ExecStart=/usr/local/bin/jolt start --api-bind 127.0.0.1 --api-port 9862 --p2p-port 4001 --no-mdns
Restart=on-failure
RestartSec=5s
StandardOutput=journal
StandardError=journal
LogRateLimitIntervalSec=30s
LogRateLimitBurst=1000
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/jolt-relay

[Install]
WantedBy=multi-user.target
```

Enable and start it:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now jolt-relay.service
```

## 5 · Open the port

Only the P2P port is public. The HTTP API stays on localhost.

```bash
sudo ufw allow 4001/udp comment "Jolt relay P2P"
```

## 6 · Verify and get your relay address

The daemon's status endpoint tells you it is healthy and gives you the peer id you need:

```bash
curl -s http://127.0.0.1:9862/api/v1/status | python3 -m json.tool
```

Look for `"peer_id"`. Your relay's multiaddr is:

```text
/ip4/YOUR.SERVER.IP/udp/4001/quic-v1/p2p/YOUR_PEER_ID
```

## 7 · Point your nodes at it

On each machine that should use your relay, add it persistently:

```bash
jolt bootstrap add /ip4/YOUR.SERVER.IP/udp/4001/quic-v1/p2p/YOUR_PEER_ID
jolt bootstrap list
```

`bootstrap list` shows configured, built-in, and effective relays; adding yours does not remove the default one, so your nodes get both. For a one-off run (or to test before committing), the same multiaddr works as a start flag:

```bash
jolt start --bootstrap /ip4/YOUR.SERVER.IP/udp/4001/quic-v1/p2p/YOUR_PEER_ID
```

## 8 · Operations

- **Logs**: `journalctl -u jolt-relay.service -f`. The unit rate-limits log bursts so a chatty peer cannot fill your disk.
- **Updating**: stop the service before replacing the binary, then start it again. Never overwrite a running binary in place.

```bash
sudo systemctl stop jolt-relay.service
sudo install -m 755 jolt-linux-x86_64 /usr/local/bin/jolt
sudo systemctl start jolt-relay.service
```

- **Health**: the status endpoint is cheap to poll; `connected_peers` tells you whether anyone is using you. A relay that has just restarted takes a minute to re-establish peer connections.
- **Footprint**: the production relay idles at a few tens of MB of RAM and negligible CPU. Disk grows only with pinned content.

## Where to go next

- The [protocol series](../rfcs/) specifies what relays can and cannot see; [JOLT-RFC-0001](../rfcs/0001-core-protocol.html) covers resolution and [JOLT-RFC-0004](../rfcs/0004-encrypted-device-access.html) covers why encrypted objects stay sealed.
- Build something on the network with the [app development guide](app-development.html) and the [Jolt SDK](../sdk/).
- Problems? [Open an issue](https://github.com/alexanderwanyoike/jolt/issues); relay reports from real networks are exactly the feedback the project wants.
