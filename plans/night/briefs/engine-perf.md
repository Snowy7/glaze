# Brief: engine-perf (hill-climb)

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md`, then `docs/performance.md` (Native engine
scenarios, Engine hillclimb) and `docs/glass.md`.

## Goal

Make `mypaper-wallpaper` as cheap as it can be on the user's **live** desktop, without changing what
it looks like. At 02:40 on 2026-11-06 it used **27.1 % of one core**, 7.8 % GPU 3D and 184 MB
private on rain-city (shader, 30 fps, 0.65 scale) across two 2560x1440 screens, while the
2026-11-05 isolated fixture measured 2.4-4.6 % CPU. Find out why the live number is ~8x the
fixture (glass overlays and captures over windows, plate traffic from the shell, cover rescans on
window events, liquid-rust's glass, presentation, something else), fix it, then keep climbing.

Targets: rain <= 3 % of one core, still image 0 % (no wakes when nothing changes), covered or
fullscreen-app screens 0 %, private memory as low as you can get it (the open items in
`docs/performance.md`: probe adapters, swapchain cost, first snapshot), GPU per screen <= 2 ms.

## You own

`crates/wallpaper`, `crates/liquid-rust`, `crates/glass-ipc`, `tools/perf`, and the engine sections
of `docs/performance.md` / `docs/engine-notes.md` / `docs/glass.md`. If the cause sits in the shell's
plate publishing (`crates/shell`, `crates/widgets`, `crates/bars`) or in `crates/desktop`'s hooks,
make the smallest additive fix there, commit it on its own, and say so in your report.

## How

- Measure first, on the live desktop: `measure.ps1 -Label <x> -Seconds 30` (path in the night plan)
  and per-thread CPU of the engine; `C:\mypaper-targets\dockprof` has a no-admin sampler. Numbers
  swing run to run: interleave at least 3 before/after runs before claiming a win.
- One change per commit, each with its measured before/after in the message.
- Quality must not change: check with the offline lab (`lab::lab`, `tools/perf/diff-frames.py`) or
  live DPI-aware captures, and look at them.
- Deploy live (`LIVE.md`) when a win is real, so the user wakes up to it.
- Build with your own `CARGO_TARGET_DIR=C:\mypaper-targets\engine-perf` for check/test; use
  `C:\mypaper-targets\main` only for the deploy build.

## Stop when

Every target is met or you have measured evidence that what's left can't move without a visual or
behaviour change (write those trade-offs down for the user), or after 3 hours. Commit as you go.

## Report

What was wrong, each win with numbers, what's still open and why, commits.
