//! Every bundled effect through the engine's real stack path (the one Studio's `--preview`
//! child and the desktop use), over a photo, at 1920x1080 and 2560x1440. The effects lab used
//! to render with its own harness, which compiled with a different DX12 compiler than the
//! engine's FXC, so a black `crt` looked right there; these tests leave no second renderer.
//!
//! `cargo test -p glaze-wallpaper --bins effects_real -- --test-threads=1 --nocapture`
//!
//! - `GLAZE_EFFECTS_REAL=<dir>` also writes `<dir>/<w>x<h>/<id>.jpg`, `before.jpg` and
//!   `report.json` (for `effects/tools/contact_sheet.py`); `GLAZE_EFFECTS_ONLY=id,id` limits it.
//! - `GLAZE_GALLERY=<dir>` with the ignored `gallery_renders` renders the jobs that
//!   `effects/tools/render_gallery.py` prepared (the effects' `preview.jpg` cards).

use std::sync::atomic::Ordering;

use super::super::blit::Blitter;
use super::super::stack_preview;
use super::tests::repo;
use super::*;

/// The photo every effect runs over (also the gallery's "city" source).
const THEME: &str = "themes/rain-city-amber";
const PHOTO: &str = "media/city.jpg";
const SIZES: [(u32, u32); 2] = [(1920, 1080), (2560, 1440)];
/// Seconds into an animated effect (past any fade-in).
const CLOCK: f32 = 12.0;

/// A stack over `photo` (relative to the city theme, or absolute) with `layers` (JSON,
/// comma-separated).
fn stack_over(gpu: &Gpu, photo: &str, layers: &str) -> StackSource {
    let json = serde_json::json!({ "name": "t", "wallpaper": { "kind": "stack",
        "base": { "kind": "image", "source": photo },
        "layers": serde_json::from_str::<Value>(&format!("[{layers}]")).unwrap() } });
    let theme = LoadedTheme::parse(repo().join(THEME), &json.to_string()).unwrap();
    let glaze_theme::Wallpaper::Stack(config) = &theme.theme.wallpaper else {
        unreachable!()
    };
    let source = StackSource::new_in(gpu, &theme, config, &[repo().join("effects")]).unwrap();
    assert!(source.errors.is_empty(), "{:?}", source.errors);
    source
}

use serde_json::Value;

fn photo_stack(gpu: &Gpu, layers: &str) -> StackSource {
    stack_over(gpu, PHOTO, layers)
}

/// One frame as Studio's preview child draws it (cursor at the center).
fn frame(gpu: &Gpu, source: &mut StackSource, size: (u32, u32)) -> Vec<u8> {
    let center = [size.0 as f32 / 2.0, size.1 as f32 / 2.0];
    stack_preview::render(gpu, source, size, Duration::from_secs_f32(CLOCK), 360, center)
}

/// An effect layer at its defaults, or with `params`.
fn layer(id: &str, params: Value) -> String {
    let params = if params.is_null() { serde_json::json!({}) } else { params };
    serde_json::json!({ "kind": "effect", "effect": id, "params": params }).to_string()
}

/// Every bundled effect id, sorted (only `GLAZE_EFFECTS_ONLY`'s when set).
fn effect_ids() -> Vec<String> {
    let only = std::env::var("GLAZE_EFFECTS_ONLY").unwrap_or_default();
    let mut ids: Vec<String> = std::fs::read_dir(repo().join("effects"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().join(stack::EFFECT_MANIFEST).is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|id| only.is_empty() || only.split(',').any(|o| o == id))
        .collect();
    ids.sort();
    assert!(!ids.is_empty(), "no effects");
    ids
}

/// COM for WIC (decoding the photo, encoding the dumps) on this test's thread.
fn com() {
    // SAFETY: no preconditions; a second call on the thread is harmless.
    let _ = unsafe {
        windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        )
    };
}

/// Per-pixel luma (0..1) of tightly packed BGRA.
fn lumas(bgra: &[u8]) -> Vec<f32> {
    bgra.chunks(4)
        .map(|p| {
            (0.0722 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.2126 * f32::from(p[2]))
                / 255.0
        })
        .collect()
}

