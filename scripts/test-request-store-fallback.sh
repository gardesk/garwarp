#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

runtime_dir="$(mktemp -d)"
log_file="$(mktemp)"
status_file="$(mktemp)"

cleanup() {
  rm -f "$log_file" "$status_file"
  rm -rf "$runtime_dir"
}
trap cleanup EXIT

mkdir -p "$runtime_dir/garwarp"
cat > "$runtime_dir/garwarp/requests.state" <<STORE
id=req-1	sender=:1.2	state=bogus
STORE

DBUS_FATAL_WARNINGS=0 dbus-run-session -- bash -lc '
set -euo pipefail
export XDG_RUNTIME_DIR="'"$runtime_dir"'"
cd "'"$repo_root"'"

cargo run -q -p garwarp -- daemon >"'"$log_file"'" 2>&1 &
pid=$!

for _ in $(seq 1 200); do
  [ -S "$XDG_RUNTIME_DIR/garwarp/control.sock" ] && break
  sleep 0.05
done

[ -S "$XDG_RUNTIME_DIR/garwarp/control.sock" ]

cargo run -q -p garwarpctl -- status >"'"$status_file"'"
cargo run -q -p garwarpctl -- stop >/dev/null
wait "$pid"
'

ls "$runtime_dir/garwarp"/requests.state.corrupt-* >/dev/null
[ -f "$runtime_dir/garwarp/requests.state" ]
rg -q "request_store_load_failed" "$log_file"
rg -q "request_store_quarantined" "$log_file"
rg -q "health=" "$status_file"

echo "request-store fallback smoke test passed"
