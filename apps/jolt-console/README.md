# Jolt Console

Jolt Console is the first-party desktop control surface for the local Jolt daemon.
It is not an external Jolt app like Pastey or Drops; it is part of the daemon
architecture and is where identity, permissions, relay state, published content,
cache state, and diagnostics live.

The daemon root points users to Jolt Console. The old daemon-served dashboard
has been retired; use Console Diagnostics for daemon troubleshooting.

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

## Package

Build the first v0 Linux package from the repository root:

```bash
scripts/package-jolt-console.sh
```

The script:

- builds the `jolt` daemon/CLI binary in release mode;
- stages it for Tauri as `src-tauri/binaries/jolt-<target-triple>`;
- builds Console web assets;
- runs `tauri build -- --bundles appimage`.

The AppImage is written under:

```text
target/release/bundle/appimage/
```

CI normalizes the release artifact to:

```text
jolt-console-x86_64.AppImage
```

Tagged releases can be installed or updated with:

```bash
curl -fsSL https://raw.githubusercontent.com/alexanderwanyoike/jolt/main/scripts/install-jolt-console.sh | bash
```

Packaged Console builds check for signed updates through Tauri's updater plugin.
The updater reads `latest.json` from GitHub Releases, verifies the AppImage
signature with the public key committed in `tauri.conf.json`, and relaunches
after installation. Console stops the daemon only when the daemon is owned by
Console; externally managed daemons are left running.

Release signing requires `TAURI_SIGNING_PRIVATE_KEY` in GitHub Actions secrets.
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is optional and only needed if the updater
key was generated with a password. The curl installer remains the fallback
repair/update path.

Check for a newer tagged release:

```bash
curl -fsSL https://raw.githubusercontent.com/alexanderwanyoike/jolt/main/scripts/install-jolt-console.sh | bash -s -- --check
```

To verify only the staging and web build without invoking the native bundler:

```bash
scripts/package-jolt-console.sh --prepare-only
```

After the sidecar has been staged, the native shell can be checked directly:

```bash
cargo check -p jolt-console
```

macOS and Windows use the same Console plus sidecar model, but v0 packaging is
only verified on Linux. OS services, tray/menu-bar daemon control, autostart,
and app installation/catalog features are intentionally out of scope.

## Reset

Jolt stores daemon state in the platform-standard per-user Jolt config/data
locations used by the CLI. For a local development reset, stop the daemon and
remove the relevant Jolt config/data directories for the test account. Packaged
v0 does not install a system service or machine-wide daemon.

## Verify

```bash
npm test
npm run build
../../scripts/package-jolt-console.sh --dry-run
../../scripts/package-jolt-console.sh --prepare-only
node ../../scripts/verify-distribution.mjs
```
