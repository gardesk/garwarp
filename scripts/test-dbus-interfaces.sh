#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
runtime_dir="$(mktemp -d)"
introspection_file="$(mktemp)"
log_file="$(mktemp)"

cleanup() {
  rm -f "$introspection_file" "$log_file"
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

dbus-send --session --print-reply \
  --dest=org.freedesktop.impl.portal.desktop.garwarp \
  /org/freedesktop/portal/desktop \
  org.freedesktop.DBus.Introspectable.Introspect >"'"$introspection_file"'"

cargo run -q -p garwarpctl -- stop >/dev/null
wait "$pid"
'

rg -q "org.freedesktop.impl.portal.Screenshot" "$introspection_file"
rg -q "org.freedesktop.impl.portal.FileChooser" "$introspection_file"
rg -q "org.freedesktop.impl.portal.AppChooser" "$introspection_file"
rg -q "method name=\"Screenshot\"" "$introspection_file"
rg -q "method name=\"PickColor\"" "$introspection_file"
rg -q "method name=\"OpenFile\"" "$introspection_file"
rg -q "method name=\"SaveFile\"" "$introspection_file"
rg -q "method name=\"SaveFiles\"" "$introspection_file"
rg -q "method name=\"ChooseApplication\"" "$introspection_file"
rg -q "method name=\"UpdateChoices\"" "$introspection_file"

printf "dbus interface export smoke test passed\n"
