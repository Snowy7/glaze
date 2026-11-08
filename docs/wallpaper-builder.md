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
- Rust types: `mypaper_theme::stack` (`StackWallpaper`, `Base`, `Layer`, `EffectManifest`).
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

1. the theme's own `effects/<id>/`;
2. `%APPDATA%\mypaper\effects\<id>/` for user effects;
3. the built-ins, in `effects/<id>/` next to the exe (the repo's `effects/` folder).

Built-in effects use exactly the same format as everyone else's.

`effect.json`:

```jsonc
{
  "id": "rain",
  "name": "Rain on glass",
  "category": "weather",   // weather | camera | color | motion | stylize | light
  "description": "Drops and streaks running down a window.",
  "author": "mypaper", "license": "MIT", "url": null,
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

- `paper`: the same `Paper` struct as the theme shader prelude (resolution, mouse, time, frame);
- `src` / `src_sampler`: the input, which is the previous layer or the previous pass;
- `original`: the base before any layer;
- `depth`: 0 is near and 1 is far; flat 1.0 when the base has none;
- `params`: a uniform struct generated from `params`, in declared order:
  - `float` → `f32`
  - `color` → `vec4f` (linear RGBA)
  - `bool` and `enum` → `u32` (an enum is its option's index)

  The fields are named exactly as the param `id`s.
- helpers: `sample_src(uv)`, `hash21`, `noise2`, `fbm2`, `luma`; also `sample_original(uv)`,
  `sample_depth(uv)` (depth is not filterable on every GPU, so never sample it with
  `src_sampler`) and `to_linear` / `to_display`. `src` and `original` hold display
  (sRGB-encoded) values; `color` params are linear.

Bindings, all in group 0: `src` 0, `src_sampler` 1, `original` 2, `depth` 3, `params` 4
(uniform); `paper` is immediate data. `paper.resolution` is the pass's own target size. The
entry points `mypaper_vs` / `mypaper_fx` are the prelude's, so a pass must not define them.

To validate exactly what the engine compiles, use `mypaper_theme::stack::pass_wgsl` (the
prelude, then the generated `struct Params`, then the pass; `first_line` maps errors back to
the pass file) or, with the crate's `check` feature, `stack::check::pass`, which runs naga and
reports the pass file's line. `stack::check::glsl` does the same for a Shadertoy base.

A multi-pass effect, such as a separable blur, lists several passes. Each pass reads the previous
pass as `src`. A pass may run at a lower `scale`; the engine upsamples its output.

### Shader params in Studio

Studio turns these comments in a shader base into sliders, the same way it does for effects:

```
// @param amount float 0..1 = 0.5 "Amount"
// @param tint color = #ffffff "Tint"
// @param streaks bool = true "Streaks"
// @param style enum drizzle|storm = drizzle "Style"
```

`mypaper_theme::stack::shader_params` parses them. The engine appends the generated
`struct Params` and a `const params = Params(...)` built from the base's `params` values, so the
shader reads `params.amount` with no binding; a change recompiles the shader.

### Shadertoy bases

A `glsl` base defines `void mainImage(out vec4 fragColor, in vec2 fragCoord)`. The engine wraps
it (`stack::glsl_fragment`) with a std140 block of `iResolution`, `iTime`, `iMouse`,
`iTimeDelta`, `iFrame` and `iFrameRate`; `fragCoord` has Shadertoy's bottom-left origin. Channel
inputs (`iChannel0..3`) are not provided yet.

### Errors

An effect that fails to load or compile is skipped and the rest of the stack keeps drawing. The
engine writes `%LOCALAPPDATA%\mypaper\stack-errors.json` on every load of a stack:
`{ "theme": "<folder name>", "errors": [{ "layer": "rain-1", "effect": "rain", "line": 12,
"message": "..." }] }` (`line` is in the pass file, or `null`; `layer` is `"base"` for the base).

## Cost rules

- **A still stack costs nothing at rest.** A stack is still when no layer is animated, the base is
  not video, shader or scene, and no param is time-driven. The engine renders it once per screen
  size and keeps it as the frame, exactly like an `image` theme.
- **A moving stack is split.** The static prefix of the layers (everything before the first
  animated layer) is baked once. Only the rest runs at `fps`, at `scale`.
- Studio shows the sum of the active layers' `cost`, scaled by resolution, fps and scale, as an
  estimate such as "about 2% GPU at 30 fps".
- The engine's resting rules (pause while covered, refresh once when changed) apply unchanged.

## Studio's Create mode

Create mode is a page under Wallpapers.

- **Canvas:** the big live preview, showing the real engine output on a stage.
- **Side panel:**
  - the base picker: import an image or video, a color or gradient, a shader (built-in or
    written), or a 3D scene;
  - the layer list: add from an effect gallery grouped by category, reorder, toggle, duplicate
    and delete;
  - the selected layer's params, generated from its schema.
- **Shader editor:** WGSL or GLSL text with live reload. Errors from naga are shown on their
  lines, and `@param` comments become sliders.
- **Saving** writes a theme folder under `%APPDATA%\mypaper\themes\<slug>/`, copying in the
  media, any custom effects, a `preview.jpg` and a credits entry for imported shaders.
