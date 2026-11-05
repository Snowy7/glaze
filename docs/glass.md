# Liquid Glass

All of mypaper's glass (bar islands, widgets, flyouts, menus, Studio's previews) is drawn by the
engine with **liquid-rust** (<https://github.com/al1h3n/liquid-rust>, by al1h3n, MIT OR
Apache-2.0), vendored in `crates/liquid-rust` (see its `VENDORED.md` for the commit and the few
marked `mypaper:` changes). Its look is calibrated against Apple's iOS 26/27 Figma kit, and we
keep its shaders, pipeline and parameter values as they are.

## The model (Apple's Liquid Glass, as liquid-rust draws it)

1. **Lensing, not blur, carries the look.** The backdrop is refracted through a squircle bevel
   (Snell's law, η 1.5). Content bends strongly at the rim and lies flat in the middle. The bevel
   is `depth` deep (30 pt, more for large glass), with a small per-channel dispersion.
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
item. It is ALU-bound: the second frost level costs 0.39 ms, dispersion 0.63 ms, the inner
shadows 0.35 ms and the rim light 0.24 ms (ablations, not shippable). The cheapest approximation
measured, one frost sample where the dispersion's R-B split is under 0.5 px, saves 0.22 ms and
differs by at most 1/255 in 0.06% of pixels. It isn't shipped.

GPU memory: +28 MiB private for two screens on separate kept frames (two half-resolution
Rgba16Float pyramids, 9.4 MiB each, of which level 0 is 7 MiB); about half that when the kept
frame is shared. Still wallpaper and still plates cost nothing.

The GPU (`engine.gpu`: integrated, high-performance or auto) and frame rate (`engine.fps`, 1 to
360) are engine settings, per wallpaper too; overlays open on the engine's adapter. Integrated
stays the default.

## Images

- Before (two outlines, one less rounded): `media/native/glass-before-widgets.png`
- After, widgets (one engine edge, frost, rim light, shadow): `media/native/glass-after-widgets.png`
- After, Quick flyout over an app (overlay): `media/native/glass-after-flyout.png`
- Lab render over rain-city at the kit's defaults: `media/native/glass-lab-rain.png`
- Apple references: `C:\mypaper-targets\refs\liquid-glass` (macOS Tahoe Control Center, iOS 26).
