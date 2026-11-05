# Engine notes

Findings from closing the known engine gaps: EXIF rotation, 10-bit/HDR video, memory,
Rust hot-patching. Each section ends with README-ready text.

## Memory

### Private memory on the integrated GPU (5 November 2026)

On the default low-power AMD iGPU, **every D3D12 heap is private memory of the engine**, and a
heap the engine releases is **not given back**: private bytes stay at their peak (allocating
the same sizes again reuses part of it, and grows the rest). Upload heaps are returned. So the
engine's private memory is its peak GPU allocation, and a texture made once at load and freed
costs as much as one kept forever. Reproduce with the ignored dev tests in `src/lab.rs`:
`lab::memory` (private bytes after each allocation step of a two-screen engine) and
`lab::memory_hints` (screen-sized textures allocated, freed and allocated again).

| Allocation | Before | After | Change |
|---|---:|---:|---|
| wgpu device | +188 MiB | **+97 MiB** | `max_non_sampler_bindings` 1,000,000 -> 16,384: wgpu's DX12 backend allocates its shader-visible view descriptor heap at that size up front (`gpu.rs`, `DESCRIPTORS`) |
| four 2560x1440 textures (56 MiB of pixels) | +114 MiB | **+60 MiB** | `MemoryHints::MemoryUsage` grew suballocation blocks from 8 to 64 MiB (124 MiB reserved); fixed 4 MiB blocks give every screen-sized texture a heap of its own size (`MEMORY_HINTS`) |
| backdrop pyramid, per screen | +32 MiB | **about +5 MiB, once for screens sharing a kept frame** | the 1/8 BMP pyramid copied each swapchain into a full-size texture first; it now shrinks the kept frame directly (`backdrop.rs`) |
| palette | +16 MiB | **0** | re-rendered the first frame at full screen size to shrink it to 64x36; it now shrinks the kept frame (`palette.rs`) |
| second screen's kept frame (rain) | +6 MiB | **0** | screens of one render size share one kept frame when the source ignores the screen and cursor (`Source::shared`) |
| rain's baked still frame | 0 | +6 MiB | one texture for both screens (`ShaderImage::bake`), the price of 0.8 ms of GPU per frame |

What remains, measured live (rain-city, two screens, see Native measurements): the D3D12
device on the iGPU itself (about 30 MiB with nothing else), what probing the other adapters
leaves behind (up to about 25 MiB: wgpu creates a D3D12 device on each RTX entry and WARP
before choosing; skipping them needs the single-adapter wgpu-hal patch planned for N5), wgpu's
own device state, and **the two swapchains, +63 and +40 MiB** for 2 x 2 x 14 MiB of pixels:
the extra is DXGI's presentation of an iGPU swapchain on monitors driven by the RTX, which
wgpu gives no control over. Swapchains already use the minimum two buffers.

### Earlier measurements

Measured with release builds on the dev laptop (AMD iGPU + NVIDIA dGPU, two 1440p monitors),
one fresh process per theme, read 7-9 s after start with PowerShell `WorkingSet64` and
`PrivateMemorySize64`. During these runs both monitors were covered by maximized windows, so
the engine was loaded and paused, which is also its most common state.

### Before and after

| Theme | Before: working set / private | After: working set / private | What moved it |
|---|---|---|---|
| `aurora` (shader) | 290 / 297 MB | **5 / 295 MB** | FXC (-7 MB), trim |
| `photo` (image) | 231 / 299 MB | **6 / 293 MB** | FXC (-5 MB), trim |
| `gallery` (slideshow) | 246 / 313 MB | **6 / 311 MB** | FXC (-8 MB), trim (±20 MB run to run: it picks different pictures) |
| 1080p video (earlier synthetic clip) | 373 / 420 MB | **7 / 418 MB** | FXC (-5 MB), trim |
| `islands` (3D scene) | 344 / 548 MB | **4 / 426 MB** | shared MSAA (-60 MB working set, -125 MB private), FXC (-7 MB), trim |

"Before" is the commit before these changes; "after" is the final build. Working set is what
Task Manager shows as memory; private is committed memory, which only changes when memory is
really released.

### The floor: a bare wgpu DX12 device and swapchain

`cargo run --release -p mypaper-wallpaper --example bare_wgpu -- fxc` creates a hidden window,
one 1440p swapchain, clears and presents one frame, then idles.

| | Working set | Private |
|---|---|---|
| bare wgpu, default compiler (`Auto`) | 182-204 MB | 237-246 MB |
| bare wgpu, FXC | 177-193 MB | 234-246 MB |
| engine, `aurora`, before | 290 MB | 297 MB |

The driver user-mode DLLs map 105 MB (`nvgpucomp64`), 86 MB (`nvwgf2umx`) and 77 MB
(`amdxc64`), and the loader pulls the NVIDIA ones into every D3D process on this machine.
(Correction, November 2026: most of this floor *was* ours to cut. The DLL images are not
private memory; wgpu's default 1M-entry descriptor heap was 92 MiB of it. See above.)

### What was cut

- **Shader compiler: FXC, set explicitly** (`gpu.rs`). wgpu's default `Dx12Compiler::Auto`
  loads any `dxcompiler.dll` it finds on the DLL search path and silently falls back to FXC.
  Here that was the Vulkan SDK's copy (21 MB image), so the same build behaved differently on
  each machine. FXC ships with Windows, compiles every shader we have (all five themes and the
  video shader run and pass their tests with it), and saves 5-8 MB of working set per theme.
- **Shared MSAA attachments** (`sources/scene.rs`). Each screen had its own 4x MSAA colour and
  depth targets, about 120 MB of GPU memory per 1440p screen. They are cleared when a pass
  starts and discarded when it ends, so screens with the same render size now draw through one
  set. Islands on two monitors: -125 MB private, -60 MB working set. A set is freed with the
  last screen that uses it.
- **Working-set trim** (`trim.rs`). Loading a theme touches memory that the running wallpaper
  never reads again (driver and compiler start-up, decoder set-up, the file just decoded). Two
  seconds after each theme load the process asks Windows to move its pages to the standby list
  (`SetProcessWorkingSetSize(-1, -1)`). On the bare example rendering at 60 fps, the working
  set goes from 199 MB to 11 MB and stays there, so the hot set really is that small; the
  frame after the trim takes about 1 ms longer (worst 1.3 ms against a 0.2 ms median, three
  runs), so there is no stutter. Pages come back by soft fault if they are touched again.
  This is *working set*, not committed memory: private bytes do not change.

### What was checked and left alone

- `MemoryHints::MemoryUsage` was already set (replaced by fixed 4 MiB blocks in November 2026,
  above). Swapchains already use the minimum two buffers
  (`desired_maximum_frame_latency = 1`). Image and slideshow uploads leave no staging memory
  behind (photo and aurora have the same private size).
- Video's extra ~120 MB private over a shader theme is mostly the decoder: Media Foundation's
  surface pool (sized by the hardware decoder for its reference frames) and the D3D11 video
  stack. Our part is the 5 shared slots (3 MB each at 1080p NV12, 6 MB as P010), which is not
  worth shrinking. 4K P010 slots are 25 MB each.
- Trimming was only measured while the screens were covered. With a visible, animated theme
  the working set settles at that theme's hot set instead of ~5 MB (11 MB for the bare
  example); it was not measured in the live engine because the monitors were never uncovered.