/// Mean luma of each cell of a `cols` x `rows` grid.
fn grid(luma: &[f32], size: (u32, u32), cols: usize, rows: usize) -> Vec<f32> {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let mut sums = vec![0.0f64; cols * rows];
    let mut counts = vec![0u32; cols * rows];
    for (i, l) in luma.iter().enumerate() {
        let cell = (i / w) * rows / h * cols + (i % w) * cols / w;
        sums[cell] += f64::from(*l);
        counts[cell] += 1;
    }
    sums.iter()
        .zip(&counts)
        .map(|(s, &c)| (*s / f64::from(c.max(1))) as f32)
        .collect()
}

fn mean(v: &[f32]) -> f32 {
    (v.iter().map(|x| f64::from(*x)).sum::<f64>() / v.len().max(1) as f64) as f32
}

fn std_dev(v: &[f32]) -> f32 {
    let m = mean(v);
    (v.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / v.len().max(1) as f32).sqrt()
}

/// Pearson correlation of two equally long series.
fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let (ma, mb) = (mean(a), mean(b));
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        ab += (x - ma) * (y - mb);
        aa += (x - ma) * (x - ma);
        bb += (y - mb) * (y - mb);
    }
    ab / (aa * bb).sqrt().max(1e-9)
}

/// Mean absolute change per channel between two BGRA frames, 0..1.
fn change(a: &[u8], b: &[u8]) -> f32 {
    let sum: u64 = a.iter().zip(b).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    sum as f32 / a.len() as f32 / 255.0
}

/// What one effect did to the photo at one size.
#[derive(Debug, Clone, Copy)]
struct Stats {
    mean: f32,
    /// Spread of luma over a 32x18 grid of cell means: 0 is flat.
    spread: f32,
    /// Correlation of that grid with the photo's: the image is still there.
    correlation: f32,
    /// Mean absolute change per channel from the photo, 0..1.
    change: f32,
    /// Share of pixels changed by more than 6 levels in some channel (sparse snow, sparks).
    touched: f32,
}

fn stats(out: &[u8], base: &[u8], size: (u32, u32)) -> Stats {
    let (lo, lb) = (lumas(out), lumas(base));
    let (go, gb) = (grid(&lo, size, 32, 18), grid(&lb, size, 32, 18));
    Stats {
        mean: mean(&lo),
        spread: std_dev(&go),
        correlation: correlation(&go, &gb),
        change: change(out, base),
        touched: out
            .chunks(4)
            .zip(base.chunks(4))
            .filter(|(a, b)| (0..3).any(|c| a[c].abs_diff(b[c]) > 6))
            .count() as f32
            / lo.len() as f32,
    }
}

/// What an effect at its defaults must still do over a photo, at every size.
fn problems(id: &str, s: &Stats, base: &Stats) -> Vec<String> {
    let mut out = Vec::new();
    if s.mean < base.mean * 0.35 {
        out.push(format!("near black (mean {:.3} vs {:.3})", s.mean, base.mean));
    }
    if s.spread < base.spread * 0.3 {
        out.push(format!("flat (spread {:.3} vs {:.3})", s.spread, base.spread));
    }
    if s.correlation < 0.6 {
        out.push(format!("photo lost (correlation {:.2})", s.correlation));
    }
    // Lightning and glitch strike now and then; the rest must visibly change the photo.
    if s.change < 0.004 && s.touched < 0.01 && !["lightning", "glitch"].contains(&id) {
        out.push(format!("no visible change ({:.4}, {:.3} touched)", s.change, s.touched));
    }
    out
}

