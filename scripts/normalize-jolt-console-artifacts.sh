#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Normalize Jolt Console package artifacts to stable release names.

Usage:
  scripts/normalize-jolt-console-artifacts.sh \
    --bundle KIND \
    --dist-dir DIR \
    --console-asset NAME \
    --updater-asset NAME \
    --cli-asset NAME \
    --cli-source PATH

Bundle kinds:
  appimage  Linux AppImage bundle
  dmg       macOS DMG bundle plus .app.tar.gz updater payload
  nsis      Windows NSIS setup bundle
USAGE
}

BUNDLE_KIND=""
DIST_DIR=""
CONSOLE_ASSET=""
UPDATER_ASSET=""
CLI_ASSET=""
CLI_SOURCE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle)
      BUNDLE_KIND="${2:-}"
      shift 2
      ;;
    --dist-dir)
      DIST_DIR="${2:-}"
      shift 2
      ;;
    --console-asset)
      CONSOLE_ASSET="${2:-}"
      shift 2
      ;;
    --updater-asset)
      UPDATER_ASSET="${2:-}"
      shift 2
      ;;
    --cli-asset)
      CLI_ASSET="${2:-}"
      shift 2
      ;;
    --cli-source)
      CLI_SOURCE="${2:-}"
      shift 2
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

for required in BUNDLE_KIND DIST_DIR CONSOLE_ASSET UPDATER_ASSET CLI_ASSET CLI_SOURCE; do
  if [[ -z "${!required}" ]]; then
    echo "Missing required option: $required" >&2
    usage >&2
    exit 2
  fi
done

hash_file() {
  local file="$1"
  local dir
  local base
  dir="$(dirname "$file")"
  base="$(basename "$file")"
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$dir" && sha256sum "$base" > "$base.sha256")
  else
    (cd "$dir" && shasum -a 256 "$base" > "$base.sha256")
  fi
}

mkdir -p "$DIST_DIR"

case "$BUNDLE_KIND" in
  appimage)
    cp target/release/bundle/appimage/*.AppImage "$DIST_DIR/$CONSOLE_ASSET"
    if compgen -G "target/release/bundle/appimage/*.AppImage.sig" > /dev/null; then
      cp target/release/bundle/appimage/*.AppImage.sig "$DIST_DIR/$UPDATER_ASSET.sig"
    fi
    ;;
  dmg)
    cp target/release/bundle/dmg/*.dmg "$DIST_DIR/$CONSOLE_ASSET"
    if compgen -G "target/release/bundle/macos/*.app.tar.gz" > /dev/null; then
      cp target/release/bundle/macos/*.app.tar.gz "$DIST_DIR/$UPDATER_ASSET"
    fi
    if compgen -G "target/release/bundle/macos/*.app.tar.gz.sig" > /dev/null; then
      cp target/release/bundle/macos/*.app.tar.gz.sig "$DIST_DIR/$UPDATER_ASSET.sig"
    fi
    ;;
  nsis)
    cp target/release/bundle/nsis/*-setup.exe "$DIST_DIR/$CONSOLE_ASSET"
    if compgen -G "target/release/bundle/nsis/*-setup.exe.sig" > /dev/null; then
      cp target/release/bundle/nsis/*-setup.exe.sig "$DIST_DIR/$UPDATER_ASSET.sig"
    fi
    ;;
  *)
    echo "Unsupported bundle kind: $BUNDLE_KIND" >&2
    exit 2
    ;;
esac

cp "$CLI_SOURCE" "$DIST_DIR/$CLI_ASSET"
chmod 0755 "$DIST_DIR/$CLI_ASSET"

hash_file "$DIST_DIR/$CONSOLE_ASSET"
hash_file "$DIST_DIR/$CLI_ASSET"
if [[ -f "$DIST_DIR/$UPDATER_ASSET" && "$UPDATER_ASSET" != "$CONSOLE_ASSET" ]]; then
  hash_file "$DIST_DIR/$UPDATER_ASSET"
fi
