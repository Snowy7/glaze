# Effect lab

Offline DX12 rendering: no windows, input events, live wallpaper changes or desktop hooks.
The lab loads packages with `mypaper_theme::stack::LoadedEffect`, uses the engine's
`pass_wgsl` builder and `params_bytes`, and validates every pass with the workspace's naga 30.

From the repository root:

```powershell
$env:CARGO_TARGET_DIR = 'C:/mypaper-targets/effects-lab'
cargo run --manifest-path effects/tools/Cargo.toml
cargo run --manifest-path effects/tools/Cargo.toml -- --render --update-costs
cargo run --manifest-path effects/tools/Cargo.toml -- --check-finite
cargo run --manifest-path effects/tools/Cargo.toml -- --check-finite --depth-ramp --only dof,fog,parallax
cargo run --manifest-path effects/tools/Cargo.toml -- --check-bloom-identity
cargo run --manifest-path effects/tools/Cargo.toml -- --missing-previews --out C:/mypaper-targets/effects-sheet/engine
py effects/tools/contact_sheet.py C:/mypaper-targets/effects-sheet
```

`--render` saves 1920×1080 photo renders, 320×180 package previews and `timings.json`.
`--photo <path>` selects a real photograph; `--out <folder>` changes the capture directory.
`--only <id,id>` limits rendering, while still validating every package. `--no-previews`
preserves the gallery thumbnails when capturing an additional photograph or depth fixture.
`--clock-hours <0..24>` supplies the real-clock prelude field for deterministic captures
(default 17.5, dusk). The live engine supplies the actual local hour.
`contact_sheet.py` requires Pillow and produces a master sheet plus six category sheets.

`--check-finite` renders every pass to RGBA16F and rejects NaN/infinity components. It checks
defaults, all float minima/maxima, both booleans, black/white colors and every enum option.
`--depth-ramp` supplies a synthetic near-to-far depth texture instead of the flat no-depth
sentinel. Lightning and glitch previews capture an active event at default intensity; their
gallery images intentionally show the effect, rather than a long quiet interval.

Measurements use the integrated DX12 adapter when available: three warmup frames, then
the median of 24 frames of GPU timestamps, summed over every pass at its declared scale.
They include effect render passes; upload, readback, stack mixing and presentation are excluded.
Only `--update-costs` writes manifests. A discrete-GPU fallback leaves `cost_estimated: true`.
The engine agent's rain, blur, color and vignette packages are validated by default.
`--missing-previews` explicitly adds any absent thumbnails, including those reference packages;
their manifests, costs and shaders are never modified by the tool. Grain is owned and measured here.

All library shaders are original MIT-licensed mypaper code; the package URL identifies the source.
Each package's `CREDITS.txt` carries the MIT notice and its preview's separate photo attribution.
The curated sources are the CC BY skyline and forest and public-domain NASA photographs.
No upstream rain or snow shader is used. `measurements.json` records the
checked-in default costs from the iGPU runs, including targeted remeasurements after revisions.

`time-of-day` uses `paper.clock_hours`, supplied by the engine, including after pause/resume.
The effect never substitutes elapsed animation time for the wall clock. Still effects retain
`animated: false`, so the engine can bake an entirely static stack or its static prefix.

`--check-bloom-identity` sets bloom strength to zero, supplies a different untouched base,
and compares the GPU output to the sharp preceding layer input with a one-byte tolerance.
Bloom blurs highlights in two half-scale Gaussian passes, then composites over `layer_input`.

## Curated gallery previews

`gallery.json` records a recipe for every card: source photograph, crop, representative time,
clock hour and any showcase parameter overrides. Most cards use the effect defaults. Light
positions are aligned to the source; Film looks demonstrates its noir option; reference
effects use showcase settings without changing their manifests or shaders. A `detail`
crop is taken after rendering, so fine grain and sharpening remain visible in a thumbnail.
Each owned package retains the software license and the correct source-photo credit.

```powershell
cargo run --manifest-path effects/tools/Cargo.toml -- --render --include-reference --no-previews --update-costs --out C:/mypaper-targets/effects-sheet/v2-city
py effects/tools/render_gallery.py --lab C:/mypaper-targets/effects-lab/debug/mypaper-effects-lab.exe --ffmpeg <ffmpeg.exe>
py effects/tools/contact_sheet.py C:/mypaper-targets/effects-sheet/v2-city --output C:/mypaper-targets/effects-sheet/contact-sheet-v2.jpg
```

Gallery generation writes all 38 `preview.jpg` files at 320 x 180, a `preview-grid-v2.jpg`
beside the captures, and full-resolution renders plus input photos and recipes in `gallery-v2/`.
`--only id,id` iterates a few cards; `--grid-only` rebuilds the grid from the checked-in images.
Use the city, forest and daytime captures to judge defaults separately from showcase presets.
All captures and measurements stay offscreen and never start the live wallpaper engine.

The lab's `--include-reference` explicitly includes rain, blur, color and vignette for
rendering or finite checks, while `--update-costs` still never edits those four manifests.
`--params '{...}'` supplies showcase overrides; combining nonempty overrides with
`--update-costs` is rejected so published costs always describe defaults. `--time <seconds>`
pins animation time, independent of `--clock-hours`. JPEG captures use quality 95.

`--mouse X,Y` supplies normalized cursor coordinates without moving the real cursor. Gallery
recipes can provide a second `motion` frame (time and/or mouse) and blend the two real renders
into a still double exposure. Cursor depth, Gentle wind and Slow camera drift use this to show
displacement. Lightning uses `--event-peak`: its representative time is computed with the GPU's
own hash, avoiding CPU/GPU floating-point differences that can miss a short flash. Event peaks
are showcase captures and cannot be used with `--update-costs`.

Snow generates three independently drifting depth planes at half resolution and composites the
flake field over the sharp layer input. Grain's default is fine monochrome noise, with less weight
in deep shadows and bright highlights. Both costs are measured at defaults on the integrated GPU.

Clouds computes its fractal field at quarter resolution and composites over the sharp input.
Dream glow uses two quarter-resolution Gaussian passes and a full-resolution screen blend.
God-rays integrates 32 samples at quarter resolution, then diffuses the light field over the
sharp input at full resolution.
Paint uses its original Kuwahara filter at half resolution, then blends at full resolution.
All three composite over `layer_input`, preserving the sharp input at zero strength.
