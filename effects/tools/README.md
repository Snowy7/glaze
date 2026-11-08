# Effect tools

There is one renderer: the engine. Every capture, preview and check below goes through the
engine's real stack path (`StackSource`, the same code Studio's `--preview` child and the
desktop run, compiled with the engine's FXC), so what you see here is what users see.

The private offline lab that used to live here was retired on 2026-11-08: it compiled with a
different DX12 compiler and missed a black `crt`, among others. Its checks now live in
`crates/wallpaper/src/sources/stack/effects_real.rs`.

From the repository root (`$env:CARGO_TARGET_DIR = 'C:\glaze-targets\main'`):

```powershell
# Every effect over a photo at 1920x1080 and 2560x1440: not black, not flat, still the photo,
# visibly changed; NaN/inf at every param extreme (RGBA16F); bloom identity; Studio's params.
cargo test -p glaze-wallpaper --bins effects_real -- --test-threads=1 --nocapture

# The same, also writing captures, then a labeled contact sheet (requires Pillow).
$env:GLAZE_EFFECTS_REAL = 'C:/glaze-targets/effects-real/after'
cargo test -p glaze-wallpaper --bins every_effect_keeps -- --test-threads=1
py effects/tools/contact_sheet.py C:/glaze-targets/effects-real/after/2560x1440 --output C:/glaze-targets/effects-real/contact.jpg

# Regenerate all preview.jpg cards from gallery.json recipes (one engine run).
py effects/tools/render_gallery.py [--only id,id] [--grid-only]

# GPU cost at 1920x1080 on the iGPU (the `cost` in effect.json and measurements.json).
$env:GLAZE_EFFECTS_ONLY = 'crt,halftone'
cargo test -p glaze-wallpaper --release --bins time_effects -- --ignored --nocapture
```

`GLAZE_EFFECTS_ONLY=id,id` limits the tests and the timer to some effects. A dev-build test
that fails aborts with 0xe06d7363; rerun with `--test-threads=1`.

## Curated gallery previews

`gallery.json` records a recipe for every card: source photograph, crop, representative time,
clock hour, cursor and any showcase parameter overrides. Most cards use the effect defaults.
`render_gallery.py` writes each card's 1920x1080 input and `job.json` under `--out`
(default `C:/glaze-targets/effects-sheet/gallery-v3`), runs the engine's ignored
`gallery_renders` test once over all of them, then crops (`detail`), blends `motion` frames into
a double exposure and saves the 320x180 `preview.jpg`, the package's `CREDITS.txt` photo credit
and `preview-grid-v3.jpg`. Video sources (the daytime Earth frame) are extracted once with
ffmpeg into `--frames` and reused.

Lightning and glitch cards aim at an event with the prelude's hash evaluated on the CPU; when
that misses the GPU's short flash, pin a `time` in the recipe.

All library shaders are original MIT-licensed Glaze code; the package URL identifies the source.
Each package's `CREDITS.txt` carries the MIT notice and its preview's separate photo attribution.
The curated sources are the CC BY skyline and forest and public-domain NASA photographs.
`measurements.json` records the checked-in default costs from the iGPU runs.

`time-of-day` uses `paper.clock_hours`, supplied by the engine (the tests pin it). Still effects
keep `animated: false`, so the engine can bake an entirely static stack or its static prefix.
