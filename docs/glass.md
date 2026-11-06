# Liquid Glass

All of mypaper's glass (bar islands, widgets, flyouts, menus, Studio's previews) is drawn by the
engine with **liquid-rust** (<https://github.com/al1h3n/liquid-rust>, by al1h3n, MIT OR
Apache-2.0), vendored in `crates/liquid-rust` (see its `VENDORED.md` for the commit and the few
marked `mypaper:` changes). Its look is calibrated against Apple's iOS 26/27 Figma kit, and we
keep its shaders, pipeline and parameter values as they are.

## The model (Apple's Liquid Glass, as liquid-rust draws it)

1. **Lensing, not blur, carries the look.** The backdrop is refracted through a squircle bevel
   (Snell's law, η 1.5). Content bends strongly at the rim and lies flat in the middle. The bevel
   is `depth` deep (30 pt, more for large glass), with a small per-channel dispersion. Thin glass
   gets a shallow lens (mypaper, below): at most a fifth of its smaller side.
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
   (widgets, bar islands, flyouts and the desktop menu only take clicks there). We use quarter-circle
   corners (`Shape::Circular`) because our surfaces are D2D rounded rectangles. Apple's
   continuous corners (`Shape::Rounded`) are one switch away once the UI kit draws squircles.
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
Plates: `mypaper_glass_ipc::Style`, sent with `Writer::publish_styled(&plates, &styles)` (one per
plate, or empty for all defaults). Overlays: `request::Style` in `Request::Style { client, style }`,
sent after each `Open` (a new overlay starts at `Style::DEFAULT`).

| Field | Range, default | What the engine draws |
|---|---|---|
| `brightness` | 0..=2, 1 | multiplies what's behind the glass (refracted and frosted), before tone mapping |
| `saturation` | 0..=3, 1 | saturation of what's behind (0 grey) |
| `smoothing` | 0..=1, 0 | corner smoothing: 0 quarter circles, 0.6 iOS's continuous squircle (`Shape::Smooth`); the surface must draw the same corners |
| `sheen` | 0..=1, unset | the kit's rim light inside the lit (top) edge, 0.5 = the kit's; unset (`None`, or negative on the overlay wire) follows `highlight` |
| `depth` | 0..=1, unset | the kit's broad rim shade under the lit edge, 0.5 = the kit's; unset follows `highlight` |
| `fade_ms` (overlays only) | 0..=2000, 300 | how long the glass fades in when it first shows; 0 shows it at once (reduced motion) |

Plate blur (`Material::blur`, physical px) now takes up to 256 px (64 DIP at 400 %); frost still
tops out at 64 pt. Styles resolve into the glass's material and shape only when a plate (or
overlay) changes, never per frame; brightness and saturation are one multiply-add in the glass
shader, skipped when both are 1. Plate styles travel in a second section stamped with the
snapshot's sequence, and only a `Style` request is longer than the old overlay wire, so a shell
or engine on either side of the change keeps working with defaults.

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

GPU per screen for the glass pass, median of 150 frames. `MYPAPER_LAB_GPU=high` runs the lab on
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
(`mypaper_desktop::display_gpu`, used by the engine's `gpu.rs` and by Studio). Here that is the
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
mypaper and built during them, and the user's GPU load changed), so it is not a finding yet.

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

Proof (frame lab, iGPU, rain-city at 3 s and 12 s, `MYPAPER_LAB_PLATES` with the 2560 x 24
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

## Images

- Before (two outlines, one less rounded): `media/native/glass-before-widgets.png`
- After, widgets (one engine edge, frost, rim light, shadow): `media/native/glass-after-widgets.png`
- After, Quick flyout over an app (overlay): `media/native/glass-after-flyout.png`
- Lab render over rain-city at the kit's defaults: `media/native/glass-lab-rain.png`
- Top bar (24 pt) before/after the thin-glass lens, the bar's ends at 3x, glass alone and with
  its content: `media/native/glass-topbar-bevel.png`
- Apple references: `C:\mypaper-targets\refs\liquid-glass` (macOS Tahoe Control Center, iOS 26).
