#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONSOLE_DIR="$ROOT_DIR/apps/jolt-console"
SIDECAR_DIR="$CONSOLE_DIR/src-tauri/binaries"
TARGET_TRIPLE="${JOLT_TARGET_TRIPLE:-}"
TAURI_CACHE_DIR="${TAURI_CACHE_DIR:-$HOME/.cache/tauri}"
CREATE_UPDATER_ARTIFACTS="${JOLT_CREATE_UPDATER_ARTIFACTS:-0}"
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

download_tauri_helper() {
  local filename="$1"
  local url="$2"
  local target="$TAURI_CACHE_DIR/$filename"

  if [[ -s "$target" ]]; then
    echo "    cached: $filename"
    return 0
  fi

  echo "    downloading: $filename"
  local tmp="$target.tmp"
  rm -f "$tmp"
  run_with_retries 5 curl -fL "$url" -o "$tmp"
  mv "$tmp" "$target"
}

prefetch_tauri_appimage_helpers() {
  if [[ "$(uname -s)" != "Linux" ]]; then
    return 0
  fi

  echo "==> Prefetching Tauri AppImage helper binaries"
  mkdir -p "$TAURI_CACHE_DIR"

  download_tauri_helper \
    "AppRun-x86_64" \
    "https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64"
  download_tauri_helper \
    "linuxdeploy-x86_64.AppImage" \
    "https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy/linuxdeploy-x86_64.AppImage"

  chmod 0755 \
    "$TAURI_CACHE_DIR/AppRun-x86_64" \
    "$TAURI_CACHE_DIR/linuxdeploy-x86_64.AppImage"
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
  JOLT_TARGET_TRIPLE             Override the Rust target triple used in the sidecar name.
  JOLT_CREATE_UPDATER_ARTIFACTS  Set to 1 for signed Tauri updater artifacts.

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
  updater files: $CREATE_UPDATER_ARTIFACTS
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

prefetch_tauri_appimage_helpers

if [[ "$CREATE_UPDATER_ARTIFACTS" == "1" ]]; then
  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -z "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
    echo "JOLT_CREATE_UPDATER_ARTIFACTS=1 requires TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH" >&2
    exit 1
  fi
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
fi

echo "==> Building Linux AppImage bundle"
TAURI_BUILD_ARGS=(build -- --bundles appimage)
if [[ "$CREATE_UPDATER_ARTIFACTS" == "1" ]]; then
  TAURI_BUILD_ARGS+=(--config '{"bundle":{"createUpdaterArtifacts":true}}')
fi
(cd "$CONSOLE_DIR" && run_with_retries 3 npm run tauri "${TAURI_BUILD_ARGS[@]}")

echo "==> Bundle artifacts"
find "$ROOT_DIR/target/release/bundle/appimage" -maxdepth 1 -type f -name '*.AppImage' -print
find "$ROOT_DIR/target/release/bundle/appimage" -maxdepth 1 -type f -name '*.AppImage.sig' -print