README-ready text (replace the memory paragraph under "What keeps it light"):

> Memory: most of the engine's footprint is the GPU driver stack. A bare wgpu DX12 window with
> one swapchain already costs 180-200 MB of working set on this machine (AMD's and NVIDIA's
> user-mode drivers and shader compilers, plus the NVIDIA overlay hooks that load into every
> Direct3D process). On top of that the engine uses FXC (deterministic, no `dxcompiler.dll`
> from whatever SDK is on `PATH`), shares multisampled scene targets between monitors, and
> trims its working set two seconds after each theme load. Measured with two 1440p monitors:
> a loaded, idle wallpaper shows 4-7 MB of working set in Task Manager (committed memory is
> 290-430 MB, of which the driver stack is most).

## EXIF orientation

`wic.rs` reads the orientation (tag 274) through `IWICMetadataQueryReader`, trying
`/app1/ifd/{ushort=274}` (JPEG), `/ifd/{ushort=274}` (TIFF, HEIC) and then
`System.Photo.Orientation` (PNG, WebP and others). A missing, unreadable or out-of-range value
means upright. The turn is applied with `IWICBitmapFlipRotator` after the scaler, not before
as one might expect: scaling first lets WIC stream the full-size picture through and hold only
the small result, whereas turning first makes the rotator keep a full-size copy (a 48 MP phone
photo would hold ~190 MB). The size budget is computed on the upright size. Slideshows share
the same decoder, so they follow.

Tests write TIFF and JPEG files with all eight orientations through WIC's own encoder and
compare every cell against the EXIF spec's table; the mapping to WIC transforms has its own
test. Only JPEG and TIFF are covered by tests; the PNG/WebP/HEIC queries are the standard WIC
property but were not run against real files.

README-ready text: in the `fit` paragraph add "Photos are turned upright by their EXIF
orientation." and drop "EXIF rotation" from the roadmap's open items.

## 10-bit and HDR video

`video_decode.rs` asks Media Foundation for NV12 or P010. A stream is treated as 10-bit when its
native type is a 4:2:0 10-bit HEVC, VP9 or AV1 profile (profile value 2 in all three), or is tagged
PQ or HLG; those ask for P010 first and fall back to NV12. 8-bit video asks for NV12 first and
is unchanged. The order matters: on this machine (AMD hardware decoder) a 10-bit HEVC file is
*accepted* as NV12 and the extra bits are silently dropped, so "try NV12, then P010" would have
played 10-bit video at 8 bits.

- **Zero-copy**: the shared slots are `DXGI_FORMAT_P010` textures, viewed in wgpu as `R16Unorm`
  (plane 0) and `Rg16Unorm` (plane 1). The device requests `TEXTURE_FORMAT_16BIT_NORM` when
  the adapter has it (D3D12 always does).
- **CPU fallback**: tightly packed 16-bit samples, uploaded as `R16Unorm` / `Rg16Unorm`.
- **Shader** (`video.wgsl`): the matrix rows fold in the bit depth and P010's shifted 10 bits,
  so SDR video of either depth takes the same path. PQ (ST 2084) and HLG (with the
  1000-nit-display OOTF) are decoded to light, divided by 203 nits (SDR reference white),
  converted from BT.2020 to BT.709 when the primaries say so, tone-mapped and sRGB-encoded.
  The tone map is linear up to 0.8 of SDR white, then a Reinhard shoulder
  (`0.8 + 0.2 * t / (1 + t)`), scaling all three channels by the same factor so hue is kept.
  Reference white (203 nits) lands at 0.9 linear (244/255 encoded); 1000 and 10000 nits stay
  distinct and under 1.0.
- **Untagged 10-bit video** is SDR: BT.601/709 by height, limited range, as for 8-bit.

Verification: unit tests render uniform frames through the real pipeline on a headless device
and read the pixels back (10-bit and 8-bit SDR agree; PQ at 0/100/203/1000/10000 nits; HLG
reference white; BT.2020 green clipping into BT.709), plus matrix, packing and tag tests.
`cargo run -p mypaper-wallpaper --example hdr_clip -- out.mp4 [pq|hlg|sdr]` writes a 2-second
1280x720 Main10 HEVC test clip (grey bars from 0.2 to 10000 nits, colour bars at 203 and 1000
nits, a moving 4000-nit disc) through Media Foundation's sink writer, about 40 KB, so nothing
needs committing. This machine's GPU driver supplies a hardware HEVC Main10 encoder; Windows has
no software one, so the example needs one too. The engine played all three clips: `P010 (Pq,
Bt2020)`, `P010 (Hlg, Bt2020)` and `P010 (Sdr, Bt709)`, hardware decode, zero-copy on the AMD
GPU, and through the forced CPU path the decoded luma samples matched what the encoder was given
(203-nit grey as code 569 against 573 expected; SDR white as exactly 940; HLG grey 673 against
675).

Not covered: the screen could not be looked at (both monitors were covered by maximized
windows), so the on-screen result is checked numerically, not by eye. VP9 and AV1 10-bit are
detected but untested (no encoder here); 12-bit (P016) and Dolby Vision profile 5 are not
supported; HLG assumes a 1000-nit display; there is no per-display tone mapping.

README-ready text: change "video is anything Media Foundation plays with 8-bit output (H.264,
HEVC 8-bit, ...)" to "video is anything Media Foundation plays: 8-bit and 10-bit (HEVC Main10,
...); HDR10 (PQ) and HLG video is tone-mapped to SDR". Drop "10-bit/HDR video" from the
roadmap's open items.

## Rust hot-patching

**Status: not working on Windows with dx 0.7.10.** A patch now builds in about 3 s, reaches the
app and its jump table maps `Engine::tick`, but the patched code crashes with `0xc0000005` the
first time it runs. Everything up to that point is diagnosed and fixed. All of it was
reproduced with a single clean instance (no stale processes involved).

What was wrong, in the order it bites:

1. **`scripts/hotpatch.cmd` did not set up the MSVC environment.** `%ProgramFiles(x86)%`
   cannot be expanded inside a `for /f (...)`: the `)` of `(x86)` ends the loop. `vswhere` now
   goes on `PATH` first. (Fixed.)
