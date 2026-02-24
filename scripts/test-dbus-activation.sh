#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

cargo build -p garwarp -p garwarpctl >/dev/null

tmp_share=$(mktemp -d)
runtime_dir=$(mktemp -d)
trap 'rm -rf "$tmp_share" "$runtime_dir"' EXIT

mkdir -p "$tmp_share/share/dbus-1/services"
cat > "$tmp_share/share/dbus-1/services/org.freedesktop.impl.portal.desktop.garwarp.service" <<SERVICE
[D-BUS Service]
Name=org.freedesktop.impl.portal.desktop.garwarp
Exec=$repo_root/target/debug/garwarp daemon
SERVICE

XDG_DATA_DIRS="$tmp_share/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}" \
XDG_RUNTIME_DIR="$runtime_dir" \
  dbus-run-session -- bash -lc '
set -euo pipefail

dbus-send --session --print-reply \
  --dest=org.freedesktop.DBus \
  /org/freedesktop/DBus \
  org.freedesktop.DBus.StartServiceByName \
  string:org.freedesktop.impl.portal.desktop.garwarp \
  uint32:0 >/dev/null

dbus-send --session --print-reply \
  --dest=org.freedesktop.DBus \
  /org/freedesktop/DBus \
  org.freedesktop.DBus.NameHasOwner \
  string:org.freedesktop.impl.portal.desktop.garwarp | rg -q "boolean true"

"$0"/target/debug/garwarpctl status >/dev/null
"$0"/target/debug/garwarpctl stop >/dev/null

sleep 0.2

dbus-send --session --print-reply \
  --dest=org.freedesktop.DBus \
  /org/freedesktop/DBus \
  org.freedesktop.DBus.NameHasOwner \
  string:org.freedesktop.impl.portal.desktop.garwarp | rg -q "boolean false"
' "$repo_root"

echo "dbus activation smoke test passed"
