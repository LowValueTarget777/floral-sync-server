#!/usr/bin/env bash
set -euo pipefail

dry_run=false
skip_install=false
include_lite=false
targets=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --target" >&2
        exit 1
      fi
      targets+=("$2")
      shift 2
      ;;
    --dry-run)
      dry_run=true
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

if [[ ${#targets[@]} -eq 0 ]]; then
  targets=("all")
fi

build_linux_gnu=false
build_linux_musl=false

for target in "${targets[@]}"; do
  case "$target" in
    all|linux)
      build_linux_gnu=true
      build_linux_musl=true
      ;;
    linux-gnu)
      build_linux_gnu=true
      ;;
    linux-musl)
      build_linux_musl=true
      ;;
    windows)
      echo "The shell wrapper only builds Linux binaries. Use scripts/build.ps1 on Windows for Windows artifacts." >&2
      exit 1
      ;;
    *)
      echo "Unknown target: $target" >&2
      exit 1
      ;;
  esac
done

if [[ "$build_linux_gnu" == false && "$build_linux_musl" == false ]]; then
  echo "No build targets selected. Use --target all, linux, linux-gnu, or linux-musl." >&2
  exit 1
fi

SCRIPT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RELEASE_SCRIPT="$SCRIPT_ROOT/release.sh"

if [[ ! -f "$RELEASE_SCRIPT" ]]; then
  echo "Unable to find release script at $RELEASE_SCRIPT" >&2
  exit 1
fi

release_args=()
if [[ "$dry_run" == true ]]; then
  release_args+=(--dry-run)
fi

if [[ "$skip_install" == true ]]; then
  release_args+=(--skip-install)
fi

if [[ "$include_lite" == true ]]; then
  release_args+=(--include-lite)
fi

if [[ "$build_linux_gnu" == false ]]; then
  release_args+=(--skip-linux-gnu)
fi

if [[ "$build_linux_musl" == false ]]; then
  release_args+=(--skip-linux-musl)
fi

selected_labels=()
if [[ "$build_linux_gnu" == true ]]; then
  selected_labels+=("linux-gnu")
fi

if [[ "$build_linux_musl" == true ]]; then
  selected_labels+=("linux-musl")
fi

printf 'Selected targets: %s\n' "$(IFS=', '; echo "${selected_labels[*]}")"
"$RELEASE_SCRIPT" "${release_args[@]}"