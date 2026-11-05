# OBS component verification — 2026-11-06

The native provider, native widget/view, built-in catalog entry, and per-process OBS executable
source are removed. Existing widget instances migrate to `obs--controls`; existing bar entries
resolve to `custom:obs--status`. Old `integrations.json` OBS fields remain readable for compatibility.

- Six guest protocol/view tests pass: challenge authentication, initial state, microphone reply
  correlation, action payloads, events, clock freezing, bounded sampling, CPU and stream bitrate.
- Two real-component runtime tests pass: scoped `4466.0` settings become port 4466 with
  `obswebsocket.json`; a real refused loopback connection renders **OBS isn't running** without
  controls. No requests are sent to a running OBS instance during verification.
- Settings-derived permission tests and three face-migration tests pass. The bars crate passes
  116 tests, with its existing Start-menu indexing test ignored.
- `cargo check --workspace --tests` passes from the HEAD archive `6c8274c`.
- The release workspace build passes. `wasm32-wasip2` component builds pass.
- The production shell renderer's `obs_plugin_faces` size sheet passes hit-bound and unchanged
  display-list checks. [The sheet](golden/host-sizes.png) was inspected against
  `crates/shell/src/integrations/golden/obs-1.png` and `obs-1.5.png`: the component adds clocks,
  pause/mic controls, all-scenes selection, stats and explicit offline faces. Compact offline
  detail is omitted to keep the title fully inside the tile.

Release helper measurements (`mypaper-plugins --bench plugins/obs`, one sample on this PC):

| Measurement | Result |
|---|---:|
| OBS linear memory | 1.06 MiB (1,114,112 bytes) |
| Active OBS process private-memory increment | 0.67 MiB |
| Whole helper working set / private bytes after loading OBS | 16.8 / 8.8 MiB |
| Warm component load | 1.13 ms |
| Cold AOT compile, instantiate and init | 741.6 ms |

The linear-memory result is also confirmed by both runtime tests. Whole-process numbers include
the runtime, compiler/cache mapping and I/O setup; they are not the plugin's linear-memory size.
