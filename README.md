# garwarp

`garwarp` is the Gardesk XDG desktop portal backend project.

Primary scope for the first implementation track:
1. Screenshot portal backend flows.
2. OpenURI/OpenFile portal backend flows.
3. App chooser and app-integration portal flows.

Planning documents live in `docs/` and are currently local-only.

## Current Status
1. Sprint 01 complete: workspace, daemon lifecycle, control protocol scaffold, activation files.
2. Sprint 02 complete: control-plane hardening (idempotence, parser strictness, trusted peers, store durability/fallback, health recovery policy).
3. Sprint 03 in progress: screenshot/filechooser/appchooser interfaces export method skeletons at `/org/freedesktop/portal/desktop`.
4. Request-handle parsing and deterministic request-id derivation are implemented (`/org/freedesktop/portal/desktop/request/<sender>/<token>` model).
5. DBus method handling derives caller identity from message headers and rejects mismatched request-handle ownership.
6. Typed option parsing foundations are in place for screenshot/filechooser/appchooser known keys.

## Local Commands
1. Start daemon: `cargo run -p garwarp -- daemon`
2. Check health and request counters: `cargo run -p garwarpctl -- status`
3. Stop daemon: `cargo run -p garwarpctl -- stop`
4. Verify D-Bus activation: `./scripts/test-dbus-activation.sh`
5. Verify DBus interfaces are exported: `./scripts/test-dbus-interfaces.sh`
6. Verify request-store fallback: `./scripts/test-request-store-fallback.sh`
7. Create mock request: `cargo run -p garwarpctl -- begin req-1 :1.2 - x11:0x2a`
8. Transition mock request: `cargo run -p garwarpctl -- transition req-1 :1.2 awaiting_user`
9. List known requests: `cargo run -p garwarpctl -- list`
10. Inspect request snapshot: `cargo run -p garwarpctl -- inspect req-1`

## Runtime Tuning
1. `GARWARP_REQUEST_TIMEOUT_MS`: timeout before in-flight requests are marked `expired`.
2. `GARWARP_TERMINAL_RETENTION_MS`: retention window before terminal requests are pruned.

## Control Protocol Compatibility
1. Protocol version is reported by `status protocol=<n>` and currently fixed at `v1`.
2. Unknown or duplicate request fields are rejected as `invalid_request` (fail-closed parsing).
3. Stable error reasons are part of the control contract; new reasons may be added, existing reasons should not be repurposed.
4. Sender identity for begin/transition is derived from Unix peer credentials; payload `sender` is ignored for ownership.
