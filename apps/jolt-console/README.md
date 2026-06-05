# Jolt Console

Jolt Console is the first-party desktop control surface for the local Jolt daemon.
It is not an external Jolt app like Pastey or Drops; it is part of the daemon
architecture and is where identity, permissions, relay state, published content,
cache state, and diagnostics will move over time.

The daemon root points users to Jolt Console. The old localhost page remains
available only as a debug dashboard at `/debug/dashboard`.

## Run

Run the Console:

```bash
npm install
npm run tauri dev
```

The Settings page can start a local daemon sidecar when no daemon is running.
For dev builds, point Console at a built `jolt` binary:

```bash
JOLT_DAEMON_BINARY=/path/to/jolt npm run tauri dev
```

By default the Console connects to:

```text
http://127.0.0.1:9862
```

To point it at another daemon URL:

```bash
JOLT_DAEMON_URL=http://127.0.0.1:9864 npm run tauri dev
```

If a daemon is already running outside Console, Console treats it as externally
owned and will not stop or restart it.

## Verify

```bash
npm test
npm run build
cargo check -p jolt-console
```
