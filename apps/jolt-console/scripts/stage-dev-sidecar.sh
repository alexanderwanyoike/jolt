#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONSOLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$CONSOLE_DIR/../.." && pwd)"

TARGET_TRIPLE="${JOLT_CONSOLE_TARGET_TRIPLE:-$(rustc -vV | awk '/host:/ {print $2}')}"
BIN_EXT=""
if [[ "$TARGET_TRIPLE" == *windows* ]]; then
  BIN_EXT=".exe"
fi

HOST_BIN="$ROOT_DIR/target/debug/jolt$BIN_EXT"
SIDECAR_BIN="$CONSOLE_DIR/src-tauri/binaries/jolt-$TARGET_TRIPLE$BIN_EXT"

echo "==> Building jolt daemon sidecar"
(cd "$ROOT_DIR" && cargo build -p jolt-node --bin jolt)

if [[ ! -x "$HOST_BIN" ]]; then
  echo "Expected built daemon binary at $HOST_BIN" >&2
  exit 1
fi

echo "==> Staging dev sidecar $SIDECAR_BIN"
mkdir -p "$(dirname "$SIDECAR_BIN")"
cp "$HOST_BIN" "$SIDECAR_BIN"
chmod 0755 "$SIDECAR_BIN"
