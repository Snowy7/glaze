# Liquid Glass

All of Glaze's glass (bar islands, widgets, flyouts, menus, Studio's previews) is drawn by the
engine with **liquid-rust** (<https://github.com/al1h3n/liquid-rust>, by al1h3n, MIT OR
Apache-2.0), vendored in `crates/liquid-rust` (see its `VENDORED.md` for the commit and the few
marked `Glaze:` changes). Its look is calibrated against Apple's iOS 26/27 Figma kit, and we
keep its shaders, pipeline and parameter values as they are.

## The model (Apple's Liquid Glass, as liquid-rust draws it)

1. **Lensing, not blur, carries the look.** The backdrop is refracted through a squircle bevel
   (Snell's law, η 1.5). Content bends strongly at the rim and lies flat in the middle. The bevel
   is `depth` deep (30 pt, more for large glass), with a small per-channel dispersion. Thin glass
   gets a shallow lens (Glaze, below): at most a fifth of its smaller side.
2. **Frost is light.** One blur pyramid (13-tap downsample, B-spline reads at a continuous level)
   serves every radius. The kit's regular glass uses 10 pt, its clear glass 6 pt.
3. **Clear vs tinted** is iOS 27's slider: clear fades every fill and thins the frost; tinted
   darkens the dark face toward `#1C1C1E` and whitens the light face. Neither goes grey.
4. **Light is directional.** A crisp rim line is brightest on the edge facing the light (from
   the top), weaker on the opposite edge, and takes its colour from what's behind it. It's not a
   uniform stroke. Soft inner rim shadows add thickness, and a darkened hairline defines the edge
   over light content.
5. **Adaptive face.** Tone mapping reproduces Figma's LINEAR_DODGE / LUMINOSITY / LIGHTEN / DARKEN
   fills in sRGB-encoded space. Large glass is thicker: more frost, a deeper shadow, a more
   opaque face.
6. **A soft drop shadow** grows with size and over busy content.
7. **Shape matters.** One edge, matching the surface exactly. The UI draws no outline on glass
   (widgets, bar islands, flyouts and the desktop menu only take clicks there). The default is
   quarter-circle corners (`Shape::Circular`); squircle surfaces use smoothing 0.6 with the same
   continuous contour in the kit, plate shader and overlay coverage.
8. **Liquid merging and glass on glass** are in the renderer (smooth-union SDF per container;
   higher layers refract lower glass). Plates don't use containers yet, because our islands
   don't fuse in the UI.

## Our controls on the kit's knobs

`crates/wallpaper/src/material.rs` maps the plan's five controls onto `Glass.regular`:

| Control | liquid-rust | Note |
|---|---|---|
| clarity, clear (1) to tinted (0) | `Appearance::transparency = 1 - clarity` | iOS 27 slider; 0.5 is the kit |
| blur (px) | `frost` = blur / scale (pt) | 15 px at 150 % = the kit's 10 pt |
| refraction | `refraction` | Figma *Refraction* |
| highlight | `specular` = 0.4 × highlight | 0.75 = the kit's 30 % *Light* |
| tint | `tint` (stained glass, alpha = strength); its colour picks the scheme | dark tint: dark face |

Every surface reaches these through one rule, `GlassMaterial::tuned` (`crates/ui/src/theme.rs`):
the style's material at the surface's own default (bars clearer, flyouts more tinted), with each
control the user set (`config.json` › `look.material`, or the family's `look.bars` / `widgets` /
`menus`) replacing it unclamped. Blur 0 is no frost; a set clarity also sets the tint's strength to
`1 - clarity`, so clarity 1 is clear, untinted glass. Blur is in DIPs until a surface hands it to the
engine (times its scale). A bar's `look.transparent` drops its plate altogether (the macOS top bar's
default); a material or style change restyles bars in place, never new windows or appbars.

### Finer knobs on the wire (6 November 2026)

Beyond the five controls, each glass takes a style, neutral by default (draws exactly as before).
Plates: `glaze_glass_ipc::Style`, sent with `Writer::publish_styled(&plates, &styles)` (one per
plate, or empty for all defaults). Overlays: `request::Style` in `Request::Style { client, style }`,
sent after each `Open` (a new overlay starts at `Style::DEFAULT`).

