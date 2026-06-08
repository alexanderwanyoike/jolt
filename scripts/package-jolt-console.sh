#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONSOLE_DIR="$ROOT_DIR/apps/jolt-console"
SIDECAR_DIR="$CONSOLE_DIR/src-tauri/binaries"
TARGET_TRIPLE="${JOLT_TARGET_TRIPLE:-}"
PREPARE_ONLY=0
DRY_RUN=0

run_with_retries() {
  local attempts="$1"
  shift

  local attempt=1
  until "$@"; do
    if [[ "$attempt" -ge "$attempts" ]]; then
      return 1
    fi

    echo "Command failed; retrying ($attempt/$attempts): $*" >&2
    sleep "$((attempt * 5))"
    attempt="$((attempt + 1))"
  done
}

usage() {
  cat <<'USAGE'
Build the v0 Jolt Console Linux package with a bundled daemon sidecar.

Usage:
  scripts/package-jolt-console.sh [--prepare-only] [--dry-run]

Options:
  --prepare-only  Build and stage the daemon sidecar and web assets, but skip
                  the Tauri bundle step.
  --dry-run       Print the resolved packaging plan without building.
  --help          Show this help.

Environment:
  JOLT_TARGET_TRIPLE  Override the Rust target triple used in the sidecar name.

Outputs:
  apps/jolt-console/src-tauri/binaries/jolt-<target-triple>
  target/release/bundle/appimage/*.AppImage
  CI normalizes release assets to jolt-console-x86_64.AppImage
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prepare-only)
      PREPARE_ONLY=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$TARGET_TRIPLE" ]]; then
  TARGET_TRIPLE="$(rustc -Vv | awk '/^host:/ { print $2 }')"
fi

if [[ -z "$TARGET_TRIPLE" ]]; then
  echo "Unable to determine Rust target triple" >&2
  exit 1
fi

HOST_BIN="$ROOT_DIR/target/release/jolt"
SIDECAR_BIN="$SIDECAR_DIR/jolt-$TARGET_TRIPLE"

cat <<PLAN
Jolt Console v0 packaging plan
  repo:          $ROOT_DIR
  console:       $CONSOLE_DIR
  target triple: $TARGET_TRIPLE
  daemon binary: $HOST_BIN
  sidecar:       $SIDECAR_BIN
  prepare only:  $PREPARE_ONLY
PLAN

if [[ "$DRY_RUN" -eq 1 ]]; then
  exit 0
fi

echo "==> Building jolt daemon/CLI"
cargo build --release -p jolt-node

if [[ ! -x "$HOST_BIN" ]]; then
  echo "Expected built daemon binary at $HOST_BIN" >&2
  exit 1
fi

echo "==> Staging Tauri sidecar"
mkdir -p "$SIDECAR_DIR"
cp "$HOST_BIN" "$SIDECAR_BIN"
chmod 0755 "$SIDECAR_BIN"

echo "==> Installing Console dependencies if needed"
if [[ ! -d "$CONSOLE_DIR/node_modules" ]]; then
  (cd "$CONSOLE_DIR" && npm ci)
fi

echo "==> Building Console web assets"
(cd "$CONSOLE_DIR" && npm run build)

if [[ "$PREPARE_ONLY" -eq 1 ]]; then
  echo "==> Prepared sidecar and web assets; skipping Tauri bundle"
  exit 0
fi

echo "==> Building Linux AppImage bundle"
(cd "$CONSOLE_DIR" && run_with_retries 3 npm run tauri build -- --bundles appimage)

echo "==> Bundle artifacts"
find "$ROOT_DIR/target/release/bundle/appimage" -maxdepth 1 -type f -name '*.AppImage' -print
