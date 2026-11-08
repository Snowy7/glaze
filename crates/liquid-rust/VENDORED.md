# Vendored: liquid-rust

Source: <https://github.com/al1h3n/liquid-rust>, commit `2073cd96c48458093ae3ea23e14e1162e84d5e80`
(crate version 0.2.5), by al1h3n. Licensed under either of MIT (`LICENSE-MIT`) or Apache-2.0
(`LICENSE-APACHE`), at your option; both licence texts are kept here unchanged, and every
vendored file starts with an attribution header.

`src/` is upstream's `src/`, copied as-is (commit `17cc402` in this repo), then changed only where
Glaze's architecture forces it. Glaze's engine draws all Liquid Glass through it:
`crates/wallpaper/src/glass_pass.rs` for plates in the wallpaper frame, and
`crates/wallpaper/src/overlay/glass.rs` for overlays over app windows. Upstream's examples, GPU
tests and web wrapper are not copied.

## Local changes

Each is marked `Glaze:` in the source.

- `Frame::copy_content`: `false` skips the content-to-target blit. Plates draw over the
  swapchain image the engine has already restored, whose kept wallpaper can be another size.
  Overlays draw over a transparent clear, so DirectComposition shows only the glass and its
  shadow (premultiplied).
- `glass.wgsl` samples the content by uv for the edge burn instead of `textureLoad` by pixel, so
  the content can be another size than the target.
- `Glass::appearance` / `Scene::set_appearance`: per-glass colour scheme and transparency slider.
  One engine scene holds bars and widgets from different styles. The scheme rides in
  `MaterialGpu.shadow.w` (unused upstream), and `glass.wgsl` reads it there instead of
  `globals.misc.z`.
- `Renderer::fork`: a renderer that shares compiled pipelines, so opening an overlay compiles no
  shaders.
- `Material::outset`: how far glass draws or samples past its frame, which sizes an overlay's
  window.
- `Profiler` / `Renderer::profiler`: optional GPU timestamps between the renderer's passes, for
  the engine's frame lab (`crates/wallpaper/src/lab.rs`). Off (`None`) unless the lab sets it.
- `glass.wgsl`, the same picture for less ALU (the lab's frames differ by at most 1/255, in
  rounding): a lone shape's backdrop statistics (shadow adaptivity, light face) are read once
  per vertex instead of per pixel; only the tone face in use is mapped; `refract_offset` is
  Snell's law in closed form instead of atan/asin/tan; the lit rim and the rim shade share one
  hole; the touch glow is skipped while it adds exactly zero.
- Pyramid rebuilds cover only what each group reads (`Reads`, `Layout::rects`): its frost
  levels over its bounds (frost samples inside its shapes), the statistics texel at its centre
  (and the top level for glass that flips with the content), each with the exact halo the
  levels above need. Upstream rebuilt the bounding box of all glass plus its reach, every level.
  The cache keeps rects per level, so content that holds still adds only what newly moved glass
  reads (a second screen sharing the content adds its own part), and is blurred whole once its
  cache passes 16 rects per level. Same pixels as a full rebuild.
- Thin glass gets a lens to scale (`material::lens`): the bevel is at most a fifth of the smaller
  side, capped at the material's depth, and the rim light, rim shade and their offset narrow by
  the same factor (`ShapeGpu.extra.y`, blended with the depth as shapes fuse). Upstream's 30 pt
  lens covered a 24 pt menu bar edge to edge. The factor is exactly 1 from about 166 pt, so large
  glass is bit-identical (`docs/glass.md`, Thin glass).
- Independent `Material::inner_shadow`: a bottom-facing inset shade, skipped at zero. The
  strength uses `MaterialGpu.dark_b.z`, so buffers retain their original size. `clip_to_shape`
  uses `.w` to suppress the outer shadow/hairline for continuous overlays: their shader coverage
  defines the contour, with a rectangular DirectComposition bound. Both default to zero/off,
  preserving the default lab frames bit for bit (`docs/glass.md`, Engine parity).
