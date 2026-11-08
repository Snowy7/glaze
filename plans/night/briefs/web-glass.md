# Brief: web-glass (parity row P14) + covered-screen wakes

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md`, `docs/glass.md`, `docs/web-and-app-wallpapers.md`,
`docs/performance.md` ("Hidden = free"), and row P14 in `plans/native/parity.md`. You own
`crates/wallpaper`, `crates/glass-ipc`, `crates/liquid-rust`, the engine docs, and small additive
edits in `crates/shell` (bars/widgets glass requests; game-quiet is finishing in `crates/shell`,
so keep edits separate and commit at once).

## 1. P14: glass over web and app wallpapers

With a web or app wallpaper (a native window source), the engine skips plate rendering, and the shell
never asks for overlays there, so bars and widgets get no glass over those wallpapers. Make it work
like it does over shader/video/image wallpapers, without extra always-on cost:

- pick the cheapest correct route: the engine already has the overlay path (WGC capture with
  `WDA_EXCLUDEFROMCAPTURE`, see `overlay/capture.rs`) and a snapshot path (`snapshot.rs`). The
  wallpaper window is ours, so its content can be captured, or the web source can hand over frames;
- the refresh must follow the wallpaper's own frame changes (a still web page costs nothing; an
  animated one is limited to the theme fps), and it must stop when screens are covered, a game runs,
  or the source is hidden;
- test with the repo's web and app example themes (`themes/` and `themes/examples`), and look at DPI-aware
  captures of the bars and a widget over each. Put the user's theme (`rain-city`) back afterwards
  and check config bytes.

Close P14 in `parity.md` with evidence and measurements (engine CPU/private MB with a web wallpaper,
before/after).

## 2. Covered-screen wakes

The engine still wakes ~15-28 times/s on covered screens and while released (round 2), and costs
~0.16 % of a core during a game. Find each wake source and make covered/released/game states wake only
on real events (uncover, config, display change). If the remaining wakes are the wgpu device's own
driver thread, prove it and leave it.

## Rules

Interleaved measurements, one change per commit with numbers, quality unchanged, deploy only through
`scripts/deploy-local.ps1` from `C:\glaze-targets\main`. **No deploy, restart or desktop-lock use before
07:53 local** (the game-quiet measurement). `CARGO_TARGET_DIR=C:\glaze-targets\engine-perf`. Stop when
both are done, or after 2.5 hours. Report: what changed, evidence, numbers, commits.
