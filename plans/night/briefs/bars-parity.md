# Brief: bars-parity

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md`, `docs/bars.md`, `docs/ui.md` (The kit),
`docs/glass.md`, and the **partial** rows in `plans/native/parity.md`: C06, C07, C09, C10, C12,
C13, C14, C15, C22, C25.

## Goal

Close those rows on the native shell so every bar/dock/flyout/menu style and motion setting in
`bars.json` and `theme.json` does what it says again: appearance (surface, tint, opacity, blur,
radius, border, text, accent, font, scale, shadow), background modes, light/dark scheme tokens,
saturation, shape, per-state item styling, glass parameters, motion (enabled, reduce-motion, curves,
durations, styles, the hover/press/open/magnify/bounce/attention leaves) reaching popups and menus
too, theme-layer resets, and the one-switch style selection (C25). Respect the approved cuts (no
neon/glow/gradients/noise, no UI outlines on glass, macOS-calm restraint: see the
`visual-restraint` and `liquid-glass` notes summarised in `docs/glass.md`).

Every change must stay free at idle: a style setting is resolved once on config change, never per
frame. Motion runs only while something moves.

## You own

`crates/bars`, `crates/theme`, the bar/flyout/menu parts of `crates/shell`, additive changes to
`crates/ui` (new tokens/helpers only, never an existing signature or look; see LIVE.md), and the
C rows of `plans/native/parity.md` plus `docs/bars.md`. Not `crates/studio` (another thread owns it):
list any Studio control a row needs under "Studio follow-ups" in the night plan instead.
`crates/wallpaper` belongs to the engine agent tonight.

## How

- One row (or a tight group) per commit, with tests where the logic is pure (config resolution,
  precedence, curve math).
- Deploy live and look at DPI-aware captures of the bars with a couple of settings changed in a
  copy of `%APPDATA%\glaze\bars.json`; restore the user's own `bars.json` exactly afterwards
  (back it up first, compare bytes after).
- Use `CARGO_TARGET_DIR=C:\glaze-targets\bars-parity` for check/test; `C:\glaze-targets\main`
  only for the deploy build.
- Measure the shell before and after (`measure.ps1`, path in the night plan): idle shell CPU must
  stay ~0 and private memory must not grow by more than 1 MB.

## Stop when

All ten rows are done (mark them in `parity.md` with evidence), or after 2.5 hours. Commit as you go.

## Report

Rows closed, what each setting now does, measurements, Studio follow-ups added, commits.
