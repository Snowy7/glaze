# Docker component

One `containers` face (`docker--containers`), rendered by Glaze's kit. The
component uses only `pipe:docker_engine`, through `pipes.http-request`; it does
not start Docker Desktop or WSL and needs no native module or plugin process.

The container list refreshes every 30 seconds while visible. Opening or refreshing
samples CPU and memory for at most 16 running containers, sequentially. The
current HTTP pipe capability returns complete responses, so the native provider's
unbounded event stream becomes bounded snapshots. Start/stop actions reconcile
the list immediately. A missing engine shows **Docker isn't running**; cached
containers keep their last values with disabled controls.

CPU uses Docker's CPU/system deltas and online CPU count. Memory excludes inactive
file cache. Missing CPU baselines stay unknown. Only ids in the current live
snapshot can be started or stopped, and pending commands cannot be repeated.

Build with `scripts/build-plugins.ps1`. The guest is its own Cargo workspace.
`cargo test --manifest-path plugins/docker/Cargo.toml` covers decoding and action
validation. The shell's `docker_plugin_size_sheet_and_action_hits` renders
`golden/sizes.png`; the runtime's `docker_component_reads_metrics_controls_and_offline_state_without_engine_side_effects`
executes the actual WASM guest while inspecting I/O effects without sending them.

## Verification (2026-11-06)

The guest tests, real Wasmtime lifecycle/action test and kit size-sheet/hit tests
pass. The snapshot also passed the integrations, bars and widgets suites. A full
release workspace build from shared HEAD `4de4d17` passed and was installed by
`deploy-local.ps1` under its desktop lock; the existing Studio window stayed open.

The deployed helper loaded Docker against the actual local pipe. Docker was not
running: the returned face showed **Docker isn't running**, with refresh available.
The probe sent only Open, Stats and Shutdown; start/stop requests were verified in
the Wasmtime test without executing their effects against the user's engine.

Measured after the offline response: **1,114,112 bytes (1.06 MiB) guest linear
memory**, **6.10 MiB helper private memory**, **10.40 MiB helper working set**. The
helper measurement is for a separate probe process with Docker as its only active
plugin, not the desktop's shared helper. No per-Docker process or native module is
shipped. `golden/comparison.html` includes the rendered sheet and native references.
