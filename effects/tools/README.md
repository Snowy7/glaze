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
The engine agent's rain, blur, color, vignette and grain packages are validated by default.
`--missing-previews` explicitly adds any absent thumbnails, including those reference packages;
their manifests, costs and shaders are never modified by the tool.

All library shaders are original MIT-licensed mypaper code; the package URL identifies the source.
Each package's `CREDITS.txt` carries the MIT notice and separate CC BY 3.0 photo attribution
for its preview. No upstream rain or snow shader is used. `measurements.json` records the
checked-in costs from the final full-library iGPU run.

`time-of-day` uses `paper.clock_hours`, supplied by the engine, including after pause/resume.
The effect never substitutes elapsed animation time for the wall clock. Still effects retain
`animated: false`, so the engine can bake an entirely static stack or its static prefix.

`--check-bloom-identity` sets bloom strength to zero, supplies a different untouched base,
and compares the GPU output to the sharp preceding layer input with a one-byte tolerance.
Bloom blurs highlights in two half-scale Gaussian passes, then composites over `layer_input`.