#[test]
fn every_effect_keeps_the_photo_through_the_engine_at_1080p_and_1440p() {
    com();
    let gpu = Gpu::headless().expect("a DX12 GPU");
    let dump = std::env::var_os("GLAZE_EFFECTS_REAL").map(PathBuf::from);
    let ids = effect_ids();
    let mut failures = Vec::new();
    let mut report = serde_json::Map::new();
    for size in SIZES {
        let dir = dump
            .as_ref()
            .map(|d| d.join(format!("{}x{}", size.0, size.1)));
        let base = frame(&gpu, &mut photo_stack(&gpu, ""), size);
        let base_stats = stats(&base, &base, size);
        if let Some(dir) = &dir {
            crate::snapshot::save_jpeg(&gpu, size, &base, &dir.join("before.jpg")).unwrap();
        }
        for id in &ids {
            let out = frame(&gpu, &mut photo_stack(&gpu, &layer(id, Value::Null)), size);
            let s = stats(&out, &base, size);
            let found = problems(id, &s, &base_stats);
            eprintln!(
                "{}x{} {id:>12}: mean {:.3} spread {:.3} corr {:.2} change {:.4} touched {:.3} {}",
                size.0,
                size.1,
                s.mean,
                s.spread,
                s.correlation,
                s.change,
                s.touched,
                found.join("; ")
            );
            if let Some(dir) = &dir {
                crate::snapshot::save_jpeg(&gpu, size, &out, &dir.join(format!("{id}.jpg")))
                    .unwrap();
            }
            report.insert(
                format!("{id}@{}x{}", size.0, size.1),
                serde_json::json!({ "mean": s.mean, "spread": s.spread,
                    "correlation": s.correlation, "change": s.change, "touched": s.touched, "problems": found }),
            );
            failures.extend(
                found
                    .into_iter()
                    .map(|p| format!("{id} at {}x{}: {p}", size.0, size.1)),
            );
        }
    }
    if let Some(dump) = &dump {
        let json = serde_json::to_string_pretty(&report).unwrap();
        std::fs::write(dump.join("report.json"), json).unwrap();
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The engine's device rendering every stack texture as RGBA16F, so a NaN or infinity stays
/// visible instead of turning black in a unorm target.
fn float_gpu() -> Gpu {
    let mut gpu = Gpu::headless().expect("a DX12 GPU");
    gpu.format = wgpu::TextureFormat::Rgba16Float;
    gpu.blit = Blitter::new(&gpu.device, gpu.format);
    gpu
}

/// Defaults, every float at its minimum, at its maximum, both bools, black and white colors,
/// and every enum option.
fn param_cases(manifest: &stack::EffectManifest) -> Vec<Value> {
    use glaze_theme::stack::ParamKind;
    let mut cases = vec![serde_json::json!({}), serde_json::json!({}), serde_json::json!({})];
    for p in &manifest.params {
        let (low, high) = match &p.kind {
            ParamKind::Float { min, max, .. } => (serde_json::json!(min), serde_json::json!(max)),
            ParamKind::Bool { .. } => (Value::Bool(false), Value::Bool(true)),
            ParamKind::Color { .. } => (serde_json::json!("#000000"), serde_json::json!("#ffffff")),
            ParamKind::Enum { options, .. } => {
                for option in options {
                    cases.push(serde_json::json!({ (p.id.clone()): option }));
                }
                continue;
            }
        };
        cases[1][&p.id] = low;
        cases[2][&p.id] = high;
    }
    cases
}

#[test]
fn no_effect_writes_nan_or_infinity_at_any_param_extreme() {
    com();
    let gpu = float_gpu();
    let size = (480, 270);
    let mut failures = Vec::new();
    for id in effect_ids() {
        let effect = stack::locate_effect_in(&id, &[repo().join("effects")]).unwrap();
        for params in param_cases(&effect.manifest) {
            let mut source = photo_stack(&gpu, &layer(&id, params.clone()));
            let texels = frame(&gpu, &mut source, size);
            let bad = texels
                .as_chunks::<2>()
                .0
                .iter()
                .filter(|h| u16::from_le_bytes(**h) & 0x7c00 == 0x7c00)
                .count();
            if bad > 0 {
                failures.push(format!("{id} {params}: {bad} non-finite components"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn bloom_at_zero_leaves_the_sharp_input_untouched() {
    com();
    let gpu = Gpu::headless().expect("a DX12 GPU");
    let size = (960, 540);
    let base = frame(&gpu, &mut photo_stack(&gpu, ""), size);
    let layer = layer("bloom", serde_json::json!({ "amount": 0 }));
    let out = frame(&gpu, &mut photo_stack(&gpu, &layer), size);
    let worst = out.iter().zip(&base).map(|(a, b)| a.abs_diff(*b)).max();
    assert!(worst <= Some(2), "zero bloom changed the photo by {worst:?} levels");
}

/// Studio writes `params` as JSON: colors as "#rrggbb", bools as booleans and enums as their
/// option names. Each must reach the engine and change the frame the way it says.
#[test]
fn params_as_studio_writes_them_change_the_render() {
    com();
    let gpu = Gpu::headless().expect("a DX12 GPU");
    let size = (960, 540);
    let render = |id: &str, params: Value| {
        frame(&gpu, &mut photo_stack(&gpu, &layer(id, params)), size)
    };
    let mean_bgr = |bgra: &[u8]| -> [f32; 3] {
        let mut sum = [0.0f64; 3];
        for p in bgra.chunks(4) {
            for c in 0..3 {
                sum[c] += f64::from(p[c]);
            }
        }
        sum.map(|s| (s / (bgra.len() / 4) as f64 / 255.0) as f32)
    };

    // Color: a red rain tint warms the fogged glass.
    let plain = mean_bgr(&render("rain", serde_json::json!({})));
    let red = mean_bgr(&render("rain", serde_json::json!({ "tint": "#ff0000" })));
    assert!(red[2] - red[0] > plain[2] - plain[0] + 0.01, "{plain:?} -> {red:?}");

    // Bool: rain without streaks draws a different frame.
    let streaks = render("rain", serde_json::json!({}));
    let none = render("rain", serde_json::json!({ "streaks": false }));
    assert!(change(&streaks, &none) > 0.0005, "streaks: false changed nothing");

    // Enum: "noir" (an option name) drains the color out.
    let chroma = |bgra: &[u8]| -> f32 {
        let c: f32 = bgra
            .chunks(4)
            .map(|p| f32::from(p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2])))
            .sum();
        c / (bgra.len() / 4) as f32 / 255.0
    };
    let teal = chroma(&render("looks", serde_json::json!({})));
    let noir = chroma(&render("looks", serde_json::json!({ "look": "noir" })));
    assert!(noir < teal * 0.5, "noir chroma {noir:.3} vs default {teal:.3}");
}

/// Renders the gallery jobs `effects/tools/render_gallery.py` prepared: each folder of
/// `GLAZE_GALLERY` with a `job.json` ({ effect, params, time, mouse [x, y] in 0..1,
/// clock_hours, second: { time, mouse }? } over its `input.jpg`) gets `<effect>.jpg` at
/// 1920x1080, and `<effect>-second.jpg` for a second frame.
#[test]
#[ignore]
fn gallery_renders() {
    com();
    let gpu = Gpu::headless().expect("a DX12 GPU");
    let size = (1920, 1080);
    let root = PathBuf::from(std::env::var_os("GLAZE_GALLERY").expect("GLAZE_GALLERY"));
    for entry in std::fs::read_dir(&root).unwrap().flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path().join("job.json")) else {
            continue;
        };
        let job: Value = serde_json::from_str(&text).unwrap();
        let id = job["effect"].as_str().unwrap();
        let input = entry.path().join("input.jpg");
        let hours = job["clock_hours"].as_f64().unwrap_or(17.5) as f32;
        TEST_CLOCK_HOURS.store(hours.to_bits(), Ordering::Relaxed);
        let mut source = stack_over(
            &gpu,
            &input.to_string_lossy(),
            &layer(id, job["params"].clone()),
        );
        let mut shoot = |spec: &Value, name: String| {
            let time = spec["time"].as_f64().unwrap_or(f64::from(CLOCK)) as f32;
            let mouse = spec["mouse"].as_array().map_or([0.63, 0.42], |m| {
                [m[0].as_f64().unwrap() as f32, m[1].as_f64().unwrap() as f32]
            });
            let at = [mouse[0] * size.0 as f32, mouse[1] * size.1 as f32];
            let clock = Duration::from_secs_f32(time);
            let out = stack_preview::render(&gpu, &mut source, size, clock, 360, at);
            crate::snapshot::save_jpeg(&gpu, size, &out, &entry.path().join(name)).unwrap();
        };
        shoot(&job, format!("{id}.jpg"));
        if job["second"].is_object() {
            shoot(&job["second"], format!("{id}-second.jpg"));
        }
        eprintln!("{id}");
    }
    TEST_CLOCK_HOURS.store(u32::MAX, Ordering::Relaxed);
}
