# Brief: bars-parity round 2 (send the new glass knobs, close C06-C14)

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md`, `docs/bars.md` (round 1 handoff, b727049),
`docs/glass.md` › "Finer knobs on the wire" (engine commits e351d93, a28aae2, c955673, 9e0a0db), the
shell-idle section of `docs/performance.md` (don't undo its wins), and the partial rows C06, C07, C10,
C12, C13, C14 in `plans/native/parity.md`.

## Goal

The engine now accepts per-plate and per-overlay glass style: `brightness`, `saturation`, `smoothing`
(continuous corners), `sheen`, `depth`, `fade_ms` (0 for reduced motion), and blur up to 256 physical px.
Send them from the shell (bars, dock, flyouts, menus, widgets where they share the look) so every
remaining partial setting does what it says, and close the six rows with evidence:

- plates: `writer.publish_styled(&plates, &styles)`; overlays: `Request::Style` after every `Open`;
- styles are resolved once per config/look change and cached per surface, never per frame or per publish;
- continuous corners: the surface (kit drawing, hit regions) must draw the same squircle the glass uses;
- reduced motion / motion off: `fade_ms = 0`;
- `depth`: the engine agent mapped it to the soft shade under the lit top edge. If the bars' meaning
  ("inner shadow toward the bottom") doesn't match, say so in the report instead of faking it.

Idle cost must not move: measure the shell before and after (3 settled samples each, `measure.ps1`),
idle CPU and private MB within noise.

## You own

The bars/flyouts/menus/widgets look code in `crates/shell`, `crates/bars`, `crates/theme`; additive in
`crates/ui`. Not `crates/wallpaper` / `glass-ipc` (the engine-perf-2 agent owns them; if the wire is
missing something, message the orchestrator in your report). Not `crates/studio`.

## How / stop

As round 1: DPI-aware captures of a couple of variants in a copy of `bars.json`, restoring the user's
file byte for byte (hash before/after), desktop lock one action at a time, deploy live.
`CARGO_TARGET_DIR=C:\mypaper-targets\bars-parity`. Stop when the six rows are closed, or after 2 hours.
Report: rows closed with evidence, measurements, commits.
