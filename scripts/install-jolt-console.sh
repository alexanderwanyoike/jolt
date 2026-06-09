#!/usr/bin/env bash
set -euo pipefail

REPO="${JOLT_REPO:-alexanderwanyoike/jolt}"
VERSION="${JOLT_VERSION:-latest}"
CONSOLE_ASSET_NAME="${JOLT_CONSOLE_ASSET_NAME:-${JOLT_ASSET_NAME:-jolt-console-x86_64.AppImage}}"
CLI_ASSET_NAME="${JOLT_CLI_ASSET_NAME:-jolt-linux-x86_64}"
INSTALL_DIR="${JOLT_INSTALL_DIR:-$HOME/.local/bin}"
STATE_DIR="${JOLT_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/jolt-console}"
CONSOLE_BIN_NAME="${JOLT_CONSOLE_BIN_NAME:-${JOLT_BIN_NAME:-jolt-console}}"
CLI_BIN_NAME="${JOLT_CLI_BIN_NAME:-jolt}"
INSTALL_CONSOLE="${JOLT_INSTALL_CONSOLE:-1}"
INSTALL_CLI="${JOLT_INSTALL_CLI:-1}"
CHECK_ONLY=0
DRY_RUN=0
FORCE=0

usage() {
  cat <<'USAGE'
Install or update Jolt Console and the Jolt CLI from GitHub releases.

Usage:
  scripts/install-jolt-console.sh [--check] [--update] [--force] [--dry-run] [--cli-only] [--console-only]

Options:
  --check         Check whether a newer tagged release is available.
  --update        Install or update if a newer tagged release is available.
                  This is the default behavior.
  --force         Reinstall even when the recorded version is already current.
  --dry-run       Print the resolved install plan without downloading.
  --cli-only      Install or check only the headless jolt CLI binary.
  --console-only  Install or check only Jolt Console.
  --help          Show this help.

Environment:
  JOLT_REPO                 GitHub repository, default alexanderwanyoike/jolt.
  JOLT_VERSION              Release tag to install, or latest. Default latest.
  JOLT_INSTALL_DIR          Install directory. Default $HOME/.local/bin.
  JOLT_STATE_DIR            State directory for recorded versions.
  JOLT_CONSOLE_ASSET_NAME   Console release asset. Default jolt-console-x86_64.AppImage.
  JOLT_CLI_ASSET_NAME       CLI release asset. Default jolt-linux-x86_64.
  JOLT_CONSOLE_BIN_NAME     Console command name. Default jolt-console.
  JOLT_CLI_BIN_NAME         CLI command name. Default jolt.
  JOLT_INSTALL_CONSOLE      Set to 0 to skip Console.
  JOLT_INSTALL_CLI          Set to 0 to skip the CLI.

Examples:
  curl -fsSL https://raw.githubusercontent.com/alexanderwanyoike/jolt/main/scripts/install-jolt-console.sh | bash
  curl -fsSL https://raw.githubusercontent.com/alexanderwanyoike/jolt/main/scripts/install-jolt-console.sh | bash -s -- --cli-only
USAGE
}

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

normalize_bool() {
  local value="$1"
  local name="$2"

  case "$value" in
    1|true|TRUE|yes|YES|on|ON)
      printf '1\n'
      ;;
    0|false|FALSE|no|NO|off|OFF)
      printf '0\n'
      ;;
    *)
      echo "$name must be 0/1, true/false, yes/no, or on/off" >&2
      exit 2
      ;;
  esac
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
    --cli-only)
      INSTALL_CONSOLE=0
      INSTALL_CLI=1
      shift
      ;;
    --console-only)
      INSTALL_CONSOLE=1
      INSTALL_CLI=0
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

INSTALL_CONSOLE="$(normalize_bool "$INSTALL_CONSOLE" "JOLT_INSTALL_CONSOLE")"
INSTALL_CLI="$(normalize_bool "$INSTALL_CLI" "JOLT_INSTALL_CLI")"

if [[ "$INSTALL_CONSOLE" -eq 0 && "$INSTALL_CLI" -eq 0 ]]; then
  echo "Nothing selected to install; enable Console, CLI, or both." >&2
  exit 2
fi

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
  local state_name="$1"
  local state_file="$STATE_DIR/$state_name.version"

  if [[ -f "$state_file" ]]; then
    cat "$state_file"
  elif [[ "$state_name" == "jolt-console" && -f "$STATE_DIR/version" ]]; then
    cat "$STATE_DIR/version"
  fi
}

