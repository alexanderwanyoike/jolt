#!/usr/bin/env bash
set -euo pipefail

REPO="${JOLT_REPO:-alexanderwanyoike/jolt}"
VERSION="${JOLT_VERSION:-latest}"
ASSET_NAME="${JOLT_ASSET_NAME:-jolt-console-x86_64.AppImage}"
INSTALL_DIR="${JOLT_INSTALL_DIR:-$HOME/.local/bin}"
STATE_DIR="${JOLT_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/jolt-console}"
BIN_NAME="${JOLT_BIN_NAME:-jolt-console}"
CHECK_ONLY=0
DRY_RUN=0
FORCE=0

usage() {
  cat <<'USAGE'
Install or update the Jolt Console Linux AppImage from GitHub releases.

Usage:
  scripts/install-jolt-console.sh [--check] [--update] [--force] [--dry-run]

Options:
  --check       Check whether a newer tagged release is available.
  --update      Install or update if a newer tagged release is available.
                This is the default behavior.
  --force      Reinstall even when the recorded version is already current.
  --dry-run    Print the resolved install plan without downloading.
  --help       Show this help.

Environment:
  JOLT_REPO         GitHub repository, default alexanderwanyoike/jolt.
  JOLT_VERSION      Release tag to install, or latest. Default latest.
  JOLT_INSTALL_DIR  Install directory. Default $HOME/.local/bin.
  JOLT_ASSET_NAME   Release asset. Default jolt-console-x86_64.AppImage.

Example:
  curl -fsSL https://raw.githubusercontent.com/alexanderwanyoike/jolt/main/scripts/install-jolt-console.sh | bash
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --check)
      CHECK_ONLY=1
      shift
      ;;
    --update)
      CHECK_ONLY=0
      shift
      ;;
    --force)
      FORCE=1
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

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

latest_tag() {
  curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" |
    sed 's#/$##; s#.*/tag/##'
}

installed_version() {
  if [[ -f "$STATE_DIR/version" ]]; then
    cat "$STATE_DIR/version"
  fi
}

need curl
need sed
need mktemp

if [[ "$VERSION" == "latest" ]]; then
  RESOLVED_VERSION="$(latest_tag)"
else
  RESOLVED_VERSION="$VERSION"
fi

if [[ -z "$RESOLVED_VERSION" || "$RESOLVED_VERSION" == "latest" ]]; then
  echo "Unable to resolve a Jolt release tag" >&2
  exit 1
fi

CURRENT_VERSION="$(installed_version || true)"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$RESOLVED_VERSION/$ASSET_NAME"
TARGET_BIN="$INSTALL_DIR/$BIN_NAME"

cat <<PLAN
Jolt Console install plan
  repo:              $REPO
  release:           $RESOLVED_VERSION
  asset:             $ASSET_NAME
  download:          $DOWNLOAD_URL
  install path:      $TARGET_BIN
  recorded version:  ${CURRENT_VERSION:-none}
  check only:        $CHECK_ONLY
PLAN

if [[ "$CHECK_ONLY" -eq 1 ]]; then
  if [[ "$CURRENT_VERSION" == "$RESOLVED_VERSION" && -x "$TARGET_BIN" ]]; then
    echo "Jolt Console is up to date."
  else
    echo "Jolt Console update available: ${CURRENT_VERSION:-not installed} -> $RESOLVED_VERSION"
  fi
  exit 0
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
  exit 0
fi

if [[ "$FORCE" -eq 0 && "$CURRENT_VERSION" == "$RESOLVED_VERSION" && -x "$TARGET_BIN" ]]; then
  echo "Jolt Console is already installed at $RESOLVED_VERSION."
  exit 0
fi

mkdir -p "$INSTALL_DIR" "$STATE_DIR"
TMP_FILE="$(mktemp)"
trap 'rm -f "$TMP_FILE"' EXIT

echo "==> Downloading Jolt Console $RESOLVED_VERSION"
curl -fL "$DOWNLOAD_URL" -o "$TMP_FILE"
chmod 0755 "$TMP_FILE"
mv "$TMP_FILE" "$TARGET_BIN"
printf '%s\n' "$RESOLVED_VERSION" > "$STATE_DIR/version"
printf '%s\n' "$ASSET_NAME" > "$STATE_DIR/asset"

cat <<DONE
==> Installed Jolt Console
  binary:  $TARGET_BIN
  version: $RESOLVED_VERSION

Run:
  $BIN_NAME

If $INSTALL_DIR is not on PATH, add it to your shell profile.
DONE
