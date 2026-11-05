# Tailscale component

Three kit faces: connection, peers, and traffic. The status bar contribution refreshes when
clicked. Existing `tailscale` widgets migrate to `tailscale--connection`, preserving their
instance, position, size, appearance, settings, and storage. Legacy bar entries resolve to
`custom:tailscale--status`.

The guest uses only `pipes.http-request` and the permission
`pipe:ProtectedPrefix\Administrators\Tailscale\tailscaled`. It talks to the installed Tailscale
service, without starting another VPN node or launching a per-plugin process.

Status and peer counters are snapshots: read on open, resume, refresh, and after an action.
The minute timer only updates relative last-seen labels. It does not poll LocalAPI or watch
engine telemetry. Missing traffic counters show “Unavailable”; zero counters remain zero.
Only a fresh `BackendState: Running` snapshot is green. A failed read retains useful peer data
with an unavailable status.

Connect/disconnect patch only `WantRunning`. Exit-node actions patch the masked exit-node
preferences and clear conflicting exit-node selectors. Selections must name an online peer
advertising an exit node, or explicitly clear the choice. Duplicate in-flight actions and
status replies predating an action cannot overwrite its outcome.

Build with `powershell -NoProfile -File scripts/build-plugins.ps1`. Pure guest tests run with
`cargo test --manifest-path plugins/tailscale/Cargo.toml`. The real Wasmtime tests run with
`cargo test -p mypaper-plugins tailscale_`; they inspect preference effects without executing
them against Tailscale. The shell size-sheet test is
`cargo test -p mypaper-shell tailscale_plugin_faces_render_on_the_kit`.

For a read-only check of the installed service and the helper's memory:

```powershell
py -3 plugins/tailscale/tools/verify.py --exe target/release/mypaper-plugins.exe --out-dir C:/mypaper-targets/tailscale-live --seconds 20
```

The probe isolates its app data, opens all three faces, waits for successful status data,
and records a memory and idle-event sample. It never sends a vendor action. Its local face
JSON contains real device names and addresses; keep it local.