asset_needs_update() {
  local state_name="$1"
  local target_bin="$2"
  local current_version

  current_version="$(installed_version "$state_name" || true)"

  if [[ "$FORCE" -eq 0 && "$current_version" == "$RESOLVED_VERSION" && -x "$target_bin" ]]; then
    return 1
  fi

  return 0
}

print_asset_plan() {
  local label="$1"
  local asset_name="$2"
  local bin_name="$3"
  local state_name="$4"
  local current_version

  current_version="$(installed_version "$state_name" || true)"

  cat <<PLAN
  $label:
    asset:             $asset_name
    download:          https://github.com/$REPO/releases/download/$RESOLVED_VERSION/$asset_name
    install path:      $INSTALL_DIR/$bin_name
    recorded version:  ${current_version:-none}
PLAN
}

check_asset() {
  local label="$1"
  local state_name="$2"
  local bin_name="$3"
  local current_version
  local target_bin="$INSTALL_DIR/$bin_name"

  current_version="$(installed_version "$state_name" || true)"

  if [[ "$current_version" == "$RESOLVED_VERSION" && -x "$target_bin" ]]; then
    echo "$label is up to date."
  else
    echo "$label update available: ${current_version:-not installed} -> $RESOLVED_VERSION"
  fi
}

install_asset() {
  local label="$1"
  local asset_name="$2"
  local bin_name="$3"
  local state_name="$4"
  local download_url="https://github.com/$REPO/releases/download/$RESOLVED_VERSION/$asset_name"
  local target_bin="$INSTALL_DIR/$bin_name"
  local tmp_file

  if ! asset_needs_update "$state_name" "$target_bin"; then
    echo "$label is already installed at $RESOLVED_VERSION."
    return 0
  fi

  tmp_file="$(mktemp)"

  echo "==> Downloading $label $RESOLVED_VERSION"
  if ! run_with_retries 5 curl -fL "$download_url" -o "$tmp_file"; then
    rm -f "$tmp_file"
    return 1
  fi
  chmod 0755 "$tmp_file"
  mv "$tmp_file" "$target_bin"
  printf '%s\n' "$RESOLVED_VERSION" > "$STATE_DIR/$state_name.version"
  printf '%s\n' "$asset_name" > "$STATE_DIR/$state_name.asset"

  if [[ "$state_name" == "jolt-console" ]]; then
    printf '%s\n' "$RESOLVED_VERSION" > "$STATE_DIR/version"
    printf '%s\n' "$asset_name" > "$STATE_DIR/asset"
  fi

  cat <<DONE
==> Installed $label
  binary:  $target_bin
  version: $RESOLVED_VERSION
DONE
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

cat <<PLAN
Jolt install plan
  repo:              $REPO
  release:           $RESOLVED_VERSION
  install dir:       $INSTALL_DIR
  state dir:         $STATE_DIR
  check only:        $CHECK_ONLY
  selected:
PLAN

if [[ "$INSTALL_CONSOLE" -eq 1 ]]; then
  print_asset_plan "Jolt Console" "$CONSOLE_ASSET_NAME" "$CONSOLE_BIN_NAME" "jolt-console"
fi

if [[ "$INSTALL_CLI" -eq 1 ]]; then
  print_asset_plan "Jolt CLI" "$CLI_ASSET_NAME" "$CLI_BIN_NAME" "jolt"
fi

if [[ "$CHECK_ONLY" -eq 1 ]]; then
  if [[ "$INSTALL_CONSOLE" -eq 1 ]]; then
    check_asset "Jolt Console" "jolt-console" "$CONSOLE_BIN_NAME"
  fi
  if [[ "$INSTALL_CLI" -eq 1 ]]; then
    check_asset "Jolt CLI" "jolt" "$CLI_BIN_NAME"
  fi
  exit 0
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
  exit 0
fi

mkdir -p "$INSTALL_DIR" "$STATE_DIR"

if [[ "$INSTALL_CONSOLE" -eq 1 ]]; then
  install_asset "Jolt Console" "$CONSOLE_ASSET_NAME" "$CONSOLE_BIN_NAME" "jolt-console"
fi

if [[ "$INSTALL_CLI" -eq 1 ]]; then
  install_asset "Jolt CLI" "$CLI_ASSET_NAME" "$CLI_BIN_NAME" "jolt"
fi

cat <<DONE
==> Jolt install complete

Run:
DONE

if [[ "$INSTALL_CONSOLE" -eq 1 ]]; then
  echo "  $CONSOLE_BIN_NAME"
fi

if [[ "$INSTALL_CLI" -eq 1 ]]; then
  echo "  $CLI_BIN_NAME --version"
fi

cat <<DONE

If $INSTALL_DIR is not on PATH, add it to your shell profile.
DONE
