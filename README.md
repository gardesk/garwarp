# garwarp

`garwarp` is the Gardesk XDG desktop portal backend project.

Primary scope for the first implementation track:
1. Screenshot portal backend flows.
2. OpenURI/OpenFile portal backend flows.
3. App chooser and app-integration portal flows.

Planning documents live in `docs/` and are currently local-only.

## Sprint 01 Status
Current scaffold includes:
1. Rust workspace with `garwarp`, `garwarp-ipc`, and `garwarpctl`.
2. Single-instance daemon lock with stale lock cleanup.
3. Local Unix socket control path (`status`, `stop`).
4. D-Bus and systemd user activation file scaffolding.

## Local Commands
1. Start daemon: `cargo run -p garwarp -- daemon`
2. Check health and request counters: `cargo run -p garwarpctl -- status`
3. Stop daemon: `cargo run -p garwarpctl -- stop`
4. Verify D-Bus activation: `./scripts/test-dbus-activation.sh`
5. Create mock request: `cargo run -p garwarpctl -- begin req-1 :1.2 - x11:0x2a`
6. Transition mock request: `cargo run -p garwarpctl -- transition req-1 :1.2 awaiting_user`
7. List known requests: `cargo run -p garwarpctl -- list`
8. Inspect request snapshot: `cargo run -p garwarpctl -- inspect req-1`

## Runtime Tuning
1. `GARWARP_REQUEST_TIMEOUT_MS`: timeout before in-flight requests are marked `expired`.
2. `GARWARP_TERMINAL_RETENTION_MS`: retention window before terminal requests are pruned.
