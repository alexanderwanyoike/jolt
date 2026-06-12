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

Build the native package for the current host from the repository root:

```bash
scripts/package-jolt-console.sh
```

The script:

- builds the `jolt` daemon/CLI binary in release mode;
- stages it for Tauri as `src-tauri/binaries/jolt-<target-triple>`;
- builds Console web assets;
- runs `tauri build` with a host-appropriate bundle kind.

The default bundle kind is `appimage` on Linux, `dmg` on macOS, and `nsis` on
Windows. Tagged macOS release builds request Tauri's `app,dmg` bundle targets so
the release includes both the user-installable DMG and the signed `.app.tar.gz`
updater payload. The public bundle kind can be selected explicitly:

```bash
scripts/package-jolt-console.sh --bundle appimage
scripts/package-jolt-console.sh --bundle dmg
scripts/package-jolt-console.sh --bundle nsis
```

CI normalizes release artifacts to stable names:

```text
jolt-console-x86_64.AppImage
jolt-console-aarch64.dmg
jolt-console-aarch64.app.tar.gz
jolt-console-x86_64-setup.exe
jolt-linux-x86_64
jolt-macos-aarch64
jolt-windows-x86_64.exe
```

The macOS `.app.tar.gz` file is the signed updater payload. Users install from
the `.dmg`.

Tagged releases can be installed or updated with:

```bash
curl -fsSL https://raw.githubusercontent.com/alexanderwanyoike/jolt/main/scripts/install-jolt-console.sh | bash
```

The Bash installer installs Console plus CLI on Linux. On macOS it installs the
standalone `jolt-macos-aarch64` CLI, and on Windows under Git Bash/MSYS it
installs the standalone `jolt-windows-x86_64.exe` CLI. Use the DMG or setup EXE
for the platform Console installer.

Packaged Console builds check for signed updates through Tauri's updater plugin.
The updater reads `latest.json` from GitHub Releases, verifies the platform
artifact signature with the public key committed in `tauri.conf.json`, and
relaunches after installation. Console stops the daemon only when the daemon is
owned by Console; externally managed daemons are left running.

The curl installer remains the fallback repair/update path.

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

macOS and Windows use the same Console plus sidecar model. CI builds native
packages for those platforms, but human install/update smoke tests and
production OS code-signing/notarization are still required before calling those
packages user-ready. OS services, tray/menu-bar daemon control, autostart, and
app installation/catalog features are intentionally out of scope.

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
