# Brief: shell-idle (hill-climb the shell to zero)

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md` (lock rules: one action at a time),
`docs/performance.md`, `docs/bars.md` (the bars-parity handoff in b727049), and the memory notes summarised
in `docs/ui.md` about wakes. Tools: `GLAZE_WAKES=1` wake report (see `crates/shell` for how it's
enabled), `C:\glaze-targets\dockprof` (no-admin sampler, sweep scripts),
`%LOCALAPPDATA%\Temp\glaze-tools\measure.ps1`.

## Goal

`glaze-shell` at **0 % CPU when nothing on screen changes** and minimal private memory, in every state:
desktop showing with bars, dock and widgets; apps maximized; a fullscreen app; after using the
launcher, menus, flyouts and the dock.

Where it is now (bars-parity handoff, 04:14): before tonight 0.42 % / 34 MB; settled after the motion
work **1.98 %** / 34 MB; after using the launcher and menus 37 MB (+3 MB that didn't come back);
the wake report showed **356 ticker wakes in 60 s** with nothing moving. A frame ticker that keeps
running, or wakes that do nothing, are bugs: find every wake source (tickers, timers, WinEvent
hooks, provider subscriptions, cursor LOCATIONCHANGE, clock alignment, widget data) and make each
one fire only when something it shows actually changed. After an interaction ends, caches built
for it (popup snapshots, font/text layouts, menu surfaces, launcher index views) are released or
bounded so memory returns to the resting level.

## You own

`crates/shell`, `crates/ui`, `crates/bars`, `crates/widgets`, `crates/desktop` (hook filtering).
The files agent may add small launcher hooks in `crates/shell/src/flyouts`. Leave those alone and
don't fight them. The engine agent owns `crates/wallpaper` / `glass-ipc`: if a plate-publishing
path from the shell is the cost, fix the shell side only.

## How

- Measure before you change anything: 3 settled 30-60 s samples per state, per-thread CPU, the
  wake report by source. One fix per commit, with its before/after in the message; interleave
  runs, because numbers swing (see `docs/performance.md`).
- Behaviour and looks must not change. Animations still play fully when something moves. Check
  with captures of a dock hover, a menu open and a flyout open after each risky change.
- `CARGO_TARGET_DIR=C:\glaze-targets\shell-idle` for check/test; `C:\glaze-targets\main` for
  deploy builds; deploy live when a win is real.

## Stop when

Idle 0.0x % with zero ticker wakes at rest, memory back to resting after interactions, or measured
evidence of what's left and why, or after 3 hours. Commit as you go.

## Report

Each wake source found and what you did, before/after per state, commits.
