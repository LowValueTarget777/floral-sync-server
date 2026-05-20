#!/bin/sh
set -eu

config_path="${FLORAL_CONFIG_PATH:-/var/lib/floral-sync/config/sync-server.toml}"
sync_token="${FLORAL_SYNC_TOKEN:-}"

if [ -z "$sync_token" ] && [ -f "$config_path" ]; then
  sync_token="$(sed -n 's/^sync_token = "\(.*\)"$/\1/p' "$config_path" | head -n 1)"
fi

if [ -z "$sync_token" ]; then
  echo "sync token is not available for healthcheck" >&2
  exit 1
fi

curl -fsS -H "Authorization: Bearer $sync_token" http://127.0.0.1:8787/health >/dev/null