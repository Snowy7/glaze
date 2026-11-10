# Brief: fswatch (a file watcher that sleeps)

Read `plans/native/LIVE.md`.

## Problem

`notify` 8.2.1 on Windows (`windows.rs:160`) waits with `WaitForSingleObjectEx(wakeup_sem, 100, TRUE)`
in a loop, so **every watcher wakes 10 times a second forever**, and `poll_interval` is ignored on
Windows. The index agent measured two watchers at ~19 context switches/s with zero real work. notify is
used by `crates/shell`, `crates/bars`, `crates/tweaks` (game activity, palette), `crates/index` and
`crates/studio`, so the shell, core and index all pay this constantly.

## Goal

A tiny native watcher and no `notify` in any always-running process:

- New crate `crates/fswatch` (add it to the workspace in its own commit): watch a directory (optionally
  recursive), optionally filtered to file names; `ReadDirectoryChangesW` with an OVERLAPPED event plus a
  stop event, `WaitForMultipleObjects(INFINITE)` on one small thread per watcher (or one thread for
  several watchers if simple); debounced callback with the changed paths; buffer overflow = "rescan"
  event; clean stop and join on drop (no thread left at exit; see 2b3161f for why that matters). A
  shared helper so callers stay one or two lines. Tests for the pure parts and an integration test on a
  temp dir (create, modify, rename, delete, overflow, stop).
- Migrate every caller in `crates/shell`, `crates/bars`, `crates/tweaks` and `crates/index` (the index
  agent knows; coordinate through small separate commits, or migrate the index's config watcher
  yourself if it has finished). Leave `crates/studio` (another thread owns it, and Studio only runs while
  open); notify can stay a dependency of studio only.
- Behaviour must not change: config hot-reload, theme watch, palette, game-activity file, index config.
  Check each live after deploy (edit a config copy and restore it byte for byte).
- Measure the shell, core and index idle wakes and CPU before and after (`measure.ps1` plus per-thread
  context switches): the watcher threads should show ~0 switches/s at rest.

## Rules

Deploy only from `C:\glaze-targets\main`. Check the night plan for active no-deploy windows (the index
agent holds one until 09:45 local). `CARGO_TARGET_DIR=C:\glaze-targets\fswatch`. Stop when done, or after
2 hours. Report: callers migrated, before/after wakes and CPU, commits.
