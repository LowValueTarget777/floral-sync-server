#!/bin/sh
set -eu

toml_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

generate_token() {
  if command -v cat >/dev/null 2>&1 && [ -r /proc/sys/kernel/random/uuid ]; then
    cat /proc/sys/kernel/random/uuid
    return
  fi

  if command -v uuidgen >/dev/null 2>&1; then
    uuidgen
    return
  fi

  date +%s | sha256sum | cut -d' ' -f1
}

sync_listen="${FLORAL_SYNC_LISTEN:-0.0.0.0:8787}"
admin_listen="${FLORAL_ADMIN_LISTEN:-0.0.0.0:8788}"
db_path="${FLORAL_DB_PATH:-/var/lib/floral-sync/data/floral-sync.sqlite3}"
export_dir="${FLORAL_EXPORT_DIR:-/var/lib/floral-sync/exports}"
log_path="${FLORAL_LOG_PATH:-/var/lib/floral-sync/logs/floral-sync-server.log}"
log_level="${FLORAL_LOG_LEVEL:-info}"
config_path="${FLORAL_CONFIG_PATH:-/var/lib/floral-sync/config/sync-server.toml}"
force_write_config="${FLORAL_FORCE_WRITE_CONFIG:-0}"
sync_token="${FLORAL_SYNC_TOKEN:-}"
admin_session_secret="${FLORAL_ADMIN_SESSION_SECRET:-}"
admin_password_hash="${FLORAL_ADMIN_PASSWORD_HASH:-}"

prepare_runtime_dirs() {
  mkdir -p \
    "$(dirname "$config_path")" \
    "$(dirname "$db_path")" \
    "$export_dir" \
    "$(dirname "$log_path")"
}

if [ "$(id -u)" = "0" ]; then
  prepare_runtime_dirs

  if chown -R floral:floral \
    "$(dirname "$config_path")" \
    "$(dirname "$db_path")" \
    "$export_dir" \
    "$(dirname "$log_path")" 2>/dev/null; then
    exec gosu floral:floral "$0" "$@"
  fi

  echo "Warning: unable to update ownership for persistent directories; continuing as root." >&2
fi

prepare_runtime_dirs

if [ "$force_write_config" != "1" ] && [ -s "$config_path" ]; then
  exec /usr/local/bin/floral-sync-server --config "$config_path"
fi

if [ -z "$sync_token" ]; then
  sync_token="$(generate_token)"
fi

if [ -z "$admin_session_secret" ]; then
  admin_session_secret="$(generate_token)"
fi

umask 077

cat > "$config_path" <<EOF
sync_listen = ["$(toml_escape "$sync_listen")"]
admin_listen = ["$(toml_escape "$admin_listen")"]
db_path = "$(toml_escape "$db_path")"
export_dir = "$(toml_escape "$export_dir")"
log_path = "$(toml_escape "$log_path")"
log_level = "$(toml_escape "$log_level")"
sync_token = "$(toml_escape "$sync_token")"
admin_session_secret = "$(toml_escape "$admin_session_secret")"
EOF

if [ -n "$admin_password_hash" ]; then
  printf 'admin_password_hash = "%s"\n' "$(toml_escape "$admin_password_hash")" >> "$config_path"
fi

exec /usr/local/bin/floral-sync-server --config "$config_path"