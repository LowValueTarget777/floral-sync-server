#!/usr/bin/env bash
set -euo pipefail

dry_run=false
skip_linux_gnu=false
skip_linux_musl=false
skip_install=false
include_lite=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      dry_run=true
      shift
      ;;
    --skip-linux-gnu)
      skip_linux_gnu=true
      shift
      ;;
    --skip-linux-musl)
      skip_linux_musl=true
      shift
      ;;
    --skip-install)
      skip_install=true
      shift
      ;;
    --include-lite)
      include_lite=true
      shift
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

SCRIPT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_ROOT/.." && pwd)"
MANIFEST_PATH="$PROJECT_ROOT/Cargo.toml"
ADMIN_UI_PATH="$PROJECT_ROOT/admin-ui"
TARGET_ROOT="$PROJECT_ROOT/target"
ARTIFACT_DIR="$TARGET_ROOT/release-artifacts"

run_step() {
  local description="$1"
  local command_text="$2"
  shift 2

  echo "==> $description"
  echo "    $command_text"
  if [[ "$dry_run" == false ]]; then
    "$@"
  fi
}

require_command() {
  local command_name="$1"
  local message="$2"
  if [[ "$dry_run" == true ]]; then
    return 0
  fi

  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "$message" >&2
    exit 1
  fi
}

have_cargo_zigbuild() {
  if [[ "$dry_run" == true ]]; then
    return 0
  fi

  cargo zigbuild --help >/dev/null 2>&1
}

require_command cargo "cargo is required. Install Rust before running this script."
require_command rustup "rustup is required so the Linux release targets can be installed."
require_command npm "npm is required. Install Node.js before running this script."

build_linux_targets=true
if [[ "$skip_linux_gnu" == true && "$skip_linux_musl" == true ]]; then
  build_linux_targets=false
fi

if [[ "$build_linux_targets" == true ]]; then
  require_command zig "zig is required for cargo-zigbuild. Install Zig before running this script."

  if ! have_cargo_zigbuild; then
    if [[ "$skip_install" == true ]]; then
      echo "cargo-zigbuild is not installed. Run 'cargo install cargo-zigbuild --locked' or rerun without --skip-install." >&2
      exit 1
    fi

    run_step \
      "Install cargo-zigbuild" \
      "cargo install cargo-zigbuild --locked" \
      cargo install cargo-zigbuild --locked
  fi
fi

if [[ "$skip_install" == false ]]; then
  run_step \
    "Install admin UI dependencies" \
    "npm --prefix \"$ADMIN_UI_PATH\" ci" \
    npm --prefix "$ADMIN_UI_PATH" ci

  if [[ "$build_linux_targets" == true ]]; then
    run_step \
      "Install Rust Linux targets" \
      "rustup target add x86_64-unknown-linux-gnu x86_64-unknown-linux-musl" \
      rustup target add x86_64-unknown-linux-gnu x86_64-unknown-linux-musl
  fi
fi

run_step \
  "Prepare release artifact directory" \
  "mkdir -p \"$ARTIFACT_DIR\"" \
  mkdir -p "$ARTIFACT_DIR"

if [[ "$skip_linux_gnu" == false ]]; then
  run_step \
    "Build Linux GNU release" \
    "cargo zigbuild --manifest-path \"$MANIFEST_PATH\" --target x86_64-unknown-linux-gnu --release" \
    cargo zigbuild --manifest-path "$MANIFEST_PATH" --target x86_64-unknown-linux-gnu --release

  run_step \
    "Remove legacy Linux GNU artifact name" \
    "rm -f \"$ARTIFACT_DIR/floral-sync-server-x86_64-unknown-linux-gnu\"" \
    rm -f "$ARTIFACT_DIR/floral-sync-server-x86_64-unknown-linux-gnu"

  run_step \
    "Collect Linux GNU artifact" \
    "cp \"$TARGET_ROOT/x86_64-unknown-linux-gnu/release/floral-sync-server\" \"$ARTIFACT_DIR/floral-sync-server-x86_64-linux-gnu\"" \
    cp "$TARGET_ROOT/x86_64-unknown-linux-gnu/release/floral-sync-server" "$ARTIFACT_DIR/floral-sync-server-x86_64-linux-gnu"

  if [[ "$include_lite" == true ]]; then
    run_step \
      "Build Linux GNU lite release" \
      "cargo zigbuild --manifest-path \"$MANIFEST_PATH\" --target x86_64-unknown-linux-gnu --release --no-default-features" \
      cargo zigbuild --manifest-path "$MANIFEST_PATH" --target x86_64-unknown-linux-gnu --release --no-default-features

    run_step \
      "Collect Linux GNU lite artifact" \
      "cp \"$TARGET_ROOT/x86_64-unknown-linux-gnu/release/floral-sync-server\" \"$ARTIFACT_DIR/floral-sync-server-lite-x86_64-linux-gnu\"" \
      cp "$TARGET_ROOT/x86_64-unknown-linux-gnu/release/floral-sync-server" "$ARTIFACT_DIR/floral-sync-server-lite-x86_64-linux-gnu"
  fi
fi

if [[ "$skip_linux_musl" == false ]]; then
  run_step \
    "Build Linux musl release" \
    "cargo zigbuild --manifest-path \"$MANIFEST_PATH\" --target x86_64-unknown-linux-musl --release" \
    cargo zigbuild --manifest-path "$MANIFEST_PATH" --target x86_64-unknown-linux-musl --release

  run_step \
    "Remove legacy Linux musl artifact name" \
    "rm -f \"$ARTIFACT_DIR/floral-sync-server-x86_64-unknown-linux-musl\"" \
    rm -f "$ARTIFACT_DIR/floral-sync-server-x86_64-unknown-linux-musl"

  run_step \
    "Collect Linux musl artifact" \
    "cp \"$TARGET_ROOT/x86_64-unknown-linux-musl/release/floral-sync-server\" \"$ARTIFACT_DIR/floral-sync-server-x86_64-linux-musl\"" \
    cp "$TARGET_ROOT/x86_64-unknown-linux-musl/release/floral-sync-server" "$ARTIFACT_DIR/floral-sync-server-x86_64-linux-musl"

  if [[ "$include_lite" == true ]]; then
    run_step \
      "Build Linux musl lite release" \
      "cargo zigbuild --manifest-path \"$MANIFEST_PATH\" --target x86_64-unknown-linux-musl --release --no-default-features" \
      cargo zigbuild --manifest-path "$MANIFEST_PATH" --target x86_64-unknown-linux-musl --release --no-default-features

    run_step \
      "Collect Linux musl lite artifact" \
      "cp \"$TARGET_ROOT/x86_64-unknown-linux-musl/release/floral-sync-server\" \"$ARTIFACT_DIR/floral-sync-server-lite-x86_64-linux-musl\"" \
      cp "$TARGET_ROOT/x86_64-unknown-linux-musl/release/floral-sync-server" "$ARTIFACT_DIR/floral-sync-server-lite-x86_64-linux-musl"
  fi
fi

echo "Release artifacts are available under $ARTIFACT_DIR"
echo "Windows MSVC artifacts are produced by scripts/release.ps1 on Windows or by the GitHub Actions workflow."