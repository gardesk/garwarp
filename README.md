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
2. Check health: `cargo run -p garwarpctl -- status`
3. Stop daemon: `cargo run -p garwarpctl -- stop`