| Field | Range, default | What the engine draws |
|---|---|---|
| `brightness` | 0..=2, 1 | multiplies what's behind the glass (refracted and frosted), before tone mapping |
| `saturation` | 0..=3, 1 | saturation of what's behind (0 grey) |
| `smoothing` | 0..=1, 0 | corner smoothing: 0 quarter circles, 0.6 iOS's continuous squircle (`Shape::Smooth`); the surface must draw the same corners |
| `sheen` | 0..=1, unset | the kit's rim light inside the lit (top) edge, 0.5 = the kit's; unset (`None`, or negative on the overlay wire) follows `highlight` |
| `depth` | 0..=1, unset | the kit's broad rim shade under the lit edge, 0.5 = the kit's; unset follows `highlight` |
| `inner_shadow` | 0..=1, 0 | independent soft shade at the bottom-facing inner edge; zero skips its SDF/blur work |
| `fade_ms` (overlays only) | 0..=5000, 300 | how long the glass fades in when it first shows; 0 shows it at once (reduced motion) |
| `fade_curve` (overlays only) | resolved Bézier or physical spring; default cubic ease-out | shell names resolve to the content's curve; compiled into a DirectComposition animation once when glass shows |

Plate blur (`Material::blur`, physical px) now takes up to 256 px (64 DIP at 400 %); frost still
tops out at 64 pt. Styles resolve into the glass's material and shape only when a plate (or
overlay) changes, never per frame; brightness and saturation are one multiply-add in the glass
shader, skipped when both are 1. Plate styles travel in a second section stamped with the
snapshot's sequence. `inner_shadow` uses another sequence-stamped section, `<channel>.inner-shadow`
(392 bytes), leaving the original five-word style stride unchanged. Only a `Style` request is
longer than the old overlay wire. Engines accept both its original 80-byte form (new fields
default) and its extended 104-byte form. A new shell retries the original form, with the supported
2,000 ms cap, when an older engine rejects the extension; its other optics still work.

Glass follows the scheme, never the content (`adapt_to_content` off): our labels follow the
surface's colours. Plates move exactly as published (`Scene::place`, since the shell owns motion);
liquid-rust's springs materialize glass in and out and fade material changes.

## Where it runs

- **Plates** (bars and widgets over the wallpaper): `glass_pass.rs` keeps one `Scene` per
  screen and one `Renderer` per kept wallpaper frame. Screens sharing a kept frame share its
  pyramid, which holds every screen's part. The pyramid is rebuilt when the kept frame holds a
  wallpaper it hasn't blurred (`KeptFrame::drawn`), and only over the texels each group reads.
  It draws over the restored swapchain image through sRGB views. Nothing runs while the
  wallpaper and plates hold still.
- **Studio's previews**: `preview.rs` draws each request from the first drawn kept frame of any
  screen, with a fork of the plates' renderer (`GlassPass::fork`: its compiled pipelines, its own
  pyramid), so an answer compiles nothing.
- **Overlays** (flyouts and menus over app windows): `overlay/glass.rs` copies the WGC crop into
  an sRGB texture, draws the glass over a transparent clear (premultiplied for
  DirectComposition) and copies it out. The window grows by `Material::outset` (shadow or
  sampling reach).

## Cost (5 November 2026, 2560 x 1440, rain-city, `src/lab.rs` fixture of 8 plates)

GPU per screen for the glass pass, median of 150 frames. `GLAZE_LAB_GPU=high` runs the lab on
the discrete GPU; the lab prints every pyramid level and glass layer (`liquid_rust::Profiler`).

| | old glass | liquid-rust as vendored | now, AMD iGPU | now, RTX 4070 SUPER |
|---|---:|---:|---:|---:|
| Glass-only frame (plates moved) | ~0.14 ms | 2.90 ms | **2.48 ms** | 0.08 ms |
| Frame where the wallpaper changed | ~0.14 ms | 4.06 ms | **2.92 ms** | 0.12 ms |
| of which: blur pyramid | | 1.17 ms | 0.42 ms | 0.03 ms |
| of which: glass layer | | 2.90 ms | 2.49 ms | 0.09 ms |

