# Brief: game-quiet (mypaper costs nothing while a game runs)

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md`, `docs/system.md` (the game-activity signal:
`classify`, `GameWatcher`, `read`, `watch`, `Owner::handle()`), `docs/research/explorer-and-files.md` §8
("What zero overhead requires from mypaper"), `docs/plugins.md` (helper lifecycle, timers, budgets),
`docs/performance.md` ("Hidden = free" for the engine).

## Goal

While a game (or any fullscreen/exclusive app the signal classifies as a game) runs, mypaper adds
nothing measurable. The engine already releases after 10 s fully covered and the bars hide on
fullscreen. What's left:

1. **Core** turns the game signal on by default (it is opt-in today; it's event-driven and cheap: check
   `docs/system.md` for its measured cost) and tells its children when a game starts and stops.
2. **Plugins helper**: while a game runs, plugin timers, polling sockets' periodic work and system-event
   deliveries pause (sockets stay connected; queued events coalesce to the latest), and resume on exit.
   Plugins that are already idle cost nothing either way.
3. **Index**: the index agent (round 2, running now) is told to pause background scans and
   reconciliation during games using the same message. Don't edit `crates/index`; define the message
   and document it.
4. **Shell**: widgets and bars on covered screens already sleep; check nothing else ticks during a game
   (clock alignment, stats sampler, provider subscriptions on hidden bars). The bars-parity-2 agent is
   in `crates/shell` look code right now: make only small, separate, additive edits there.
5. Measure with a real fullscreen app you start yourself (a borderless fullscreen window or a D3D
   fullscreen test app you build; never the user's apps or games): every mypaper process's CPU, wakes,
   and private memory over 60 s, before and during. Target: 0.0x % total, no wakes from timers.

## You own

`crates/core` (the wiring, small commits), `crates/plugins` (pause/resume), the message definition in
whichever shared crate carries core→child control today (additive), additive shell edits as above, and
a "Games" section in `docs/system.md` and `docs/performance.md`. Not `crates/studio`, `crates/index`,
`crates/wallpaper`.

## Stop

When the target holds with evidence, or after 2 hours. Report: what pauses, measurements, commits.
