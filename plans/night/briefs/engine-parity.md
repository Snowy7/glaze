# Brief: engine-parity (the last three glass rows: C10, C12, C14)

Read `plans/native/LIVE.md`, `docs/glass.md` (incl. "Finer knobs on the
wire"), `docs/bars.md` from line 185 (bars-parity-2 handoff), and rows C10, C12, C14 in
`plans/native/parity.md`. You own `crates/wallpaper`, `crates/glass-ipc`, `crates/liquid-rust` and the
engine docs. Additive wire changes only (old shells and engines must keep working, as in 9e0a0db).

## Gaps (from the bars-parity-2 handoff)

- **C10:** the overlay's circular compositor clip trims squircle (continuous-corner) geometry: the glass
  over apps must clip to the same continuous shape the plate and the kit surface use.
- **C12:** `depth` shades below the lit top rim; the bars' legacy contract is an inner shadow toward the
  bottom edge. Add it as its own field (e.g. `inner_shadow`, 0..=1, unset = off), resolved once per
  plate/overlay, never per frame, skipped in the shader at 0. Keep `depth` as is.
- **C14:** glass fades cap at 2,000 ms but the bars accept motion up to 5,000 ms, and the fade is always
  cubic ease-out: accept up to 5,000 ms and a curve (the bars' named curves / cubic Bézier), and keep
  `fade_ms = 0` as instant for reduced motion.

Then wire the new fields from the shell (`Tokens::glass_style`, `GlassStyle` in `crates/ui` /
`crates/shell`: small additive edits, committed separately) so the rows close end to end, and mark the
rows done in `parity.md` with evidence.

## Rules

Default values must render pixel-identical (prove with the lab, as round 1 did) and cost no GPU time.
**No deploy-local.ps1 or core restart between 06:40 and 07:05 local** (index measurement window);
check the clock right before deploying. Restarting only the engine or shell is fine outside it too.
`CARGO_TARGET_DIR=C:\glaze-targets\engine-perf`. Stop when the three rows are closed, or after 2 hours.
Report: what changed, evidence, commits.
