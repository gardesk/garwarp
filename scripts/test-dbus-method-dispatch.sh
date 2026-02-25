#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
runtime_dir="$(mktemp -d)"
log_file="$(mktemp)"

cleanup() {
  rm -f "$log_file"
  rm -rf "$runtime_dir"
}
trap cleanup EXIT

DBUS_FATAL_WARNINGS=0 dbus-run-session -- bash -lc '
set -euo pipefail
export XDG_RUNTIME_DIR="'"$runtime_dir"'"
cd "'"$repo_root"'"

cargo run -q -p garwarp -- daemon >"'"$log_file"'" 2>&1 &
pid=$!

for _ in $(seq 1 200); do
  if dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
      org.freedesktop.DBus.NameHasOwner string:org.freedesktop.impl.portal.desktop.garwarp \
      | rg -q "boolean true"; then
    break
  fi
  sleep 0.05
done

cargo run -q -p garwarpctl -- portal-smoke >/dev/null
cargo run -q -p garwarpctl -- stop >/dev/null
wait "$pid"
'

printf "dbus method dispatch smoke test passed\n"
