# Jolt Console

Jolt Console is the first-party desktop control surface for the local Jolt daemon.
It is not an external Jolt app like Pastey or Drops; it is part of the daemon
architecture and is where identity, permissions, relay state, published content,
cache state, and diagnostics will move over time.

The existing localhost dashboard remains available as a temporary debug page.

## Run

Start a local daemon first, then run the Console:

```bash
npm install
npm run tauri dev
```

By default the Console connects to:

```text
http://127.0.0.1:9862
```

To point it at another daemon URL:

```bash
JOLT_DAEMON_URL=http://127.0.0.1:9864 npm run tauri dev
```

## Verify

```bash
npm test
npm run build
cargo check -p jolt-console
```
