#!/usr/bin/env bash
set -euo pipefail

image=""
declare -a tags=()
declare -a platforms=()
latest=false
push=false
dry_run=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image)
      image="$2"
      shift 2
      ;;
    --tag)
      tags+=("$2")
      shift 2
      ;;
    --platform)
      platforms+=("$2")
      shift 2
      ;;
    --latest)
      latest=true
      shift
      ;;
    --push)
      push=true
      shift
      ;;
    --dry-run)
      dry_run=true
      shift
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$image" ]]; then
  echo "--image is required." >&2
  exit 1
fi

SCRIPT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_ROOT/.." && pwd)"
MANIFEST_PATH="$PROJECT_ROOT/Cargo.toml"
DOCKERFILE_PATH="$PROJECT_ROOT/docker/Dockerfile"

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

require_command docker "docker is required. Install Docker before running this script."

if [[ "$dry_run" == false ]]; then
  docker buildx version >/dev/null 2>&1 || {
    echo "docker buildx is required. Install Docker Buildx before running this script." >&2
    exit 1
  }
fi

version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$MANIFEST_PATH" | head -n 1)"
if [[ -z "$version" ]]; then
  echo "Unable to read the package version from Cargo.toml." >&2
  exit 1
fi

if [[ ${#platforms[@]} -eq 0 ]]; then
  platforms=("linux/amd64")
fi

if [[ ${#tags[@]} -eq 0 ]]; then
  tags=("$version")
  latest=true
fi

if [[ "$latest" == true ]]; then
  already_has_latest=false
  for item in "${tags[@]}"; do
    if [[ "$item" == "latest" ]]; then
      already_has_latest=true
      break
    fi
  done

  if [[ "$already_has_latest" == false ]]; then
    tags+=("latest")
  fi
fi

if [[ "$push" == false && ${#platforms[@]} -ne 1 ]]; then
  echo "Local builds can only use a single platform. Pass --push for multi-platform builds." >&2
  exit 1
fi

platform_csv="$(IFS=,; echo "${platforms[*]}")"
declare -a build_args=(buildx build --file "$DOCKERFILE_PATH" --platform "$platform_csv")

for item in "${tags[@]}"; do
  build_args+=(--tag "$image:$item")
done

if [[ "$push" == true ]]; then
  build_args+=(--push)
else
  build_args+=(--load)
fi

build_args+=("$PROJECT_ROOT")

run_step \
  "Build Docker image" \
  "docker ${build_args[*]}" \
  docker "${build_args[@]}"

if [[ "$push" == true ]]; then
  echo "Published Docker image tags:"
else
  echo "Built Docker image tags:"
fi

for item in "${tags[@]}"; do
  echo "  $image:$item"
done