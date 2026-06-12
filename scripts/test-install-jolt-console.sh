#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

run_plan() {
  local os="$1"
  local arch="$2"
  shift 2

  JOLT_VERSION=v0.3.5 \
    JOLT_INSTALL_OS="$os" \
    JOLT_INSTALL_ARCH="$arch" \
    JOLT_INSTALL_DIR="$TMP_DIR/$os-$arch/bin" \
    JOLT_STATE_DIR="$TMP_DIR/$os-$arch/state" \
    bash scripts/install-jolt-console.sh --dry-run "$@"
}

assert_contains() {
  local output="$1"
  local expected="$2"

  if [[ "$output" != *"$expected"* ]]; then
    echo "Expected output to contain: $expected" >&2
    echo "$output" >&2
    exit 1
  fi
}

assert_not_contains() {
  local output="$1"
  local unexpected="$2"

  if [[ "$output" == *"$unexpected"* ]]; then
    echo "Expected output not to contain: $unexpected" >&2
    echo "$output" >&2
    exit 1
  fi
}

linux_plan="$(run_plan linux x86_64)"
assert_contains "$linux_plan" "platform:          linux-x86_64"
assert_contains "$linux_plan" "asset:             jolt-console-x86_64.AppImage"
assert_contains "$linux_plan" "asset:             jolt-linux-x86_64"
assert_contains "$linux_plan" "install path:      $TMP_DIR/linux-x86_64/bin/jolt-console"
assert_contains "$linux_plan" "install path:      $TMP_DIR/linux-x86_64/bin/jolt"

macos_plan="$(run_plan darwin aarch64)"
assert_contains "$macos_plan" "platform:          darwin-aarch64"
assert_contains "$macos_plan" "asset:             jolt-macos-aarch64"
assert_contains "$macos_plan" "install path:      $TMP_DIR/darwin-aarch64/bin/jolt"
assert_not_contains "$macos_plan" "jolt-console-aarch64.dmg"

windows_plan="$(run_plan windows x86_64)"
assert_contains "$windows_plan" "platform:          windows-x86_64"
assert_contains "$windows_plan" "asset:             jolt-windows-x86_64.exe"
assert_contains "$windows_plan" "install path:      $TMP_DIR/windows-x86_64/bin/jolt.exe"
assert_not_contains "$windows_plan" "jolt-console-x86_64-setup.exe"

macos_console_error="$(
  JOLT_VERSION=v0.3.5 \
    JOLT_INSTALL_OS=darwin \
    JOLT_INSTALL_ARCH=aarch64 \
    JOLT_INSTALL_DIR="$TMP_DIR/macos-console/bin" \
    JOLT_STATE_DIR="$TMP_DIR/macos-console/state" \
    bash scripts/install-jolt-console.sh --console-only --dry-run 2>&1 || true
)"
assert_contains "$macos_console_error" "Console direct install is only supported for the Linux AppImage."
assert_contains "$macos_console_error" "install jolt-console-aarch64.dmg manually"

unsupported_error="$(
  JOLT_VERSION=v0.3.5 \
    JOLT_INSTALL_OS=darwin \
    JOLT_INSTALL_ARCH=x86_64 \
    bash scripts/install-jolt-console.sh --dry-run 2>&1 || true
)"
assert_contains "$unsupported_error" "Unsupported Jolt install target: darwin-x86_64"

echo "Jolt install script platform selection verified"
