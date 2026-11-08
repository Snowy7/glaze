# Brief: style-sync (parity row C25: one switch styles everything)

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md`, `plans/native/parity.md` row C25 and its
links, `docs/windows-styling.md` (including "App styler"), `docs/bars.md` (the bars-parity handoff,
b727049), `docs/system.md`.

## Goal

Picking one style (liquid, mono, solid, and the planned fluent / oneui families) makes the whole desktop
agree, with one switch: the shell surfaces already follow it (bars-parity wired five native consumers);
what's missing is the Windows side. Window frames (border, caption, corners, backdrop), the system
accent, light/dark app and system mode, and accent on Start/taskbar/title bars (ColorPrevalence) should
follow the chosen style and the wallpaper palette, through the existing `windows` section machinery and
its undo journal. The user's explicit `look.windows` fields still win, field by field. Also feed the
same tokens to the app styler's `$accent` / `$dominant` values so Explorer and Settings match.

No new always-on cost: resolve on style/theme/palette change only.

## You own

`crates/tweaks` (the Windows styling side; `docs/system.md`'s settings belong to the system-tweaks work,
which is finished, so don't change their behaviour), additive in `crates/theme` and `crates/config`
(the app-styler round-2 agent also edits `crates/theme` additively: small commits), and the C25 row
in `plans/native/parity.md` plus `docs/windows-styling.md`. Not `crates/studio` (list the switch under
the night plan's Studio follow-ups), not `crates/shell` (owned by the shell-idle agent; if a shell
call site is needed, make it one small additive commit and say so).

## How

- Pure resolution (style + palette + user layer → Windows fields) with tests.
- Live: switch styles in a copy of the user's config, check title bars, accent and dark mode change
  and come back. Restore the user's `config.json` byte for byte afterwards (hash before and after).
  Desktop lock: one action at a time.
- `CARGO_TARGET_DIR=C:\glaze-targets\style-sync`.

## Stop when

C25 is done with evidence, or after 2 hours. Report: what each style sets on Windows, evidence, commits.
