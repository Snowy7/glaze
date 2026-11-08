# Effect lab

Offline DX12 rendering: no windows, input events, live wallpaper changes or desktop hooks.
The lab loads packages with `mypaper_theme::stack::LoadedEffect`, uses the engine's
`pass_wgsl` builder and `params_bytes`, and validates every pass with the workspace's naga 30.

From the repository root:

```powershell
$env:CARGO_TARGET_DIR = 'C:/mypaper-targets/effects-lab'
cargo run --manifest-path effects/tools/Cargo.toml
cargo run --manifest-path effects/tools/Cargo.toml -- --render --update-costs
py effects/tools/contact_sheet.py C:/mypaper-targets/effects-sheet
```

`--render` saves 1920×1080 photo renders, 320×180 package previews and `timings.json`.
`--photo <path>` selects a real photograph; `--out <folder>` changes the capture directory.
`contact_sheet.py` requires Pillow and produces a master sheet plus six category sheets.

Measurements use the integrated DX12 adapter when available: three warmup frames, then
the median of 24 frames of GPU timestamps, summed over every pass at its declared scale.
They include effect render passes; upload, readback, stack mixing and presentation are excluded.
Only `--update-costs` writes manifests. A discrete-GPU fallback leaves `cost_estimated: true`.
The engine agent's rain, blur, color, vignette and grain packages are validated but never modified.

All library shaders are original MIT-licensed mypaper code; the package URL identifies the source.

## Required prelude addition

`time-of-day` needs `paper.clock_hours`: the real local hour including fractional minutes,
updated by the engine, including after pause/resume. Animated wallpaper elapsed time cannot
substitute for the wall clock. The current prelude has no clock field; this library does not
modify the engine-owned prelude.
