#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Assemble normalized Jolt Console artifacts into a tagged release directory.

Usage:
  scripts/assemble-jolt-console-release.sh --tag TAG [--dist-dir DIR] [--release-dir DIR]

Defaults:
  --dist-dir     dist
  --release-dir  release
USAGE
}

TAG=""
DIST_DIR="dist"
RELEASE_DIR="release"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag)
      TAG="${2:-}"
      shift 2
      ;;
    --dist-dir)
      DIST_DIR="${2:-}"
      shift 2
      ;;
    --release-dir)
      RELEASE_DIR="${2:-}"
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

if [[ -z "$TAG" ]]; then
  echo "--tag is required" >&2
  usage >&2
  exit 2
fi

mkdir -p "$RELEASE_DIR"
find "$DIST_DIR" -maxdepth 2 -type f -exec cp {} "$RELEASE_DIR/" \;

required_assets=(
  "jolt-console-x86_64.AppImage.sig"
  "jolt-console-amd64.deb"
  "jolt-console-amd64.deb.sha256"
  "jolt-console-aarch64.app.tar.gz.sig"
  "jolt-console-x86_64-setup.exe.sig"
  "jolt-linux-x86_64.sha256"
  "jolt-macos-aarch64.sha256"
  "jolt-windows-x86_64.exe.sha256"
)

for asset in "${required_assets[@]}"; do
  if [[ ! -f "$RELEASE_DIR/$asset" ]]; then
    echo "Missing release asset: $RELEASE_DIR/$asset" >&2
    exit 1
  fi
done

node scripts/write-jolt-console-update-manifest.mjs \
  "$TAG" \
  "$RELEASE_DIR/latest.json" \
  linux-x86_64 \
  "$RELEASE_DIR/jolt-console-x86_64.AppImage.sig" \
  jolt-console-x86_64.AppImage \
  darwin-aarch64 \
  "$RELEASE_DIR/jolt-console-aarch64.app.tar.gz.sig" \
  jolt-console-aarch64.app.tar.gz \
  windows-x86_64 \
  "$RELEASE_DIR/jolt-console-x86_64-setup.exe.sig" \
  jolt-console-x86_64-setup.exe
