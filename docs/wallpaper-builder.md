# Wallpaper builder

Studio's **Create** mode builds a wallpaper from a **base** plus a stack of **layers**: effects,
images and 3D models. The result is saved as a normal theme folder with `"kind": "stack"`, so it
can be shared and shows up in the theme browser later.

This file is the contract between the engine (`crates/theme`, `crates/wallpaper`), the effect
packages (`effects/`) and Studio (`crates/studio`). If one side needs a change, make it additive
and update this file in the same commit.

## Theme format

```jsonc
{
  "name": "Rainy Oslo",
  "wallpaper": {
    "kind": "stack",
    "fps": 30,        // cap while something moves (default 30)
    "scale": 1.0,     // render scale while something moves, 0.25..1 (default 1)
    "base": { "kind": "image", "source": "media/photo.jpg", "fit": "cover", "focus": [0.5, 0.5] },
    "layers": [
      { "id": "rain-1", "kind": "effect", "effect": "rain", "enabled": true,
        "mix": 1.0, "params": { "amount": 0.6, "tint": "#c8d8ff" } },
      { "id": "logo", "kind": "image", "source": "media/logo.png",
        "rect": [0.70, 0.10, 0.20, 0.20], "blend": "normal", "mix": 1.0 },
      { "id": "planet", "kind": "model", "source": "media/planet.glb",
        "rect": [0.05, 0.40, 0.40, 0.50], "orbit": 6.0, "parallax": 0.3, "light": [0.4, 0.8, 0.5] }
    ]
  }
}
```

- Paths are relative to the theme folder. When an image or model is imported, Studio copies it
  into `media/`.
- `rect` is `[x, y, w, h]` as fractions of the screen.
- `mix` (0..1) blends the layer's output over its input.
- `blend` is one of `normal`, `add`, `screen` or `multiply` (default `normal`), on every layer kind.
- `enabled: false` keeps the layer in the file but skips it.
- Bases and layers ignore fields they do not know, so a newer Studio can add one without
  breaking an older engine. Layer `id`, `enabled` (true) and `mix` (1) are optional.
- Rust types: `glaze_theme::stack` (`StackWallpaper`, `Base`, `Layer`, `EffectManifest`).
- `params` lists only values that differ from the effect's defaults. Unknown params are ignored,
  so an effect can drop a param without breaking older themes.

### Bases

| `kind` | Fields |
|---|---|
| `image` | `source`, `fit` (`cover`/`contain`/`stretch`/`center`), `focus` [x, y] |
| `video` | `source`, `fit` |
| `color` | `color` "#rrggbb" |
| `gradient` | `stops` [{ `color`, `at` 0..1 }], `angle` in degrees, `type` `linear`/`radial` |
| `shader` | `source` .wgsl defining `fn wallpaper(frag: vec2f) -> vec4f` (the `shader` theme contract), `params` {} |
| `glsl` | `source` .glsl in Shadertoy style (`mainImage`), plus `author`, `license` and `url`, which are kept for credits |
| `scene` | the `scene` theme fields (`source` .glb/.gltf, `orbit`, `parallax`, `background`, `interactive`), plus `environment` (`sky`/`color`/`transparent`) and `exposure` |
| `theme` | `theme`: another theme's id (its folder name, resolved like `config.theme`) or path. Its own source renders offscreen as the base, exactly as it would on the desktop: a shader keeps its images, prefilters and bakes. Web and app wallpapers (child windows) and other stacks can't be a base |

A `scene` base also provides **depth**. Effects that read depth, such as depth of field and fog,
use it. Any other base gives a flat depth of 1.0.

### Layer kinds

| `kind` | What it does |
|---|---|
| `effect` | Runs an effect package over the input. Fields: `effect` (id), `params`, `blend` |
| `image` | Draws a PNG/JPG/WebP into `rect`, with `blend` |
| `model` | Renders a glTF into `rect` with a transparent background, then composites it. Fields: `orbit`, `parallax`, `light`, `scale`, `rotation` [x, y, z] in degrees, `animation` (glTF clip name, or `null`) and `blend` |

## Effect packages

An effect is a folder. The engine looks for it in three places, in this order:

