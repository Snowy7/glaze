# Engine notes

Findings from closing the known engine gaps: EXIF rotation, 10-bit/HDR video, memory,
Rust hot-patching. Each section ends with README-ready text.

## Memory

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

So about two thirds of the engine's footprint is the GPU stack, which is not ours to cut: the
driver user-mode DLLs map 105 MB (`nvgpucomp64`), 86 MB (`nvwgf2umx`) and 77 MB (`amdxc64`),
and the loader pulls the NVIDIA ones into every D3D process on this machine.

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

- `MemoryHints::MemoryUsage` was already set. Swapchains already use the minimum two buffers
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

## Liquid glass pass (`crates/wallpaper/src/glass_pass.rs`)

UI processes publish glass plates (`mypaper_theme::glass`: `%LOCALAPPDATA%\mypaper\glass\<source>.json`
plus the registered `mypaper.glass` message to the control window); the engine draws them into its
own frame after the backdrop copy: the rim strips are copied out of the swapchain, then each plate
is drawn as rings (a soft shadow outside, the rim band bent inward with a chromatic split and a
specular rim; the clear centre is never rasterized). Plates spring between rects. The hooks are four
lines in `engine.rs` (`advance`, `take_dirty`, `deadline`, `encode`) and one in `main.rs`'s pump;
without plates nothing is allocated or drawn.

Cost, measured with the ignored test `glass_cost` (`cargo test --release -p mypaper-wallpaper
glass_cost -- --include-ignored --nocapture`; many passes per submit, so it is GPU time, on the
low-power AMD iGPU at 2560x1440): about 0.35 ms with 1 plate, 0.5 ms with 4 and 1.1 ms with 24.
Most of the small-count cost is fixed (the copy out of and the second pass into a full-size
swapchain texture), so the 0.1 ms target is not met yet; the ideas that would get there are in
the widgets agent's report (drawing the plates inside the source's own pass, or into a small
per-plate target).