2. **"Missing rustc capture" in a worktree.** dx records each crate's rustc arguments by acting
   as rustc's wrapper and looking for `--crate-name`. With this repo checked out under a long
   worktree path, the engine's rustc command line passes Windows' 32 KB limit, so cargo hands
   rustc an `@response` file and no capture is written (the libraries are captured, the engine
   binary never is). A short `CARGO_TARGET_DIR` fixes it; the script defaults to
   `%SystemDrive%\mypaper-hotpatch`. (Fixed; the response-file cause is inferred from this
   behaviour and dx's source, not seen directly.) An `sccache` `rustc-wrapper` in a parent
   `.cargo/config.toml` was not the cause.
3. **A second `dx serve` over an unchanged tree fails** ("Missing linker args"), and a failed or
   killed run leaves a 0-byte `libdeps-*.a` that the next link rejects (`LNK1136`). The script
   touches `main.rs` and deletes the archive before starting. (Worked around.)
4. **Patches were built and sent but never took effect: the jump table did not contain
   `Engine::tick`.** The table has an entry for every function present in both builds (5935
   here), keyed by the function's real address from the PDB. With MSVC's default incremental
   linking, a function pointer taken in the program (`Engine::tick as fn(...)`) is the address
   of a 5-byte jump thunk in the incremental link table, not the function, so the lookup missed:
   `Engine::tick at 0x7ff7b14a3c15 is NOT mapped`. Linking with `/INCREMENTAL:NO` (passed as
   `--rustc-args=-Clink-arg=/INCREMENTAL:NO`, now in the script) makes the pointer the real
   address and it maps: `Engine::tick at 0x7ff709a960e0 is mapped (patched)`. (Fixed.)
5. **The patched function crashes on its first call.** After the patch is applied and
   `Engine::tick` is redirected, the next tick faults with `0xc0000005`, in a module-less address
   (`Faulting module name: unknown`, e.g. offset `0x00007ffe5c3c0b0d`). Two runs, with a
   one-line edit (adding a `log::info!`), with and without a new `static`. This
   points at how dx 0.7.10 links the patch on Windows (its stub thunks and relocations for
   symbols resolved back into the running exe), not at the engine. It was not diagnosed further.

`main.rs` now logs each patch (`hot-patch applied: N functions mapped, Engine::tick at ... is
mapped (patched)`), which is the quickest check of problem 4. The wiring (`hotpatch` feature,
`Engine::tick` called through a `fn` pointer in `subsecond::HotFn`) is unchanged and correct.

Workflow, as far as it goes (Dioxus CLI 0.7.10, `cargo binstall dioxus-cli@0.7.10`):

```text
scripts\hotpatch.cmd themes\aurora      # from a normal prompt; add --windowed yourself for the dev window
# edit something called from Engine::tick, save: "Hot-patching: ... took 3.2 s" and the log line above
```

The first build takes about 5 minutes; later starts take seconds. If dx reports "Missing
rustc capture" or "Missing linker args", delete the target dir named by `CARGO_TARGET_DIR`.

README-ready text: replace the "Rust hot-patching (experimental, not working yet)" paragraph with:

> **Rust hot-patching (experimental, not working yet).** `scripts/hotpatch.cmd` runs it with
> the setup `dx` needs on Windows. Patches build in about 3 s, reach the app and map
> `Engine::tick`, but the patched code crashes on its first call with dx 0.7.10; see
> [`docs/engine-notes.md`](docs/engine-notes.md) for the four setup problems already solved
> and the one that remains.

## Native plate engine (N0.3 / N2)

The engine is the only hardware GPU process. Native shell plates arrive through
`mypaper-glass-ipc`, a session-local page-file mapping protected by a seqlock and a named
auto-reset wake event. The shell owns all animation. Geometry, including subpixel positions,
is used directly: there is no engine spring, JSON file, registered glass message, fade timer
or plate scratch texture. A dead writer clears its plates when its process handle signals;
a new shell can recover the abandoned writer mutex.

Each visible screen keeps its **unmodified last wallpaper** as a sampled texture. Shader
`scale` determines that texture's size: at 2560 x 1440, rain-city keeps 1664 x 936 rather than a
second full-size texture. A full-scale source keeps 2560 x 1440. The engine restores the acquired
swapchain image (copy at full scale when supported, otherwise an upscale blit that covers every
pixel: no clear and no blending), then draws plates sampling the kept frame. When the source
draws the same frame on every screen (`Source::shared`: a shader theme that never names
`paper.mouse`, or a still image), screens of one render size share **one** kept frame, drawn
once per engine frame; the backdrop BMP is then read back once for them too. Rain and snow
share; aurora's cursor glow does not. A geometry/material update only restores and draws glass. A still
source draws once and then makes no submissions until something changes; removing a plate
restores the old region too. Theme switches release all old kept textures and plate bindings.
Web/app kinds continue to own their windows and have no engine plates; their live glass is the
separate overlay slice.

The clear material uses a continuous fourth-power rounded corner profile, a circular-cap
**inward** bevel confined to the edge, a clear centre, optional blur, and two opposite specular
cones coloured by the backdrop. Clarity jointly controls tint strength and luminance
compression. Clear centres preserve the wallpaper; a caller can supply a dimming tint over bright media.
The clear default has no shadow,
matching the Tahoe clear references in `C:\mypaper-targets\refs\liquid-glass`. This pass does
not introduce a regular-material drop shadow.

### Shell writer API

The crate is `mypaper-glass-ipc` (`crates/glass-ipc`); all geometry/blur units are **physical
virtual-desktop pixels**, not DIPs. Clarity 1 is clear and 0 is tinted. Colours are straight
sRGB RGBA in 0..=1. Radii are nonnegative; rect width/height must be positive and finite.

```rust
pub const MAX_PLATES: usize = 96;

pub struct Material {
    pub clarity: f32,
    pub blur: f32,         // 0..=32 physical px
    pub refraction: f32,   // 0..=1
    pub highlight: f32,    // 0..=1
    pub tint: [f32; 4],    // RGBA, each 0..=1
}
pub struct Plate {
    pub id: u64,           // unique within the complete snapshot
    pub rect: [f32; 4],    // x, y, width, height
    pub radius: f32,
    pub material: Material,
}
impl Writer {
    pub fn open() -> anyhow::Result<Self>;
    pub fn publish(&mut self, plates: &[Plate]) -> anyhow::Result<u64>;
}
pub struct Snapshot {
    pub sequence: u64,
    pub published_qpc: u64,
    pub plates: Vec<Plate>,
}
impl Reader {
    pub fn open() -> anyhow::Result<Self>;
    pub fn wake_event(&self) -> windows::Win32::Foundation::HANDLE;
    pub fn writer_process(&self) -> Option<windows::Win32::Foundation::HANDLE>;
    pub fn read_changed(&mut self) -> anyhow::Result<Option<Snapshot>>;
}
```

`Material` and `Plate` derive `Debug, Clone, Copy, PartialEq`; `Material::default()` is
`{ clarity: 1.0, blur: 0.75, refraction: 0.65, highlight: 0.55, tint: [0,0,0,0] }`.
`Writer::publish` atomically replaces the complete set and returns its even sequence. Duplicate
ids, nonfinite geometry, out-of-range materials and more than 96 plates are rejected before
publication. Keep the writer on its creating thread: its lifetime mutex is thread-owned.
Dropping it publishes an empty set and releases the lease. A second writer is rejected,
including a recursive acquisition on the same thread. There is deliberately one shell writer.

The named objects are `Local\mypaper.glass.v1`, `.wake`, and `.writer`. The 5,784-byte v1
mapping holds atomic sequence/QPC/PID/count fields and 96 x 15 atomic 32-bit payload words;
there are no pointers, strings, serde or filesystem operations on this path. The odd/even
sequence brackets atomic payload accesses, so readers neither tear a snapshot nor create
Rust data races. Reader retries are bounded; the event remains signalled for a later publish.
`qpc()` and `qpc_frequency()` expose the common Windows clock for the performance harness.

`cargo run --release -p mypaper-wallpaper --example plates -- --seconds 120` publishes a
floating bar, dock and six widgets **on each monitor** (16 plates on this fixture).
`--animate --hz <display rate>` moves both docks at that rate; no shell or second GPU device
is created. Run this only under the desktop lock described in `plans/native/IMPLEMENTING.md`.

### Renderer settings (GPU and frame cap)

The default is `auto`: the adapter that drives the most desktop screens (the primary screen's on
a tie; docs/glass.md, GPU default). There is no battery-dependent switch. `integrated` and
`high-performance` request wgpu's low-power and high-performance adapters. These are preferences when the requested class is unavailable.
The selected adapter and requested preference are logged. Only the engine opens a device.

Global and per-wallpaper user keys in `%APPDATA%\mypaper\config.json`:

```json
{
  "engine": {
    "gpu": "integrated",
    "fps": 60,
    "wallpapers": {
      "rain-city": { "gpu": "high-performance", "fps": 30 }
    }
  }
}
```

A theme may provide root-level `"engine": {"gpu":"auto","fps":30}` in `theme.json`.
Resolution is user per-wallpaper override > theme preference > global > built-in default.
Overrides use the theme folder basename (`rain-city`), including themes loaded by absolute
path. Omitted/null `fps` inherits; the global default is uncapped. Integer caps are 1..=360:
`60` means at most 60 wallpaper frames per second (60 Hz). A cap cannot raise a source's own
cadence; rain remains 30 fps at 0.65 scale unless explicitly capped lower. Plate publications
remain independent of the wallpaper cap. Stills and slideshow holds still sleep, and external
web/app processes retain their own clocks. The keys do not change shader quality or scale.

Both files are watched. FPS changes apply in process; GPU changes exit with `EXIT_RESTART`,
so the existing core supervisor releases and restarts only the engine on the selected adapter.
A directly launched development engine exits with that code and needs its launcher to restart
it. Invalid live config keeps the last valid settings. Studio controls are deferred to N7.
The shared schema types are `mypaper_config::{GpuPreference, RenderOptions, EngineConfig,
RenderSettings}`; theme `engine` uses `RenderOptions`. See their exact fields in
`crates/config/src/engine.rs`.

### Shader image inputs

```json
{"wallpaper":{"kind":"shader","source":"rain.wgsl","fps":30,"scale":0.65,
 "images":[{"source":"media/city.jpg","binding":1},
           {"binding":3,"prefilter":{"source":"profile.wgsl",
                                     "size":[8192,1],"format":"rgba32float"}}]}}
```

Group 0 binding 0 remains the 32-byte `paper` uniform. Each image reserves an odd texture
binding from 1 through 29 and its following filtering-sampler binding. Paths follow the same
validation and resolution rules as other assets. WIC decodes each photo once; the GPU builds
its mip chain once. Unorm sampling preserves the old WebGL ShaderMaterial's colour treatment.
Use `textureSample` for implicit refracted-UV mip selection, or `textureSampleLevel` for an
explicit LOD. `scale` still controls rendering resolution and the kept texture.

Optional `prefilter` runs a theme-owned WGSL `vs`/`fs` pass once at load, with input texture
at group 0/binding 0 and filtering sampler at binding 1. It has no `paper` uniform. Output
`size` defaults to the input dimensions; valid dimensions are 1..=16384. `format` defaults
to `bgra8unorm` (with mipmaps); `rgba32float` is an unfiltered, single-mip data cache accessed
with `textureLoad`. Omit image `source` for a generated texture; its default size is then 1x1.
Artistic shader code and its licence stay in the theme, outside the MIT engine.

A **baked layer**, `{"binding":5,"bake":"still_frame"}`, names a `@fragment` entry point of
the theme's own shader. The engine draws it once per render size, in the frame's format, before
the first frame of that size, and binds it like an image (texture at the binding, sampler
after it). It sees `paper` with the render resolution and time 0, and every loaded image; it
must not read a baked layer. Screens of one render size share it. The theme reads it with
`textureLoad(layer, vec2i(frag), 0)`: a pixel it draws unchanged is then bit-identical to the
baked one. Rain bakes its frame without drop refraction (blur, grade, vignette): a 2x2 quad in
which no pixel refracts loads the baked pixel, and the others draw the 18 samples, so the
blur's implicit derivatives see the same quad either way.

The three rain variants retain Heartfelt's layers, speeds, intensity, grading, vignette,
1.06 stationary CSS zoom and **all 18 original photo samples**, including the original /16
gain. Analytic gradients share field evaluation instead of evaluating Heartfelt three times.
The 8192 x 1 float32 row/column cache removes repeated trig/hash work without quantizing the
rain field. Fixed-time captures before/after this cache have 99.995% identical pixels in the
1800 x 800 wallpaper crop; the rest differ by at most one RGB level. A static photo-blur
prefilter was rejected because it changed the stochastic grain; branchy droplet early-outs
were rejected because they were slower on this iGPU. Neither optimization remains in production.
Since November 2026 each Heartfelt layer returns its value and the intermediates of its
gradient; the three layer gradients (most of the shader's divisions) run only where the
field's smoothstep slopes (0.3 < field < 1), elsewhere the refraction is exactly zero. A
per-layer "below the drop" early-out was exact but slower again (2.63 against 2.52 ms) and was
dropped; unrolling the 17-tap loop was bit-identical but no faster. Offline, one 1664 x 936
rain-city frame: 3.69 ms before, 2.88 with the baked still frame, 2.52 with the gradient split;
fixed-time frames differ from the original by at most 1/255 in 0.0003% of pixels
(`tools/perf/diff-frames.py`).
Explicit disk radii and reduced sine arguments avoid the FXC blur regression caught in the
web/native comparison. The snow ports keep all 50 original layers and the old CSS-resolution/
0.65-canvas crop and density. Fluids and vanta-waves remain web themes. Original GLSL, photo
grants, vendor notices and CC BY-NC-SA terms remain; obsolete page/JS hosts are removed.

### Timestamp telemetry

`MYPAPER_GPU_TIMINGS=<absolute JSONL path>` enables the adapter's wgpu timestamp-query and
encoder-timestamp features. Four GPU stamps measure wallpaper, restore/backdrop and glass
separately, per screen. Eight asynchronous readback slots avoid waiting for a map; file output
runs on a worker. Unsupported queries fail explicitly instead of substituting CPU timings.
With telemetry disabled there are no queries, readback buffers, worker or polling timer.

Each line contains `screen`, `frame`, `wallpaper_drawn`, `glass_drawn`, `sequence`,
`wallpaper_ms`, `restore_ms`, `glass_ms`, `publish_to_submit_ms`, and `submit_qpc`.
Publish-to-submit is a CPU delivery measurement. `MYPAPER_COMPOSITION_TIMING=1` additionally
calls `DwmFlush` after presenting both screens and records `publish_to_composition_ms`, an
upper bound on display delivery. This benchmark-only wait is excluded from ordinary CPU
baselines. [Microsoft documents](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmflush)
that DwmFlush waits for the caller's queued DirectX updates to be drawn. It is not a
per-display scanout timestamp.
GPU stamps use `Queue::get_timestamp_period()`. See `docs/performance.md` for the live runs,
merge gates and the limits of the lag check.

### Native measurements (4-5 November 2026)

These are the N0.3 figures. The engine hillclimb of 5 November 2026 cut rain-city to
257.11 MiB and aurora to 262.35 MiB, GPU per engine frame to 5.41 ms and all 16 plates to
0.275 ms: see `docs/performance.md`, Engine hillclimb, and Memory above.

Release build, AMD Radeon(TM) integrated GPU (DX12/FXC), Ryzen 7 7800X3D / 16 logical
CPUs. Both physical 2560 x 1440 displays were visible throughout every accepted sample:
DISPLAY1 at 240 Hz, DISPLAY2 at 200 Hz. The default clear fixture publishes a bar, dock and
six widgets on each display (16 plates); writer cost is excluded from engine-process figures.
Each run takes the shared desktop lock, and restores the original core and window placements.

CPU/private memory below are medians of **three independent 60-second samples**, without
queries or composition waits. MiB means private bytes / 1,048,576; these are not VRAM figures.

| Engine fixture | Private MiB (three runs) | Median MiB | CPU, % of one core (three runs) | Median CPU |
|---|---|---:|---|---:|
| Rain-city, 30 fps / 0.65 | 445.56 / 448.07 / 449.09 | **448.07** | 3.175 / 2.759 / 2.134 | **2.759%** |
| Aurora, 60 fps / full scale | 420.21 / 418.10 / 420.68 | **420.21** | 2.629 / 2.212 / 2.889 | **2.629%** |

The engine memory gate is the larger median + 10 MiB: **458.1 MiB private** (448.1 + 10,
rounded to one decimal). Rain meets the <=4% one-core CPU gate on both screens together.
The plan's whole-product allowance remains engine gate + 48 MiB (506.1 MiB); no native-shell
or whole-product pass is claimed by this engine-only fixture.

A separate 20-second timestamp run measures every component. Values below are
**median / p95 / maximum milliseconds**; complete-frame time includes wallpaper + restore/
backdrop + glass, so the 2 ms gate cannot be passed by timing only the wallpaper pass.
All 600 two-screen frame pairs were complete.

| GPU work | DISPLAY1 (240 Hz) | DISPLAY2 (200 Hz) | Gate / result |
|---|---|---|---|
| Wallpaper pass | 4.341 / 4.566 / 4.723 | 3.829 / 3.958 / 4.671 | Both already exceed 2 ms |
| Restore / backdrop | 1.125 / 1.285 / 1.698 | 1.149 / 1.325 / 1.643 | Included in frame time |
| Plates, per screen | 0.204 / 0.230 / 0.248 | 0.215 / 0.240 / 0.275 | Not the all-plate gate |
| **Complete frame** | **5.623 / 5.949 / 6.542** | **5.196 / 5.466 / 6.160** | **FAIL <=2 ms per screen** |
| **All 16 plates, summed** | **0.401 / 0.460 / 0.502** | (both screens combined) | **FAIL <=0.3 ms** |

The remaining median gaps are **3.623 / 3.196 ms** per complete frame, and **0.101 ms** for
all plates. Analytic gradients share Heartfelt field evaluation; one-time float32 row/column
caching removes repeated trig/hash work; photo mipmaps are built once; static sources and
unchanged plates skip rendering; glass reads the kept frame without a scratch copy or spring.
All 18 photo samples, every rain/snow layer, 0.65 scale and 30 fps are retained. A static photo
blur changed the grain and was removed; branchy droplet early-outs were slower and removed.
The row-cache-only comparison was modest: before 4.415 / 3.877 ms wallpaper medians, after
4.341 / 3.829 ms, with scheduling noise. This is not enough to meet the iGPU budget.

A 20-second motion run publishes moving docks at **240 Hz**. Only glass-only rows are used
for lag below: 2,005 on DISPLAY1 and 1,834 on DISPLAY2; 1,200 wallpaper submissions still run
at 30 fps total across both screens. Latest geometry is latched after swapchain acquisition.

| Rain + moving plates | DISPLAY1 | DISPLAY2 |
|---|---|---|
| One display frame | 4.167 ms | 5.000 ms |
| Publish -> queue submit, median / p95 / max | 2.867 / 4.185 / 4.779 ms | 2.369 / 4.207 / 4.897 ms |
| Publish -> DWM completion bound, median / p95 / max | 7.486 / 11.984 / 13.368 ms | 5.858 / 8.312 / 10.137 ms |
| One-frame glass lag gate | **Not established** | **Not established** |

The DwmFlush bound exceeds one frame by 3.319 / 0.858 ms at the median, and 7.817 / 3.312 ms
at p95. This is a conservative completion bound for both screens, not a per-display scanout
measurement. Queue submission alone cannot establish visible lag. Precise scanout remains
unverified; the <=1-frame gate is not claimed. The composition wait changes pacing and is
excluded from the ordinary CPU/memory baselines.

Over a still photo with unchanged plates, the 60-second exact window contains **0 submissions,
0 wallpaper passes and 0 glass passes**, with 362.09 MiB private and 0.104% one-core CPU.
The frame loop has no idle render timer; window/config events can still wake the process.
The telemetry-off 60-second still baseline measures **363.98 MiB / 0.078% of one core**.
The strict 0% still-CPU gate is therefore not met in this desktop session, even though the
separate query run proves zero rendering; event handling still has a measurable CPU cost.

A separate 15-second still + 240 Hz motion run has **0 wallpaper submissions** throughout:
2,158 / 2,110 glass-only rows. Queue delivery is 1.801 / 1.344 ms median,
4.203 / 4.089 ms p95, 4.847 / 4.647 ms maximum. Its DWM completion bound is
6.132 / 5.383 ms median, 8.740 / 8.160 ms p95, and 29.241 / 29.241 ms maximum.
Its all-plate GPU time is 0.377 / 0.431 / 0.589 ms (median / p95 / max).
This verifies source reuse, not the one-frame presentation gate.

Telemetry overhead is measured separately: GPU-query rain uses 4.607% one-core CPU / 454.80 MiB;
rain motion + DwmFlush uses 12.023% / 456.97 MiB; still motion + DwmFlush uses 13.534% /
380.80 MiB. These opt-in runs are not substituted for the telemetry-off idle fixture.

**Default stays integrated** on both desktops and laptops. (Superseded 5 November 2026: the
default is now the adapter that drives the screens; see docs/glass.md, GPU default.) The earlier RTX candidate's roughly
0.62 ms rain / 0.067 ms all-plate figures used the rejected photo prefilter, so they are not
final-quality measurements and do not justify a policy change. Users can explicitly select
`high-performance` or `auto`; there is no automatic RTX promotion. Studio controls remain N7.

Raw CPU/RAM samples and GPU/lag summaries are committed under
[`tools/perf/results/native-2026-11-04`](../tools/perf/results/native-2026-11-04).
Full JSONL traces and fixed-time images remain in `C:/mypaper-targets/native-engine/`.
The five `visuals/<theme>-web.png` / `-native.png` pairs and `visuals/weather-comparison.png`
were inspected for photo/crop, preset colour, weather density and blur. WebGL/WGSL hash
rounding changes individual rain/snow locations; the port is not pixel-identical to WebGL.
`visuals/quality-comparison.png` compares the native rain shader before/after the row cache at
12 seconds: **99.99507% identical pixels** in a 1800 x 800 crop, RGB MAE 0.00001644,
p99 difference 0 and maximum 1/255. No blur taps, scale or layer counts were reduced.
Clear-glass edge/dock crops were compared with the Tahoe clear references: inward lensing,
clear centre, opposite angle-dependent glints, no clear-material shadow.

The required `measure.ps1 -Label native-engine-restored-desktop -Seconds 60` also ran
under its own lock after the isolated fixture was stopped. It samples the **restored legacy
product**, not the native engine fixture: 24 processes, 98.72% of one core (6.170% of this
machine), 7.73% GPU 3D, 455 MiB GPU memory, 1,069 MiB private / 1,587 MiB working set.
Its 19 WebView2 processes account for 852 MiB private and 96.43% one-core CPU. This separate
counter sample is excluded from N0.3 gates; its raw output is `restored-product-counters.json`.

Cleanup limitation: automatic approval review rejected removal of the stopped test WebView
profile, returning only "blocked by policy". No test WebView processes remain, but
`C:/mypaper-targets/native-engine/release/native-stage/env-0/local/mypaper/webview2` was left
on disk. No user browser profile was targeted.

Desktop cleanup stopped all fixture engines/writers and restarted the original
`D:/Coding/projects/desktop/wallpaper/target/release/mypaper-core.exe`. The existing user's
`bars.json` has `taskbar.mode: "hide"`: a final show request left the primary taskbar hidden
and the secondary visible. That user setting was not changed. Another agent subsequently
acquired the desktop lock for its own session; its processes/lock were left alone.

The native runner's success exit means the sample was accepted (including visibility), not
that GPU/lag/zero-CPU budgets passed. These open gates are reported explicitly above.

Validation: 132 scoped Rust tests pass (config 11, IPC 2, theme 14, wallpaper 87, perf 18),
with two existing wallpaper dev tests ignored (real desktop wallpaper mutation and the still
export helper). All-target Clippy is clean on the five changed crates. The GPU port-compile
test ran under the desktop lock; all 66 catalog asset byte counts and SHA-256 hashes match.
Live config verified a 12 fps cap without restart and high-performance/auto/integrated restart
requests using core's existing EXIT_RESTART contract. No bars/widgets compatibility patch,
N7 Studio UI, or overlay code was changed. GPU, lag and strict still-CPU gates remain open for further optimization.


## Curated live wallpapers — 4 November 2026

The gallery contains 15 live themes: four H.264 videos, two local WebView2 pages and
nine WGSL ports/palettes. The user rejected plain stills and weak loops; those candidates
were removed. Rainy Hong Kong is retained with amber/heavier-rain and midnight/lighter-rain
presets. No new original shader artwork was invented. Sources, author grants, exclusions
and the proposed local importer are in [the research record](research/wallpaper-sources.md).
Every shipped file is attributed in [CREDITS.md](../themes/CREDITS.md); the machine-readable
[catalog](../themes/catalog.json) holds sources, changes, sizes, hashes and measured loops.

New binary data is **23,947,981 bytes (22.84 MiB)**, including the new legacy interactive
preview. Curated binaries account for 23,945,606 bytes; all curated theme files total
24,706,400 bytes. Existing assets moved into `examples/` are excluded from new-binary
accounting. Full upstream movies, download tools and temporary recordings are not shipped.
Eight selections are CC BY-NC-SA 3.0: the rain, snow and Seascape families. Remove them or
obtain permission before a commercial distribution; retain their ShareAlike terms and the
separate CC BY background credits. The other seven selections use MIT or CC BY 4.0.

### Compatibility and adapters

- The original `aurora`, `midnight`, `earth-orbit`, `islands`, `photo`, `gallery`, `webdemo`
  and `interactive` folders moved to `themes/examples/`, with `hidden: true`. The app template
  moved too. Existing shader/media content remains; the stopped WIP's new shaders were omitted.
- The theme schema accepts a default-false boolean `hidden`. Lookup falls back to `examples/`
  for bare IDs and old direct `themes/<id>` paths. Existing direct paths and user-theme
  overrides keep precedence. No user's config was rewritten. Studio must honor `hidden`
  and retain a current legacy selection; Studio code is outside this branch's ownership.
- Shader ports use the existing 32-byte `paper` uniforms, bottom-left Shadertoy coordinates
  and the engine prelude. Original GLSL and license headers are supplied. Seascape keeps its
  default mouse position to avoid cursor-driven camera jumps; its midnight variant only
  grades the output. The existing snapshot tests now reference the relocated Aurora.
- The ignored `snapshot::tests::still_of_a_theme` helper renders native sources through the
  actual offscreen DX12 pipeline without changing the desktop. `MYPAPER_STILL` supplies
  `theme;absolute-output.jpg;seconds;widthxheight`; its examples are in the test's comment.
- Web dependencies are local. Fluids retains the upstream simulation with automatic original
  splats, reduced resolution and a 30 fps ceiling. Removing analytics also fixes its actual
  startup `ga is not defined` failure. Rain/Snow clocks no longer reset every six hours.
  No engine runtime changes were needed for these pages or the videos.

### Finite video loop verification

Every retained video was re-encoded at its source's 24/25 fps, muted H.264/yuv420p, no larger
than 1920×1080, CRF 18 with a 6 Mb/s VBV cap. Spring was trimmed between film cuts; its earlier
candidate contained an internal cut and was rejected. Bright/generic and less suitable
selections were dropped rather than kept to fill the gallery.

Each clip starts after its source head. Its tail overlaps that head with a quintic
smootherstep fade; the next file start is the next source frame after the head. There is no
ping-pong or reversed motion. For an N-frame overlap, FFmpeg's descending progress `P` is
normalized as `q=clip((P-1/N)/(1-1/N),0,1)` and weighted with `q³(6q²−15q+10)`. This gives exact
discrete endpoints and zero endpoint fade slopes, avoiding the residual blend snap found
in the earlier linear overlaps. Exact recipes and source time ranges are in the catalog.

| Theme | Output / overlap | Seam SSIM | Adjacent SSIM before / after | Seam PSNR | Seam / nearby pixel-difference ratio |
|---|---|---:|---|---:|---:|
| charge-city | 2.500 s / 0.75 s | 0.98663 | 0.98789 / 0.99108 | 46.88 dB | 1.17 |
| cozy-fireplace | 23.000 s / 2 s | 0.97837 | 0.97831 / 0.98929 | 42.07 dB | 1.67 |
| milky-way | 16.000 s / 2 s | 0.72178 | 0.72623 / 0.72396 | 30.34 dB | 1.09 |
| spring-forest | 2.875 s / 1 s | 0.99007 | 0.99416 / 0.99923 | 47.63 dB | 1.57 |

SSIM/PSNR use decoded 640×360 RGB frames. Pixel differences, camera registration and optical
flow use 160×90 frames. The ratio compares the last→first mean absolute difference with six
nearby normal transitions. The Milky Way timelapse's moving stars/noise lower normal adjacent
SSIM too; its seam matches that behavior rather than an arbitrary 0.99 threshold. Flame
flicker explains a higher fireplace ratio; its seam SSIM is the same as the preceding normal
transition. Each seam's difference is below the clip's largest ordinary internal transition.

Phase registration at the seam matched the surrounding frame shifts for all four clips:
**0.0 px excess translation at 160×90**, measured in 0.1 px increments. Mean optical-flow speeds
before / across / after the seam were respectively 0.209/0.079/0.119, 0.195/0.122/0.107,
0.284/0.210/0.304 and 0.065/0.043/0.013 px/frame. The full-stream cut scan and five additional
flow samples through each overlap were inspected together with these values; there is no
detected camera snap or hard cut. Local flame/foliage motion is not constant-speed translation.
Crossfades softly combine two source moments during the overlap; these are prepared looping
excerpts rather than claims that the original films were inherently cyclic.

Reproduce the measurements without changing content:

```powershell
py -3 themes/verify-loops.py --ffmpeg C:/tools/ffmpeg.exe --ffprobe C:/tools/ffprobe.exe --output C:/temp/loops.json
```

The script requires numpy and scikit-image, decodes every frame, and records codec, size,
audio count, seam/ordinary SSIM, PSNR, pixel differences, registration and flow. Scores are
evidence to assess together, not an automatic universal perceptual pass threshold. Procedural
pages/shaders run continuously and have no last→first media restart; original algorithms were
preserved and the weather clock resets were removed.

### Checks and visual evidence

With `CARGO_TARGET_DIR=C:\mypaper-targets\wallpapers`:

- `cargo test -p mypaper-theme --lib`: **12 passed**. Includes all 23 manifests, referenced
  assets, 15 curated previews, every file's attribution/size, strict hidden parsing and old-ID
  and old-path compatibility.
- `cargo test -p mypaper-wallpaper sources::shader -- --nocapture`: **2 passed**. All four
  curated WGSL shaders compile through the actual DX12 engine prelude; uniform layout matches.
- Independent catalog checks: **121 attributed files**, all sizes and SHA-256 hashes match;
  every curated preview is 640×360; independently counted new binaries match 23,947,981 bytes.
- JavaScript syntax checks: **19 programs parse**, local HTML dependencies exist. A focused
  probe of Fluids' actual WebGL context initializer passes without a global analytics function.
- The loop tool decoded all four final videos successfully: H.264/yuv420p, native 24/25 fps,
  no audio tracks, within the resolution limit. Final metrics are embedded in the catalog.

Native shader previews came from actual DX12 renders; web previews came from real local
WebView2 desktop captures, with the taskbar cropped out. Earlier desktop batches showed the
weather variants and working Fluids. The final two-cycle desktop recordings were obscured by
an open application and were discarded; they are **not playback evidence**. No windows were
focused or rearranged. The desktop lock was released and the user's original release core
was restored. Final video verification therefore covers decoded files and source motion,
not an unobscured end-to-end recording of the engine's decoder restart.

The final contact sheet is `C:\WINDOWS\TEMP\mypaper-curation-aeb50a5e75cd490fb27d58f080e87bfd\gallery-contact-sheet.png`;
the weather desktop contact is `weather-variants.png` in the same directory. These review
artifacts are temporary and excluded from repository byte totals. Source fetch limits are
recorded honestly: no current live Shadertoy popularity ranking or complete current Lively
installer contents were independently verified, so no unresolved asset was bundled.

## Glass overlays

Liquid Glass under shell surfaces that are not over the engine's own frame: flyouts, menus and bars
over app windows (`Layer::Popup`), and widgets over web and app wallpapers (`Layer::Desktop`, D-W).
Built in N0.2 as `crates/wallpaper/src/overlay`; the engine loop drives it from N3. Measured on the
dev desktop (Windows build 26300, the engine on the low-power AMD iGPU, both 1440p monitors on the
NVIDIA dGPU at 239 and 199 Hz) with `examples/overlay_spike.rs`.

How it works, per open surface: a click-through, never-activated window directly under the shell's
window shows glass.wgsl (the plate pass's shader, used unchanged) through a DirectComposition
surface under a rounded clip. What it refracts comes from one Windows.Graphics.Capture session per
monitor that has an overlay, with one frame-pool buffer, `DirtyRegionMode::ReportOnly`, no cursor,
no border, and an exclusion list holding every overlay and client window.

What the spike decided, each by measurement:

- **The capture device is a separate D3D11 device** on wgpu's adapter: +16 MB private.
  D3D11On12 on wgpu's own device costs +92 MB. The CPU per crop update is the same for both
  (`examples/overlay_spike_device.rs none|11on12|d3d11`). Crops are D3D12 textures in a shared heap
  with simultaneous access, opened in D3D11; one shared fence orders D3D11's copies before wgpu
  reads them and wgpu's glass before D3D11 copies it out.
- **Overlays show DirectComposition surfaces, not swapchains.** The first composition swapchain on
  the iGPU, shown on the dGPU's monitors, costs ~32 MB private whether wgpu or D3D11 makes it
  (+15 MB for a 600x400 one); on the dGPU it costs ~0. A DirectComposition surface drawn by the
  capture device costs ~3 MB (`overlay_spike_device swapchain-wgpu|swapchain-d3d11|swapchain-surface|swapchain-wgpu-hp`).
  wgpu draws the glass into a shared texture, D3D11 copies it into the surface, one commit.
- **Excluded windows still produce dirty rects.** An excluded window that repaints or moves
  reports a dirty rect exactly over itself, though the captured pixels don't change
  (`overlay_spike_device frames`). DWM also reports an animated wallpaper as dirty under opaque
  windows. So dirty rects alone can't stop the glass redrawing itself:
  1. On WGC's thread, frames older than the exclusion list's configuration iteration, or with
     dirty rects that miss every overlay's frame, are dropped.
  2. A frame that touches an overlay is copied into a spare crop and compared with the shown one
     on the GPU (`compare.wgsl`).
  3. Only a crop that differs is drawn.

  While what is behind keeps changing, the glass is drawn speculatively in the compare's own
  submission (one GPU round trip) and shown only if the compare says so. The same dirty rects are
  also what makes a new or moved overlay get its pixels at once.
- **Window exclusion works only for top-level windows.** `IDisplayGraphicsCaptureSession` is in
  neither the installed SDK (10.0.26100.0) nor `windows` 0.62, so `overlay/abi.rs` defines it by IID
  (`bb91f61b-…`, `SetWindowExclusionList` at vtable slot 6), along with `IDirect3D11CaptureFrame3`
  for `ConfigurationIteration`. Support is detected by `QueryInterface`. With top-level windows it
  works:
  - the glass's backdrop shows none of the popup's marker pixels;
  - a GDI screenshot and an independent capture session (Snipping Tool's API) show all of them.

  With **child windows** of the desktop host it does nothing: a child overlay captured itself
  (over a still web page it kept redrawing, 55 passes in 15 s, with a doubled rim) and its widget.
  So `Layer::Desktop` is a plain top-level window directly under the widget, and the shell must
  keep its widget windows top-level, just above the desktop (Progman). Over the same still page:
  0 passes, and the widget is excluded.
- **Under the raised desktop, neither GDI nor per-pixel-alpha (`UpdateLayeredWindow`) child
  windows of Progman show anything**, even with a Windows 8 manifest; DirectComposition children
  do. That matters for the shell's widget surfaces too (N1, N4).
- **Borderless capture needs no package.** `GraphicsCaptureAccess::RequestAccessAsync(Borderless)`
  returns `Allowed` for the unpackaged exe (Settings > Privacy & security > Screenshot borders), so
  no sparse package was made. It is asked once when the overlays are created, off the open path.
  The check is controlled: a session that requires the border puts 11,321 yellow pixels on the
  monitor's edges (`overlay_spike_device border`); ours, 0.
- **Moves land in one frame.** A `Place` moves the overlay window inside the client's
  `SendMessage`, so the client's own move follows it within the same composition frame.

N0.2's checks (popups on the left monitor; two runs, 20 s phases untimed for CPU and 10 s phases
with timestamp queries for GPU; CPU is % of one core):

| Check | Result | Gate |
|---|---|---|
| z-order while both move | overlay directly under the popup; 286 of 287 moves placed inside the client's `SendMessage`, the popup followed 0.56 ms later (max 1.15 ms) | pass |
| glass lag | window: same composition frame (0 of 287 more than a 4.17 ms refresh apart); the glass inside: redrawn 8 ms after the move (median; p95 16 ms) | window passes; the glass inside is up to ~4 refreshes at 239 Hz, bounded by the capture's frame rate |
| clicks | the popup's alpha 1/255 body gets a real click; the overlay's rim outside it is not hit | pass |
| one session, two overlays | 2 overlays on 1 session | pass |
| video elsewhere, nothing behind changes | 0.08–0.70% CPU, 0 glass passes (all frames dropped on WGC's thread) | pass (≤1%) |
| no feedback loop (popup hover 10 Hz and clock, overlay presents) | 0 glass passes, ~10 compares/s, 0.3–0.65% CPU | pass |
| open-to-glass | cold (first overlay, new session) 182–379 ms over 14 runs (median ~260), then the 300 ms fade; warm (session running) 15–20 ms | warm passes; cold misses (WGC's first frame takes ~170–340 ms after `StartCapture`) |
| memory while open, after close | +21 MB just after the first open (the capture device is 16), +4 to +7 MB a few seconds later with 1–2 overlays; below the starting figure after close | pass (≤ +30 MB) |
| CPU per update, a video behind the popup (~59 updates/s at 60 or 30 fps: at 30 every change is followed by a compare of our own present) | 1.6–4.4% over 6 runs, median ~3.4% (about 0.55 ms per update; `Commit` ~0.2 ms of it); other agents were running perf tools at the time | borderline miss (≤3%) |
| GPU per update | 0.071–0.075 ms (copy, compare, glass); a compare alone 0.054 ms | pass (≤0.5 ms) |
| desktop layer over a web wallpaper (fluids), under a widget | widget > glass > Progman; 323 glass passes in 15 s (live), 0.080 ms GPU each, 2.2% CPU; 0 passes over a still page | pass with top-level overlays (fails as a child of the desktop host) |
| exclusion and screenshots | 0 popup pixels in the glass's backdrop; 256 of 256 in a GDI screenshot and in an independent capture session | pass |
| borderless | `Allowed`, 0 yellow edge pixels | pass |

Running it: take the desktop lock first, then
`cargo build --release -p mypaper-wallpaper --example overlay_spike --example overlay_spike_popup`
and `target/release/examples/overlay_spike popup 20 left`. The `desktop` scenario expects a web
wallpaper showing, for example a second `mypaper-wallpaper fluids` run with `RUST_LOG=off`.
`MYPAPER_SPIKE_NO_TIMING=1` leaves out the GPU timestamps (they wait for the GPU). Screenshots go
to `%TEMP%\mypaper-overlay-spike\`.

In the engine since N3 (the shell's flyouts and menus use it; docs/bars.md "Flyouts (N3)"):
- The engine creates `Overlays` in desktop mode with `Engine::overlay_gpu()` (`gpu.rs` exposes the
  adapter), calls `tick` after each message pump, and wakes by `Overlays::deadline`. Requests and
  frame wakes arrive as messages to its own window: no new wait handle.
- **Warmth:** the capture device, sessions and DirectComposition device stay 3 s (`WARM`) after
  the last close; the glass pipelines are built once per engine. Warm opens show glass in
  38-58 ms; cold ones in 166-252 ms in 13 of 16 live clicks, 326-354 ms in the other three
  (WGC's first frame). A pointer-down
  warm-up request was tried and removed: starting the capture during a press delayed the
  click's delivery to the shell by ~30 ms, breaking the 30 ms content gate.
- **Update rate:** `MIN_INTERVAL` is 1/30 s. With an animated window behind quick settings the
  engine used 1.8-2.1% of a core (~25 updates/s) and 0.12-0.15 ms GPU per update; 0 glass
  passes and no session once closed.
- `MYPAPER_OVERLAY_TIMINGS=1` (with `MYPAPER_GPU_TIMINGS` for the timestamp feature) times the
  overlays' GPU work; every open logs its open-to-glass time and every close the update totals.