1. `%APPDATA%\glaze\effects\<id>/` for user effects;
2. the built-ins, in `effects/<id>/` next to the exe (the repo's `effects/` folder);
3. the theme's own `effects/<id>/`.

Built-in effects use exactly the same format as everyone else's. Studio packages every used
effect into a saved theme so it travels when shared, but those copies are only a fallback: an
id this install knows always runs the current effect, so a fix reaches old themes too. A
theme's own custom effects (`custom-…`) are found in its folder.

`effect.json`:

```jsonc
{
  "id": "rain",
  "name": "Rain on glass",
  "category": "weather",   // weather | camera | color | motion | stylize | light
  "description": "Drops and streaks running down a window.",
  "author": "Glaze", "license": "MIT", "url": null,
  "animated": true,        // false: the result can be baked into a still
  "cost": 1.2,             // measured GPU ms at 1920x1080 on the iGPU, at defaults
  "cost_estimated": false, // true when cost is an estimate rather than a GPU measurement
  "reads": ["depth"],      // optional: "depth", "original"
  "passes": [ { "source": "rain.wgsl", "scale": 1.0 } ],
  "params": [
    { "id": "amount", "label": "Amount", "type": "float", "min": 0, "max": 1, "default": 0.6, "step": 0.01 },
    { "id": "tint",   "label": "Tint",   "type": "color", "default": "#ffffff" },
    { "id": "streaks","label": "Streaks","type": "bool",  "default": true },
    { "id": "style",  "label": "Style",  "type": "enum",  "options": ["drizzle", "storm"], "default": "drizzle" }
  ]
}
```

### WGSL contract

Each pass defines this function:

```wgsl
fn effect(frag: vec2f, uv: vec2f) -> vec4f
```

`frag` is the pixel position (top-left origin) and `uv` is `frag / paper.resolution`. The engine
prepends `effects/prelude.wgsl`, which provides:

- `paper`: resolution, mouse, time and frame as in the theme shader prelude, plus
  `clock_hours`: the current real local hour, 0..24 with fractional minutes, read each frame
  (also refreshed after pause/resume). Clock-driven effects must set `animated: true`;
- `src` / `src_sampler`: the input, which is the previous layer or the previous pass;
- `original`: the base before any layer;
- `layer_input`: this effect's input before its first pass, retained across passes so a
  downscaled blur or bloom can composite over the sharp preceding stack;
- `depth`: 0 is near and 1 is far; flat 1.0 when the base has none;
- `params`: a uniform struct generated from `params`, in declared order:
  - `float` → `f32`
  - `color` → `vec4f` (linear RGBA)
  - `bool` and `enum` → `u32` (an enum is its option's index)

  The fields are named exactly as the param `id`s.
- helpers: `sample_src(uv)`, `hash21`, `noise2`, `fbm2`, `luma`; also `sample_original(uv)`,
  `sample_layer_input(uv)`,
  `sample_depth(uv)` (depth is not filterable on every GPU, so never sample it with
  `src_sampler`) and `to_linear` / `to_display`. `src` and `original` hold display
  (sRGB-encoded) values; `color` params are linear.

Bindings, all in group 0: `src` 0, `src_sampler` 1, `original` 2, `depth` 3, `params` 4
(uniform), `layer_input` 5; `paper` is immediate data. `paper.resolution` is the pass's own target size. The
entry points `glaze_vs` / `glaze_fx` are the prelude's, so a pass must not define them.

To validate exactly what the engine compiles, use `glaze_theme::stack::pass_wgsl` (the
prelude, then the generated `struct Params`, then the pass; `first_line` maps errors back to
the pass file) or, with the crate's `check` feature, `stack::check::pass`, which runs naga and
reports the pass file's line. `stack::check::glsl` does the same for a Shadertoy base.

A multi-pass effect, such as a separable blur, lists several passes. Each pass reads the previous
pass as `src`. A pass may run at a lower `scale`; the engine upsamples its output.

Rules the engine's real path imposes (every bundled effect is checked against them by
`crates/wallpaper/src/sources/stack/effects_real.rs`, through the same `StackSource` Studio's
preview and the desktop use):

- The engine compiles with **FXC** (DX12). Never write a vector component at a dynamic index
  (`v[i] = x`): FXC miscompiled exactly that into a black `crt`. Build the value with
  `select`/`mix` instead.
- Size things in 1080p units (`paper.resolution.y / 1080`, or `uv`), never in raw pixels, so an
  effect looks the same on a 1080p and a 1440p screen. Keep a whole-pixel pitch for patterns
  that would otherwise alias (scanlines, phosphor masks).
- Tone curves must preserve low-key pictures too. Halftone derives ink coverage from each
  cell's input brightness and the chosen ink/paper brightness, including joined dots in shadows;
  stretching a dark photo's range or capping dot coverage floods it with paper.
- Defaults must visibly change a photo: adding an effect that does nothing reads as broken.
- Studio writes `params` as JSON: a color as `"#rrggbb"`, a bool as `true`/`false`, an enum as
  its option name (an index also works).

### Shader params in Studio

Studio turns these comments in a shader base into sliders, the same way it does for effects:

```
// @param amount float 0..1 = 0.5 "Amount"
// @param tint color = #ffffff "Tint"
// @param streaks bool = true "Streaks"
// @param style enum drizzle|storm = drizzle "Style"
```

`glaze_theme::stack::shader_params` parses them. The engine appends the generated
`struct Params` and a `const params = Params(...)` built from the base's `params` values, so the
shader reads `params.amount` with no binding; a change recompiles the shader.

### Shadertoy bases

A `glsl` base defines `void mainImage(out vec4 fragColor, in vec2 fragCoord)`. The engine wraps
it (`stack::glsl_fragment`) with a std140 block of `iResolution`, `iTime`, `iMouse`,
`iTimeDelta`, `iFrame`, `iFrameRate` and `iChannelResolution`; `fragCoord` has Shadertoy's
bottom-left origin. `iChannel0..3` all sample one 256x256 tiling RGBA noise texture with a
repeating linear sampler, like Shadertoy's noise inputs, so common imports work as they are.

### Errors

An effect that fails to load or compile is skipped and the rest of the stack keeps drawing. The
engine writes `%LOCALAPPDATA%\glaze\stack-errors.json` on every load of a stack:
`{ "theme": "<folder name>", "errors": [{ "layer": "rain-1", "effect": "rain", "line": 12,
"message": "..." }] }` (`line` is in the pass file, or `null`; `layer` is `"base"` for the base).

## Cost rules

- **A still stack costs nothing at rest.** A stack is still when no layer is animated, the base is
  not video, shader or scene, and no param is time-driven. The engine renders it once per screen
  size and keeps it as the frame, exactly like an `image` theme.
- **A `theme` base follows its theme.** A still image theme bakes like an `image` base. Any other
  theme moves at its own rate (a 30 fps shader stays 30 fps; a slideshow keeps its own schedule while
  no layer moves).
- **A moving stack is split.** The static prefix of the layers (everything before the first
  animated layer) is baked once. Only the rest runs at `fps`, at `scale`.
- Studio shows the sum of the `cost` of the active layers that run every frame, scaled by
  resolution, fps and scale, as an estimate such as "about 2% GPU at 30 fps". Studio and the
  engine split at the same layer (`glaze_theme::stack::per_frame_from`). A layer is active when
  it is enabled and its `mix` is above 0; the engine skips the others.
- The engine's resting rules (pause while covered, refresh once when changed) apply unchanged.

## Studio's Create mode

Create mode is a page under Wallpapers. Every wallpaper card has **Edit** (on hover and in its menu):
a stack opens as it is; any other kind opens a new draft whose base is `{"kind": "theme"}` of that
wallpaper. Saving never changes a built-in theme: an edit of one is saved to your themes as
"<Name> (edited)". Your own stack saves in place (**Save as copy** makes a new one). Web and app
wallpapers show Edit disabled, because they draw in child windows.

- **Canvas:** the big live preview, showing the real engine output on a stage.
- **Side panel:**
  - the base picker: import an image or video, a color or gradient, a shader (built-in or
    written), or a 3D scene;
  - the layer list: add from an effect gallery grouped by category, reorder, toggle, duplicate
    and delete;
  - the selected layer's params, generated from its schema.
- **Shader editor:** WGSL or GLSL text with live reload. Errors from naga are shown on their
  lines, and `@param` comments become sliders.
- **Saving** writes a theme folder under `%APPDATA%\glaze\themes\<slug>/`, copying in the
  media, any custom effects, a `preview.jpg` and a credits entry for imported shaders.

### Draft preview

Studio previews a draft without touching the desktop by running a headless child:

```text
glaze-wallpaper --preview <draft dir> --out <file.jpg> [--size 1280x720] [--fps 10]
```

It renders the draft offscreen and rewrites `<file.jpg>` atomically (`.tmp`, then rename) at start,
on every change to the draft folder or a used effect folder, and at `--fps` while the stack moves.
Its own output, `*.tmp`, `*.log`, `preview.*` and `CREDITS.md` changes do not reload it. Studio
ends it by ending the process.

Video sources return from loading with their first decoded frame already bound/uploaded
(decoder startup is bounded to 10 seconds). The first output is therefore a real video frame,
including when an offscreen caller renders before its first update. No CLI or JPEG format change.

For diagnostic software rendering, set `GLAZE_FORCE_WARP=1` before launching the child.
It forces wgpu's DX12 fallback adapter and bypasses the display-GPU preference. Other values
leave normal adapter selection in effect. This does not emulate missing Windows media codecs.