Every step kept the picture: the lab's frames differ from the vendored render by at most 1/255
in 0.0017% of pixels (rounding in the shader's closed forms); the pyramid changes are
bit-identical. The integrated GPU is still over its 2 ms budget, so the glass layer is the open
item there; the engine now defaults to the GPU that drives the screens (below). It is ALU-bound: the second frost level costs 0.39 ms, dispersion 0.63 ms, the inner
shadows 0.35 ms and the rim light 0.24 ms (ablations, not shippable). The cheapest approximation
measured, one frost sample where the dispersion's R-B split is under 0.5 px, saves 0.22 ms and
differs by at most 1/255 in 0.06% of pixels. It isn't shipped.

GPU memory: +28 MiB private for two screens on separate kept frames (two half-resolution
Rgba16Float pyramids, 9.4 MiB each, of which level 0 is 7 MiB); about half that when the kept
frame is shared. Still wallpaper and still plates cost nothing.

The GPU (`engine.gpu`: auto, integrated or high-performance) and frame rate (`engine.fps`, 1 to
360) are engine settings, per wallpaper too; overlays open on the engine's adapter.

## GPU default: the card that drives the screens (5 November 2026)

**`auto` is the default, and it renders on the adapter that drives the most desktop screens**
(the primary screen's on a tie), read from DXGI outputs without creating a device
(`glaze_desktop::display_gpu`, used by the engine's `gpu.rs` and by Studio). Here that is the
RTX 4070 SUPER, which drives both monitors; on a laptop whose panel hangs off the iGPU it is the
iGPU, so a laptop keeps rendering where it presents and the dGPU stays asleep. `integrated` and
`high-performance` still pick by power class, and an explicit choice in `config.json` is kept.

Why: a frame drawn on one GPU and shown by another is copied across every frame, and the glass
does not fit the iGPU's budget. Live on this desktop (rain-city at 30 fps, one screen visible,
the other covered by a maximized window and paused; 60 s each, `GPU Engine` running time per
process, private bytes):

| Engine on | Engine GPU busy | Cross-adapter cost | Engine private | Engine CPU (one core) |
|---|---:|---:|---:|---:|
| AMD iGPU (`integrated`) | iGPU 3D 294 ms/s + copy 17 ms/s | **+43 ms/s of the RTX's 3D engine**, billed to the engine, to put iGPU frames on RTX-driven monitors | **262.0 MiB** | 2.25% |
| RTX 4070 SUPER (`auto`) | RTX 3D 9.5-44.8 ms/s (two runs) | none | **179.9, 197.6 MiB** | 3.15, 4.32% |

Glass per screen (the lab fixture above): **2.48 ms** glass-only and 2.92 ms with a new
wallpaper frame on the iGPU, against a 2 ms budget; **0.08 / 0.12 ms** on the RTX. So the
default fits the budget with room to spare. Memory is 64-82 MiB lower on the RTX: its textures
live in dedicated VRAM, and there is no cross-adapter swapchain overhead (docs/engine-notes.md,
Memory: +63 and +40 MiB for two swapchains presented across adapters); both screens visible on
the RTX was not measured today. Engine CPU read higher on the RTX (3.15 and 4.32% against
2.25%); these runs could not separate that from the desktop's activity (other agents restarted
Glaze and built during them, and the user's GPU load changed), so it is not a finding yet.

Power: no clean whole-system number. The RTX board read 56 W during the iGPU run and 182 W
during the second RTX run, because the user's own GPU work changed (video decode 159 ms/s, other
3D work); RAPL package power (CPU plus iGPU) read 50-62 W with builds running. The engine's own
share is the GPU busy time above: about 3% of the RTX against 31% of the iGPU plus 4% of the
RTX. An RTX that already drives the monitors is awake whatever the engine does; the extra cost of
rendering there is that busy time at its idle clocks.

Studio shows the choice as "Your screens' card" with the adapter's name, beside Integrated and
High performance, and the frame rate as before.

## Thin glass: a lens to scale (5 November 2026)

The kit's 30 pt lens is deeper than a 24 pt menu bar is tall. The bevel was clamped only to half
the bar, so the whole bar was curved, and the rim light (5 pt blur, offset 40 pt) and rim shade
(15 pt) met in the middle: the labels sat on the highlight. iOS gives thin glass (menu bars,
pills, small buttons) a shallow lens with a flat face.

Now the lens is at most a fifth of the smaller side (`liquid_rust::material::lens`,
`LENS_PER_SIDE`), capped at the material's own depth; the rim light, rim shade and their offset
narrow by the same factor. The cap applies from about 166 pt for regular glass (150 pt for
clear), where the factor is exactly 1, so large glass draws exactly as before. A 24 pt bar
gets a 4.8 pt lens and rims at 0.16 of the kit's.

Proof (frame lab, iGPU, rain-city at 3 s and 12 s, `GLAZE_LAB_PLATES` with the 2560 x 24
strip, small pills and buttons, the seven live widget plates and two large panels; each plate's
pixels plus an 8 px ring, other plates' pixels left out):

| Plates | Largest difference |
|---|---:|
| 180, 206, 313 and 400 pt (six plates) | **0/255** (bit-identical) |
| 150 pt | 4/255 in about 1% of its pixels |
| 99 pt widgets | 11-19/255 |
| 24-56 pt bar, pills and buttons | 9-17/255 |

Brightness down the bar (mean of 1100 columns, rows 0-23): before 95.8 at the top edge, falling
to 77.3 mid-bar and rising to 95.7 at the bottom, a gradient under every label; after, a crisp
top line (90.3), a flat face (77-79) from row 2 to row 21, and the bottom rim in rows 22-23. The
glass costs no more: glass-only frames 2.39 ms median on the iGPU (2.48 ms before, the fixture's
40 pt bar now has a shallower lens).

## Engine parity (6 November 2026)

C10, C12 and C14 finish the [bars handoff](bars.md#finer-glass-controls-2026-11-06).
For smoothed overlays, `Material::clip_to_shape` makes the shader's own continuous coverage the
clip and suppresses its outer shadow/hairline; DirectComposition supplies only the rectangular
bound. The circular/default path is unchanged. A visible restyle commits its new clip with the
matching shader frame, avoiding an intermediate mismatched contour.

The bars' legacy `look.glass.depth` now maps, times glass intensity, to
`glaze_glass_ipc::Style::inner_shadow` and overlay `request::Style::inner_shadow`.
This is a bottom-facing inset shade, independent of the engine's existing top-rim `depth`.
Unset is zero. It resolves with the plate/overlay material, and at zero the shader skips its
distance/blur calculation entirely. It occupies an unused GPU material slot; GPU buffers keep
their existing sizes. `glaze_ui::glass::GlassStyle` carries the same field through
`Tokens::glass_style`, menus, flyouts and bars.

Glass takes the same resolved show/open duration and curve as the content, including the full
5,000 ms limit, every named curve, custom cubic Béziers and physical springs. The UI carries
`GlassStyle::fade_curve: Option<transition::Easing>`; the wire carries
`request::FadeCurve::{DEFAULT, bezier([f32; 4]), spring(mass, stiffness, damping, seconds)}`.
At first show, the engine builds DirectComposition polynomial segments with adaptive subdivision;
no engine animation frames or curve-sampling timer were added. The default wire curve retains
the original cubic polynomial exactly. Disabled/reduced motion sends zero milliseconds and
settles opacity immediately. Glass slides remain a stationary fade to avoid capture flashes.

Offline verification (`CARGO_TARGET_DIR=C:\glaze-targets\engine-perf`):

- `lab::lab`, rain-city, RTX 4070 SUPER, 2560x1440, eight plates, captures at 3 and 12 seconds:
  default final and kept frames are **100% bit-identical**, largest difference **0/255**.
- GPU timestamps, 150 frames per run: baseline glass-only median **0.090 ms**, after runs
  **0.088 and 0.092 ms**; glass on wallpaper-change frames **0.127 ms** before and
  **0.101 / 0.114 ms** after. No added shadow SDF/blur work at defaults and no sustained GPU
  regression observed; the small timing differences are not claimed as an improvement.
- `lab::glass_parity` renders a transparent 512x320 continuous overlay fixture. Its new bottom
  shadow leaves the top pixel and every alpha byte unchanged; bottom BGRA changes from
  `[181,196,210,255]` to `[150,166,180,255]`. At radius 48, 18 sampled corner pixels retain
  alpha that a circular clip would trim (up to 49/255). The outside stays transparent.
- Curve tests compare compositor pieces against 10,001 samples plus endpoint probes, within
  one alpha step, including extreme Béziers and the fastest permitted undamped spring. Shell
  tests match every named curve, a custom Bézier and an overdamped spring to the content's
  resolved transition. Five-second duration and reduced-motion policy are covered.
- Compatibility: the original 1,928-byte style section keeps its five-word stride. Tests cover
  mismatched/missing extension stamps and a real message-only legacy receiver that rejects
  104 bytes then accepts the 80-byte fallback with its original optics and 2,000 ms cap.

Raw lab frames/timestamps are `C:\glaze-targets\engine-parity\{before,after,after-final}`;
the inspected overlay images and metrics are in its `overlay` directory. Reproduce the overlay
fixture with `GLAZE_PARITY_OUT=<directory>` and
`cargo test -p glaze-wallpaper lab::glass_parity -- --ignored --exact --nocapture`.
The main lab additionally accepts `GLAZE_LAB_STYLE=brightness,saturation,smoothing,sheen,depth,inner_shadow`
(negative sheen/depth means unset), applied to captures and timed frames.

The maximum new-shadow/squircle fixture (strength 1 on all eight plates) took **0.096 ms** median
glass-only GPU time on the RTX, within the 2 ms gate. The inspected contact sheets are
`C:\glaze-targets\engine-parity\rain-verification.png` and `overlay\verification.png`.

Live verification used guarded input on Glaze's own dock, with DPI-aware 2560x1440 screenshots.
The inspected continuous menu faded for five seconds: a screenshot fit of its content opacity was
0.14 at 0.75 s, 0.30 at 1.52 s, 0.60 at 3.00 s, and 1.00 at 5.31 s. This fit is supporting
image evidence, not a compositor API reading. Plate/style/inner-shadow sections shared stamp 70:
bars carried smoothing 0.6, inner shadow 1 and unset top-rim depth; widgets retained neutral optics
and inner shadow 0. The first live menu's glass arrived in 42 ms (warm capture), separate from its
fade duration.

The final full shared-checkout release workspace build passed and was deployed through
`scripts/deploy-local.ps1 -From C:\glaze-targets\main\release` at 07:38. A later attempt to
capture custom Bézier/reduced-motion/shadow variants stopped before input when the foreground
safety guard refused activation; those extra live variants remain unverified. Their resolver,
wire and GPU/curve behavior are covered by the offline checks above. No user app was clicked.
The original `bars.json` and `config.json` hashes were restored/unchanged:
`88032FB713BEDDF3D3B8386859655E7A0E2C05DCBDB2F8E9718C5607C0C61BA0` and
`B0A117160D59CB8698E47DC874E501C585B2B7B67ADF6B906191EF99A7F9F538`.
At 07:43, deployed engine/shell hashes matched the shared `main` build:
`3DEB2FD3A14910B50B68E54222DD3351DBEE05D32F9D815EEF8EA24993F0059B` and
`7CFF809E907D79CBBF616EA75A84F59471C0765D3D0F203A23F53E864918C0EF`.
The running PIDs were core 59688, engine 85520, shell 28576. At handback, the 07:43–07:53
measurement hold remained in force; no further desktop-lock use or restarts are needed for this task.

Live artifacts are `%TEMP%\glaze-tools\engine-parity`: `menu-linear-*.png`,
`linear-verification.png`, `linear-opacity-fit.json`, `linear-plates.json`, `bezier-plates.json`
and `deploy-shared.log`. Crate checks/tests and Clippy passed (pre-existing warnings retained):
renderer 61 tests, IPC 4, UI 62, shell 146 plus the targeted engine/wire/curve tests (103 engine
tests in the full run, then the added legacy-receiver test). The first GPU shadow fixture caught
top-edge shading; bottom-facing normal weighting fixed it, and the fixture then passed.

## Glass over web and app wallpapers (P14, 6 November 2026)

A `web` or `app` wallpaper draws with windows of its own (WebView2, an embedded app), so the
engine has no kept frame to draw the shell's plates into, and widgets and bars had no glass there.
Now the engine draws those plates as overlays itself (`Overlays::sync_plates`,
`Engine::native_plates`). The shell is unchanged: it publishes plates as always, and each screen
takes the route its source needs.

- **Which plates.** Those on a screen whose source is native (`Source::native`) and whose centre
  is on that screen. A web page's held still is not native (the swapchain shows it), so a screen
  next to a fullscreen game gets plates drawn over its still as before.
- **Which window.** A plate's id carries its surface's window in its low 32 bits (widgets use their
  HWND; bars their island's, plus the shape index above bit 32; `glaze_glass_ipc::Plate::id`).
  The overlay goes directly under that window, if it is a live window of the plates' publisher:
  in the desktop band for a widget (`Layer::Desktop`, which the kit keeps together with its
  widget, `Surface::keep_above_desktop`), topmost under a bar (`Layer::Popup`). These overlays are
  keyed by plate id, apart from the shell's own requests for the same window (a bar over an app
  window still opens its own, P15).
- **Same glass.** The plate's material (with its stained tint, carried in the overlay material's
  fourth tint byte, which the shell leaves at 0), style (brightness, saturation, smoothing,
  sheen, depth, inner shadow) and corners; it fades in over 300 ms like other overlay glass.
- **Refresh follows the wallpaper.** The capture delivers a frame only when that monitor changes;
  frames whose dirty rects miss every overlay are dropped on WGC's thread, and a crop that is
  touched is compared on the GPU before any glass is drawn. A still page costs no glass passes.
  An animated one redraws its plates at most 30 times a second, or at the theme's `fps` when that
  is lower (`Overlays::set_fps`, the capture's minimum update interval).
- **Stops.** On a covered or held screen (a maximized window, a game, paused) the overlays are
  *frozen*: the glass stays as last drawn, nothing is captured for them, and a monitor with only
  frozen overlays has no capture session. Released (every screen covered for 10 s), or another
  kind of wallpaper: they close. A plate first seen on a covered screen waits until it is seen.

Other routes, and why not: the static snapshot only has stills; capturing the wallpaper's own
window would still need a window above the WebView to show the glass in, which is what the overlay
route already has (and its monitor capture already leaves our windows out); WebView2's
`CapturePreview` is a PNG encode per frame.

**Measured** on the live desktop (RTX 4070 SUPER, DISPLAY2 visible with seven glass plates: six
widgets and the dock, the top bar draws none; DISPLAY1 covered), 30 s each with `measure.ps1`, the theme
switched in place (`webglass/ab.ps1`), CPU in % of one core. Glass updates and passes come from the
engine's overlay totals over the time the theme was shown.

| `themes/examples/webdemo` (animated, 30 fps) | Engine CPU | Engine GPU 3D | Engine private | WebView2 CPU | Glass updates/s (passes/s) |
|---|---:|---:|---:|---:|---:|
| Before (no glass, 08:01 build) | 0.21 % | 0 % | 169 MB | 9.6 % | - |
| After | 5.83 % | 6.81 % | 206 MB | 12.1 % | 29.2 (198) |
| After, a still page (`still-web`) | **0.31 %** | 0.12 % | 210 MB | 0.15 % | 1.7 (1.8) |
| After, the web demo under `"engine": { "fps": 10 }` (`web-10fps`) | 1.41 % | 2.93 % | 211 MB | 8.5 % | 9.7 (66) |

So a still page costs what the wallpaper did (its few updates are windows overlapping widgets, such
as Studio's, whose glass is under them), and an animated page costs one glass update per
captured frame for every plate it touches: here about 0.28 ms of engine CPU per overlay pass, and
+37 MB for the capture device, DirectComposition and each overlay's crops. Lower the page's own
rate, or the theme's `engine.fps`, to lower both.

Captures (DPI-aware, 2560x1440 of DISPLAY2) are in `C:\glaze-targets\engine-perf\webglass`:
`webglass-web-before-1.png` (no glass under the widgets or the dock), `webglass-web-after-2.png`,
`webglass-still-after.png`, and crops `widgets-{before-1,after-2}.png`, `dock-{before-1,after-2}.png`.

**Known limit.** Two engines at once (one still exiting while its replacement starts, seen once
tonight during another agent's lifecycle test) each capture the other's glass, which feeds back
into red and blue blocks under the bars until the old one exits. A normal restart stops the old
engine first.

## Images

- Before (two outlines, one less rounded): `media/native/glass-before-widgets.png`
- After, widgets (one engine edge, frost, rim light, shadow): `media/native/glass-after-widgets.png`
- After, Quick flyout over an app (overlay): `media/native/glass-after-flyout.png`
- Lab render over rain-city at the kit's defaults: `media/native/glass-lab-rain.png`
- Top bar (24 pt) before/after the thin-glass lens, the bar's ends at 3x, glass alone and with
  its content: `media/native/glass-topbar-bevel.png`
- Apple references: `C:\glaze-targets\refs\liquid-glass` (macOS Tahoe Control Center, iOS 26).
