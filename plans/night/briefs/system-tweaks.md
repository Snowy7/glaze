# Brief: system-tweaks (games, dev, Explorer speed: supported, reversible settings)

Read `plans/native/LIVE.md`, `crates/tweaks/src/lib.rs` and `undo.rs`
(the journal), `docs/windows-styling.md`, and `docs/research/explorer-and-files.md` §7 and §8 (what
measurably helps, what doesn't, what needs admin).

## Goal

A small set of **honest** system settings Glaze can apply and fully undo, without admin, each
backed by a primary source in the research. No "boost" presets, no placebo, no registry piles.

- `config.json` › `system` (new section, every field optional, absent = leave Windows alone, the
  same layering as `windows`): for example `game_mode`, `background_capture` (Game DVR historical
  capture), `windowed_game_optimizations`, `power_mode` (Windows' own efficiency/balanced/performance
  overlay, restoring the user's mode), and Explorer speed settings that are real and per-user
  (`folder_type_discovery` off for big Downloads-style folders, `separate_process` for folder
  windows, `show_extensions`, `show_hidden`). Pick only the ones the research or Microsoft docs
  support; write the source next to each in `docs/system.md`.
- Every change journaled through the existing undo journal (value, type, absence), applied live
  where Windows supports it (broadcasts, Explorer refresh), undone when the field is removed, on
  panic hotkey, on quit and after a crash, exactly like the `windows` section.
- Admin-only items (HAGS, long paths, Defender exclusions, Dev Drive creation, MPO): **don't apply
  them**. List them in `docs/system.md` with what they do and why they need admin, and add
  launcher entries that open the right Settings page (`ms-settings:` URIs) for each: Graphics,
  Gaming, Captures, Power, Dev Drive (`ms-settings:disksandvolumes`), For developers.
- A "game is running" signal for the rest of Glaze: a small pure function plus a watcher in
  `crates/tweaks` (or a new tiny module in `crates/desktop` if it fits better, additive) based on
  `SHQueryUserNotificationState` (`QUNS_RUNNING_D3D_FULL_SCREEN`, `QUNS_BUSY`,
  `QUNS_PRESENTATION_MODE`) and the foreground window covering a monitor, published so other
  processes can pause; event-driven, no polling loop. Don't wire it into the shell, plugins or
  engine yet (other agents own those); document the API in `docs/system.md`. A later round uses it.

## You own

`crates/tweaks` (additive: new modules; don't change the `windows` styling behaviour), additive fields
in `crates/config`, the Settings-page entries in the launcher's Settings vocabulary in
`crates/shell/src/flyouts` (additive), core's call site to apply the `system` section
(`crates/core`, one small commit), and a new `docs/system.md`. Not `crates/studio`.

## How

- Unit tests for the plan/undo logic of each setting (pure: desired vs current -> writes; undo
  restores type and absence).
- Live: apply each setting on, verify with `reg query` and the Settings page state, then undo and
  verify the original values came back byte for byte. Leave the user's machine exactly as it was
  (don't keep any setting on).
- `CARGO_TARGET_DIR=C:\glaze-targets\system-tweaks` for check/test.

## Stop when

Done above, or after 2 hours. Commit as you go.

## Report

The settings with sources, what's admin-only, the game signal API, test/live evidence, commits.
