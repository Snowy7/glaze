# Widgets and plugins: the contract

This document is the spec for mypaper's widget system. It's the source of truth for the host
(`crates/widgets`), the shell page (`widgets/shell`), the SDK (`packages/widget-sdk`), and every
widget and plugin. Change it deliberately, and in the same commit as the code.

## Goals

- **Interactive desktop widgets** that sit on the desktop layer: above the wallpaper and icons,
  below every app window, and still there after Win+D.
- **Edit mode**: move, resize, snap to a grid, add and remove widgets, live.
- **Anyone can write a widget** (HTML/CSS/JS) or a **plugin** (any language). Both are folders with
  a manifest; no rebuild of mypaper needed.
- **Light**: one WebView2 per monitor, not one per widget. Everything sleeps whenever the wallpaper
  sleeps (fullscreen game, locked, display off, battery, user pause).
- **Safe**: widgets are sandboxed web content with declared permissions. Plugins are separate
  processes. No code is injected anywhere.

## Architecture

```text
mypaper-core ── supervises ──► mypaper-widgets (crates/widgets, Rust)
                                 ├─ per monitor: a desktop-layer window + one transparent WebView2
                                 │    └─ shell page  https://shell.mypaper/        (widgets/shell)
                                 │         └─ one sandboxed <iframe> per widget instance
                                 │              origin https://<widget-id>.widget.mypaper/
                                 ├─ bridge: shell ⇄ host (WebView2 postMessage), permission checks in Rust
                                 ├─ plugin manager: spawns plugins, JSON-RPC 2.0 over stdio
                                 └─ hold logic: pause, battery, lock, display off (global) and a window
                                    covering a monitor (that monitor only), see Pausing
```

- **Window placement.** Each monitor gets a window that's a child of the desktop host
  (`mypaper_desktop::DesktopHost::icons_parent`). In the layered (24H2+) layout it's z-ordered
  **above** `SHELLDLL_DefView`, so widgets draw over the icons. In the legacy layout it's a child of
  the DefView's parent, above DefView. It covers the monitor and is `WS_EX_NOACTIVATE` until a widget
  is clicked. The WebView's rasterization scale is set from the monitor's own DPI.
- **Which monitors.** A monitor gets a window and a WebView only while it has widget instances, or
  while edit mode is on (so widgets can be added to any monitor). With no instances and edit mode
  off, the host creates no WebView at all.
- **Explorer restarts.** A hidden top-level window receives `TaskbarCreated`; a WinEvent hook
  also watches the desktop host's destruction and replacement windows, even with every layer
  parked. Dead desktop layers and controllers are released immediately. After a 250 ms quiet
  period, the host resolves the new desktop once and recreates the required layers in process,
  keeping layout, settings, storage and edit mode. Further shell creation events wake recovery
  if the desktop was not ready; there is no polling. Repeated broadcasts for the same desktop
  do not rebuild it. Recreated shells restore subscriptions and publish their glass plates again.
- **Click-through.** The window's region (`SetWindowRgn`) is the union of the widget rectangles,
  plus the whole monitor while edit mode is on. Clicks outside widgets go to the desktop (icons,
  right-click menu, rubber-band selection).
- **Transparency.** WebView2 `DefaultBackgroundColor` is fully transparent, and the shell page's
  background is transparent. Chromium paints an **opaque** backdrop behind an iframe whose
  `color-scheme` differs from the one its embedding page resolves for the `<iframe>` element, which
  shows as a dark (or white) rectangle in the corners outside a rounded card. So the shell pins
  `color-scheme: dark` on its root and on every widget iframe, and **every widget document must pin
  `:root { color-scheme: dark }` too**, whatever its text tone (the widget kit's `base.css` does; the
  tone is `data-text`, not the color scheme). A widget on the default scheme gets an opaque white
  backdrop. Keep `html` and `body` transparent and paint only your own card.
- **Isolation.** Each widget has its own origin, `https://<id>.widget.mypaper/`, served from its
  folder by the host's network filter (see [Network](#network)), with a Content-Security-Policy on
  every response. (Not with `SetVirtualHostNameToFolderMapping`: mapped hosts bypass
  `WebResourceRequested`, so the host could neither filter them nor attach a policy.) Iframes use
  `sandbox="allow-scripts allow-same-origin"` (same-origin means *their own* origin). Widgets
  can't read each other or the shell. The shell, which is trusted, is mapped from
  `shell.mypaper` → `widgets/shell/dist` (`DENY`). Popups, top-level navigation away from the shell,
  script dialogs and every permission prompt (camera, location, notifications, clipboard read, …)
  are refused.
- **Pausing is per monitor.** What pauses a layer:
  - **A window covering that monitor** (a maximized or fullscreen app or game): that monitor's
    layer is hidden (`put_IsVisible(false)`, `TrySuspend`) and nothing else changes: widgets on
    every other monitor keep showing and animating. (A fullscreen app on monitor 1 no longer touches
    monitor 2. `pause_on_fullscreen` in `config.json` governs the wallpaper, not widgets.) Edit mode
    shows a covered monitor anyway.
  - **A global hold with the desktop still on screen**: the user pauses from the tray or Studio, or
    the machine is on battery with `pause_on_battery`. Layers *freeze*: they stay up showing their
    last frame, widgets get `visibility: false` (so they stop animating), `system` and `media`
    subscriptions, plugins and the stats timer stop. Layers are not hidden or released.
  - **Nothing on screen** (session locked, display off): every layer is hidden and suspended.

  A hidden layer is *parked* after 30 s: its window and WebView are released (a suspended WebView
  still holds hundreds of MB) and rebuilt, about a second, when its monitor shows again. Only that
  monitor's layer is released; when every monitor with widgets is parked, WebView2 itself is
  released. On release, `Resume` and `put_IsVisible(true)`. Widgets see `visibility` events (below).

## Folders

| What | Where |
|---|---|
| Bundled widgets | `widgets/<id>/` beside the exe or any parent (the same search as themes) |
| User widgets | `%APPDATA%\mypaper\widgets\<id>\` (wins over a bundled widget with the same id) |
| Bundled plugins | `plugins/<id>/` beside the exe or any parent |
| User plugins | `%APPDATA%\mypaper\plugins\<id>\` (wins over a bundled plugin with the same id) |
| Shell page | `widgets/shell/dist/` beside the exe or any parent |
| Layout | `%APPDATA%\mypaper\widgets.json` |
| Widget storage | `%APPDATA%\mypaper\widget-data\<instance-id>.json` |
| Plugin data (`dataDir`) | `%APPDATA%\mypaper\plugin-data\<plugin-id>\` |
| WebView2 profile | `%LOCALAPPDATA%\mypaper\webview2-widgets\` |
| Wallpaper palette (optional) | `%LOCALAPPDATA%\mypaper\palette.json`: `{ "dominant": "#rrggbb", "accent": "#rrggbb" }` |

## Widget manifest: `widget.json`

```json
{
  "id": "clock",
  "name": "Clock",
  "version": "1.0.0",
  "author": "mypaper",
  "description": "Big, calm clock with date.",
  "entry": "index.html",
  "size": { "default": [2, 1], "min": [1, 1], "max": [4, 2] },
  "resizable": true,
  "preset": "desktop",
  "instances": "multiple",
  "permissions": ["system.stats"],
  "plugins": [],
  "settings": [
    { "key": "format24", "type": "boolean", "label": "24-hour", "default": true, "group": "Time" },
    { "key": "accent", "type": "color", "label": "Accent", "default": "#ffffff" },
    { "key": "city", "type": "string", "label": "City", "default": "" },
    { "key": "density", "type": "select", "label": "Density", "options": ["cozy", "compact"], "default": "cozy" },
    { "key": "opacity", "type": "number", "label": "Opacity", "min": 0, "max": 1, "step": 0.05, "default": 1 }
  ]
}
```

- `id`: lowercase `[a-z0-9-]`, unique; it's the folder name too.
- Required: `id`, `name`, `size.default`. Optional: `version`, `author`, `description`, `entry`
  (default `index.html`, a relative path inside the folder), `size.min` (default `[1, 1]`),
  `size.max` (default: unbounded), `resizable` (default `true`), `preset` (default `desktop`),
  `instances` (default `multiple`), `permissions`, `plugins`, `settings`. Unknown fields and
  unknown permissions are errors, so typos fail loudly.
- `preset` is where the widget lives (see [Placement presets](#placement-presets)): `desktop`
  (today's behavior), `overlay` (always on top, click-through except the widget) or `popup` (opened
  from a bar module; the manifest field and SDK only, the bars host implements it). Any other value
  is an error.
- `instances` is how many the user may add: `single` (one in total, on any monitor), `multiple`
  (the default, no limit) or `per-monitor` (at most one per monitor). The edit-mode palette greys
  out **Add** and the ⋯ menu's **Duplicate** when the limit is reached, and the host refuses the
  `add`/`duplicate` anyway (a layout file that already holds more is shown as it is: the rule only
  stops *adding*). Any other value is an error.
- `size` is in **grid cells**, `[columns, rows]`. `min <= default <= max`. `"resizable": false`
  locks the size to `default`.
- `permissions` (anything not listed is denied):
  - `system.stats`: CPU, memory, GPU, network and disk usage.
  - `media`: now-playing info and transport controls (Windows SMTC).
  - `launch`: open files, folders, URLs and apps (`mypaper.launch`).
  - `network:<origin>`: `fetch` to that origin (for example `network:https://api.open-meteo.com`),
    enforced by the host's web resource filter (see [Network](#network)). No other network access.
    An origin is `scheme://host[:port]` with no path; schemes are `http`, `https`, `ws`, `wss`.
  - `theme`: read the current wallpaper palette and theme name.
  - `audio`: the audio spectrum of whatever the default output device plays (`mypaper.audio`,
    see [Audio spectrum](#audio-spectrum)). Capturing is the host's loopback of the speakers; nothing
    is recorded or stored, and no microphone is touched.
  - `user`: the signed-in user's display name (`mypaper.user.info`).
  - `storage.shared:<namespace>`: read and write a storage namespace that every widget declaring the
    same permission shares (`namespace` is `[a-z0-9-]+`, for example `storage.shared:activity` for the
    todo list, the timer and the contribution heatmap). Values live in
    `%APPDATA%\mypaper\widget-data\_shared\<namespace>.json` (1 MB per namespace). There is no
    locking: give each writer its own keys.
  - `clipboard.write`: the shell adds `allow="clipboard-write"` to the widget's iframe, so
    `navigator.clipboard.writeText` works. There's no wire method for it.
- `plugins`: plugin ids whose methods this widget may call (see Plugins).
- `settings`: rendered by the shell in edit mode, and by Studio. Values go to the widget at load
  and on every change. Types: `boolean`, `string`, `color`, `select` (needs `options`), `number`
  (optional `min`, `max`, `step`), and `list`:

  ```json
  { "key": "shortcuts", "type": "list", "label": "Shortcuts", "maxItems": 24,
    "fields": [ { "key": "label", "type": "string", "label": "Label" },
                { "key": "target", "type": "string", "label": "Path or URL" } ],
    "default": [ { "label": "Files", "target": "explorer.exe" } ] }
  ```

  A `list` is an editable list of records: the shell renders one input per field for each item
  (with add and remove), and the widget receives an array of `{ <field key>: value }` objects.
  `fields` take the scalar types above, and their `default` is optional.

  **More types** (the property schema; every one is also valid as a `list` field except `file`
  and `image`):

  | type | extra fields | value the widget receives |
  |---|---|---|
  | `slider` | `min`, `max` (both required), `step`, `unit` (a suffix such as `"px"` or `"%"`) | a number. Like `number`, but always a slider with a readout; while it is dragged the shell previews the value in the widget at once (a `settings` event), and saves once on release |
  | `color` | `alpha: true` | `#rrggbb`, or `#rrggbbaa` when `alpha` is set (a plain `#rrggbb` is also accepted there). The form shows an opacity slider with the swatch, and with `alpha` it always saves `#rrggbbaa` (8 digits, even when opaque) |
  | `file`, `image` | `extensions`: the allowed extensions without dots (`["jpg", "png"]`; `image` defaults to the common image formats) | a string: `""`, or the served URL of the picked file, which the host copied into the widget's data folder (see [Picking files](#picking-files)). `image` shows a thumbnail and **Clear** |
  | `font` | | a CSS font-family name (for example `"Cascadia Code"`; at most 200 characters, none of `; { } < > \`). The form offers common fonts and a text box. The widget uses it as `font-family: var(--w-font, ...)`-style CSS, quoting the name |
  | `select` | `options` may hold **objects** `{ "value", "label"?, "icon"? }` as well as plain strings (`icon` is a short glyph, an emoji or a symbol, shown beside or instead of the label; a select of 5 or fewer options with icons renders as an icon bar) | the option's `value` |

  **Places.** `{ "key": "city", "type": "place", "label": "Location", "search": { "plugin":
  "weather", "method": "search" }, "default": null }`. The value is `null` (automatic: where this PC
  is) or `{ name, lat?, lon?, region?, country? }` (a picked town; without coordinates the widget looks
  the name up, and a plain string an older text setting saved reads as `{ name }`). The form offers
  **Automatic** and a search as you type: about 300 ms after two characters it calls the widget's
  own `plugin` `method` with `{ query }` (through the host, with the widget's permissions; the
  plugin must be in `plugins`) and lists the answers, `{ name, admin1, admin2, country,
  country_code, lat, lon, population }` with nulls allowed; only a picked candidate is saved (as
  `{ name, lat, lon, region: admin1, country }`), never the typed text. Not valid as a `list` field.

  **Groups.** Any setting (and a `list`) may carry `"group": "Layout"`. The form shows a section
  heading whenever the group changes from one setting to the next (settings without a group are
  moved to the top, ungrouped, in their order), so keep a group's settings together: a group that
  comes back later is a new section. Groups are presentation only: the
  settings stay a flat list of keys. At most 64 characters.

  A saved value that doesn't fit its setting (wrong type, a number outside `min`/`max`, a select
  value not in `options`, a color that isn't `#rrggbb` (or `#rrggbbaa` with `alpha`), a file that
  isn't `""` or one of this widget's data URLs, a list longer than `maxItems` or with an invalid
  field) is replaced by the default. List items get missing fields filled from the field's default
  (or `false`, `0`, `""`, the first option, `#ffffff`) and unknown fields dropped.

  **Manifest checks** (an error when the manifest loads): a `slider` needs `min < max` and a
  positive `step`; `extensions` is a non-empty list of 1 to 16 letters and digits each (no dot);
  the `default` of a `file` or `image` must be `""`; every `default` must itself be a valid value;
  a `group` is at most 64 characters; an unknown field is an error. The `select` options keep the
  shape they were written in when the host sends the manifest to the shell.
- `$schema`: optional, ignored by the host. Point it at
  `packages/widget-sdk/schema/widget.schema.json` for editor validation and completion
  (`plugin.schema.json` for plugins).

## Layout: `widgets.json`

```json
{
  "version": 1,
  "grid": { "cell": 0, "gap": 8, "margin": 16 },
  "instances": [
    {
      "instance": "clock-1",
      "widget": "clock",
      "monitor": "\\\\.\\DISPLAY1",
      "cell": [0, 0],
      "size": [2, 1],
      "settings": { "format24": true },
      "appearance": { "text": "dark" }
    }
  ],
  "overlay": true,
  "appearance": { "surface": "glass", "opacity": 0.5 }
}
```

- `overlay` (top level, default `true`) says whether the topmost overlay layer is shown (see
  [Placement presets](#placement-presets)); the hotkey, the tray and `--overlay` flip it, and the host
  writes it.
- `appearance` (top level: the user's look for every widget; per instance: that widget's own) holds
  only the fields it sets, with the names and values of [Appearance](#appearance). Both are
  optional and validated when the file loads (ranges, `#rrggbb`, unknown fields are errors, so the
  file never silently loses a setting). The host writes only the fields a layer sets, and drops an
  empty layer.

- **The grid.**
  - `cell` is the **nominal** cell size: as many whole cells as fit are laid out, then every
    cell is stretched so the grid spans the full area edge to edge (no leftover strip).
  - `cell: 0` means "match the desktop icon grid": horizontal and vertical icon spacing from
    `HKCU\Control Panel\Desktop\WindowMetrics` `IconSpacing`/`IconVerticalSpacing`. Negative values
    are twips (`px = -value / 1440 × DPI`, so `-value / 15` DIPs); positive values are DIPs. Missing
    values fall back to 96 DIPs. Cells may therefore be rectangular.
  - A positive `cell` is a fixed size in DIPs.
  - `gap` and `margin` are in DIPs. The grid starts at the monitor's **work area** (excluding the
    taskbar and any app bar, such as mypaper's own top bar and dock) plus `margin`. A cell `[c, r]`
    sits at `origin + [c, r] × (cell + gap)`, and a widget `[w, h]` cells wide is
    `w × cell + (w − 1) × gap`. The grid has as many whole cells as fit in the work area minus the
    margins.
  - **The work area changes** (an app bar appears or goes, a display is added, resized or changes
    scale, the icon spacing changes): the host hears it through `WM_SETTINGCHANGE`
    (`SPI_SETWORKAREA`, icon metrics) and `WM_DISPLAYCHANGE`, a few hundred milliseconds after the
    last one, and re-lays out: the grid is recomputed (the stretch included) and every widget is
    placed on it again, clamped, so none sits under a bar. Saved cells are not rewritten; they
    clamp again if the bar goes.
- `instance` ids are `[a-z0-9-]`, unique; the host names new ones `<widget>-<n>`.
- `monitor` is the device name (`MONITORINFOEXW.szDevice`). If that monitor is missing, the
  instance is shown on the primary monitor at its cell, clamped, and its saved monitor isn't
  changed.
- On display, sizes are clamped to the manifest (`min`/`max`, or `default` when not resizable) and
  cells to the grid. Instances of widgets that aren't installed are kept in the file but not shown.
- The host writes the file atomically (temp file plus rename) and watches it, so Studio or a text
  editor can edit it live. If the file doesn't parse, the host keeps showing the last good layout,
  refuses edits (so it never overwrites your file), and the shell shows the error in edit mode.

## Appearance

How a widget *looks* (its surface, text, accent, font and size) is a separate, layered setting,
so users and themes can restyle every widget without editing widget code.

**Fields** are defined by `mypaper_theme::Appearance` (`crates/theme`). Each one is optional:

| Field | Values | Default |
|---|---|---|
| `surface` | `none` (no background: text sits on the wallpaper), `glass` (a translucent tint, blurred where the page can see what is behind it), `solid` | **`glass`** |
| `tint` | `"#rrggbb"`: the surface color | `#343a42`, a calm slate (`#f2f2f2` with dark text, see below) |
| `opacity` | 0–1: opacity of a `glass` surface (a `solid` surface is always opaque) | 0.46 |
| `blur` | 0–64 px: background blur (glass only) | 24 |
| `radius` | 0–48 px: corner radius | 22 |
| `border` | bool: hairline border | true |
| `text` | `light` (for dark wallpapers) or `dark` | `light` |
| `accent` | `"#rrggbb"` or `auto` (the wallpaper palette's accent) | `#ffffff` (`#0a0a0c` with dark text, see below) |
| `font` | a CSS font-family list | Segoe UI Variable |
| `scale` | 0.5–2: content scale | 1 |
| `shadow` | bool: soft text shadow, for legibility with `surface: none` | true |
| `style` | `minimal`, `depth`, `neon`, `paper`, `aero` or `liquid`: the design language, see [Styles](#styles) | `minimal` |

**Tone-aware defaults.** A near-black tint is dark on dark text, and a white accent vanishes on a
light surface, so with `text: dark` the built-in defaults flip: a `tint` that is still `#343a42`
becomes `#f2f2f2`, and an `accent` that is still `#ffffff` becomes the ink, `#0a0a0c`. **The host
does this, once, in the merge**, and sends the final values (any color a theme or the user chose
stays; picking exactly the default color counts as the default). So the SDK has nothing to flip: it
applies what it gets. The shell's live preview applies the same rule.

**Layers**, lowest to highest (each set field wins over the layers below it):

1. The built-in defaults above.
2. The active theme: `theme.json` → `"widgets": { "appearance": { … }, "css": "widgets.css" }`.
3. The user's global choice: `widgets.json` → `"appearance": { … }`.
4. The instance: `widgets.json` → `instances[i].appearance`.

**Resolution is done by the host.** It sends the merged result as plain JSON values: `accent: auto`
is resolved to a color from `palette.json`, falling back to `#ffffff`. It also sends the theme CSS as
text (read from the theme's `widgets.css` path, 512 KB at most). Both arrive in the `hello` result
as `appearance` (an object with every field set) and `themeCss` (string or null). Changes arrive as
the pushed event `appearance` with `{ appearance, themeCss }`, whenever any layer, the theme or the
palette changes: an edit of `widgets.json`, a theme switch in `config.json`, an edit of the active
theme's `theme.json` or `.css` files, or a new `palette.json`. (On the wire between host and shell
the same data travels in `state`, see [Host ⇄ shell](#host--shell); the shell relays it.)

**What the SDK does with it.** `packages/widget-sdk`'s kit applies the appearance on `<html>`:

| Attribute or variable | Set from |
|---|---|
| `data-surface="none\|glass\|solid"` | `surface` |
| `data-text="light\|dark"` | `text` |
| `data-shadow="on\|off"` | `shadow` |
| `data-style="minimal\|depth\|neon\|paper\|aero\|liquid"` | `style` |
| `data-plate="on\|off"` | whether the shell has drawn this widget's wallpaper lens (the state's `plate`) |
| `--surface` | `tint` at `opacity` (always opaque for `solid`) |
| `--blur`, `--radius` | `blur`, `radius` (px) |
| `--line` | the border color, or **transparent if `border` is false** |
| `--t0`…`--t3` | text steps from `text` |
| `--fill-1`…`--fill-3` | fills from `text` |
| `--ink`, `--halo`, `--on-accent` | the ink (as a channel triple), its opposite, the ink that reads on the accent |
| `--accent`, `--font`, `--scale` | `accent`, `font`, `scale` |

It also inserts `themeCss` as the last `<style>`. Widgets should use these variables and never
hard-code a background, so a theme can restyle them. The widget's own CSS still works, and theme CSS
can target `[data-widget="<id>"]` (set by the kit on `<html>`).

Rules for widget authors that follow from this:

- **`surface: solid` is always opaque**; `opacity` and `blur` only apply to `glass`.
- **Dividers and hairlines inside a widget use `--fill-2`, not `--line`**: `--line` disappears when
  the user turns the border off.
- **Measure content with a `ResizeObserver`, not `size.onChange`.** `size` reports the widget's box
  in CSS px, which does not follow `scale` (the kit applies `--scale` inside the page) and can be
  re-sent after an appearance change. Use the size for the grid, and observe your own elements for
  anything you lay out by pixels.

### Styles

`style` is the widget's design language. The host only carries the name; the kit
(`packages/widget-sdk/widget-kit/base.css`) turns it into light and material. `minimal` is the
default: a calm desktop widget after macOS's (one flat translucent fill and a hairline, no
gradients, gloss or shadows, generous padding, text in a few neutral steps, one accent used
sparingly). `liquid` is as calm, with Apple-style Liquid Glass drawn by the wallpaper (see
[Liquid glass](#liquid-glass)). The others add light to the *surface* and to the
kit's building blocks (`.card`, `.panel`, `.chip`, `.track`, `button`, `.field`, `.glow`), so a
widget that sticks to the kit looks right in all five without knowing about them:

| `data-style` | Is | Surface it likes |
|---|---|---|
| `minimal` | The default: flat, quiet, monochrome. | any (glass by default) |
| `depth` | The calm tile raised a little: one soft shadow, a faint top edge, a one-color highlight; no gradients on fills or text. | glass or solid |
| `neon` | A dark card with a glowing gradient outline, glow on accent elements and bright tracks. | solid or glass |
| `paper` | Soft matte: warm low-contrast fills, a grain, shallow inset shadows, no gloss. | solid |
| `aero` | Glossy glass: a specular sheen across the top, a bright rim, saturated blur. | glass |
| `liquid` | Calm like minimal; a blurred wallpaper plate with refraction at the rim and a thin specular edge. Without a snapshot, the visible tinted tile and rim remain. | glass |

Every style also works with `surface: none`: the building blocks then keep their own light
(a glow, a rim, a sheen) without painting a card.

The kit exposes NEW optional CSS variables that a style sets and a widget or theme can use or
override (all have defaults, so never required): `--elev-1`, `--elev-2`, `--elev-3` (box-shadow
stacks for resting, raised and floating things), `--bevel` (an inset highlight/shade pair),
`--sheen` (the specular gradient image), `--rim` (the edge gradient image), `--glow` (a color, the
accent's glow, `transparent` when the style has none), `--glow-size` (px), `--grain` (a noise
image, `none` for styles without), `--spark`/`--accent-2` and `--hi`/`--hi-2` (the highlight pair
glows and gradients are made of: the accent and its neighbour in hue when the accent has a color,
else the style's signature pair, deepened for dark text; `--hi` is the accent itself in `minimal`;
`--accent-rgb`, `--spark-rgb`, `--accent-2-rgb` are channel triples for alphas) and `--flow` (the
drifting accent gradient: the `.flow` class and `.track > i` animate it, at about 10 frames a
second, only while the widget is visible, and build it from `--hi`/`--hi-2` on the element, so a
widget may retint a bar locally by setting those two). `data-style` is also on `<html>` for widget CSS:
`html[data-style="neon"] .time { … }`.

### Liquid glass

With `style: liquid` and `surface: glass`, the shell draws a blurred, refracting wallpaper
plate beneath the widget's transparent iframe. Its quiet tint, hairline and radius come from the
resolved appearance (defaults: `#343a42`, 0.46 and 22). One light specular rim makes the edge read
on a dark wallpaper too.

- **Geometry.** The SDK's `plate` report measures the card; the shell clips it to the slot and
  follows its position during dragging. The page paints content only once the shell has decoded
  its backdrop and drawn a plate. A missing or failed image keeps the normal glass tile and CSS
  rim visible. Overlay widgets and previews use that same visible fallback.
- **Pixels.** The engine publishes each monitor's wallpaper to
  `%LOCALAPPDATA%\mypaper\backdrop\DISPLAYn.bmp`. Shader, image and video sources copy their
  frame; web wallpapers use WebView2 `CapturePreview`, which excludes widgets by construction.
  Live sources refresh every ten seconds while visible and requested. The host watches atomic
  replacements and sends each monitor's shell its own URL and DIP dimensions; feedback is driven
  by file events. The shell applies blur and the same lens geometry as the bars, aligned to the
  monitor, then a tint and specular rim. No capture includes widget content.
- **Knobs.** `widgets.glass` supplies `intensity`, `refraction`, `highlight`, `tint` (0-1)
  and extra `blur` (DIPs, 0-32), layered over 1, 0.6, 0.55, 0.06 and 0. The appearance's blur
  and opacity supply the base material.
- **Why a snapshot.** The desktop widget HWND is a child of Explorer. Windows rejects its DWM
  host backdrop attribute (`0x80070006` on the tested desktop), so it cannot use the top-level
  bars' live DWM compositor. A snapshot gives all wallpaper kinds the same visible material.
  The older engine plate pass remains available for other writers; widgets retire its old file.

**The CSP** allows `style-src 'self' 'unsafe-inline'`, so the kit can inject theme CSS.

**Editing.** In edit mode, the toolbar's **Appearance** button edits the user layer (all widgets).
Each widget's ⋯ menu has **Appearance** for the instance layer. Controls: style (segmented:
minimal / depth / neon / paper / aero / liquid), surface (segmented: none / glass / solid), tint (color), opacity, blur and radius (sliders), border and text shadow
(toggles), text (light / dark), accent (Auto, or a color), font (text field with presets) and scale
(slider). A field the edited layer sets shows **Use default**, which clears it so the layer below
shows through (an unset field shows that inherited value). Sliders and color pickers preview live on
the affected widgets while they move (nothing is written) and save once on release; the other
controls save at once. A user-layer change reaches every widget that doesn't set the field itself;
an instance change reaches only that widget.

## Edit mode

- **Toggle**:
  - from the tray (**Edit widgets**);
  - from Studio;
  - with the hotkey **Ctrl+Alt+Shift+W**;
  - by posting the registered window message `mypaper.widgets.edit` (`RegisterWindowMessageW`)
    with `wParam` 1 (on), 0 (off) or 2 (toggle) to the host's message-only window, whose class is
    `mypaper.widgets.host`. `mypaper-widgets --edit on|off|toggle` does exactly that and exits.
- **What it shows**: the grid (subtle dots or lines on the work area), each widget with a frame,
  a drag area, resize handles (edges and corners) and a ⋯ menu (Settings, Appearance, Duplicate,
  Remove), plus a toolbar with **+ Add widget** (a palette listing every installed widget),
  **Appearance** (the user layer, see [Appearance](#appearance)) and **Done**. The toolbar sits
  centred at the top of the work area, or at the bottom when a widget is under the top spot and the
  bottom is clear (it is always drawn above widgets). The window region covers the whole monitor
  while editing, on every monitor.
- **Moving and resizing**: live, with the pointer. Positions snap to cells. Sizes are clamped to the
  manifest's `min`/`max`. Widgets can't overlap: a move that would overlap swaps or shifts the
  other widgets (they move down, then right; never off-grid), or the move is refused with a shake
  animation. The exact rule is documented in the shell (`widgets/shell/src/grid.ts`). Widgets stay
  on their monitor; to move one to another monitor, remove it and add it there.
- **Saving**: every committed change is written to `widgets.json` at once. **Esc** or **Done**
  leaves edit mode.
- **Keyboard**: arrows move the selected widget one cell, and Shift+arrows resize it (right and
  down grow, left and up shrink). Esc closes an open menu first. The shell needs keyboard focus for
  this, which it gets when you click it (the host never takes focus by itself).
- Edit mode never takes effect while the host is held (paused by the user, on battery with
  `pause_on_battery`, locked, display off): requests to turn it on are ignored, and becoming held
  leaves edit mode. A fullscreen app on one monitor doesn't block editing the others.

## Widget JS API (`mypaper`)

Widgets bring the SDK themselves: the `mypaper.js` drop-in (`<script src="mypaper.js">`, which
defines the global `mypaper`) or the typed package (`packages/widget-sdk`). It speaks the wire
protocol below with the shell. Every call is a promise. A call to a capability the manifest didn't
declare rejects with `PermissionDenied`.

```ts
interface Mypaper {
  readonly ready: Promise<void>;    // resolves once the shell answers the handshake; every call waits for it
  readonly instance: string;        // "clock-1" (after ready)
  readonly widget: WidgetManifest;  // the parsed manifest (after ready)
  readonly visible: boolean;        // false while paused, or while a window covers this monitor
  readonly editing: boolean;        // edit mode is on
  settings: {
    get(): Promise<Record<string, unknown>>;
    onChange(cb: (s: Record<string, unknown>) => void): () => void;
    // Saves some of the widget's own settings, checked against its manifest like a form edit
    // (no permission: they are its own). onChange follows with the new values.
    set(values: Record<string, unknown>): Promise<void>;
  };
  size: {
    // px is in CSS pixels (what innerWidth/innerHeight report); dpi is the monitor's (96 = 100%).
    get(): Promise<{ cells: [number, number]; px: [number, number]; dpi: number }>;
    onChange(cb: (s: { cells: [number, number]; px: [number, number]; dpi: number }) => void): () => void;
  };
  on(event: "visibility", cb: (visible: boolean) => void): () => void;  // pause your timers when false
  on(event: "editmode", cb: (editing: boolean) => void): () => void;
  storage: {                           // per instance, persisted
    get<T>(key: string): Promise<T | undefined>;
    set(key: string, value: unknown): Promise<void>;
    // "storage.shared:<ns>": the same two calls on a namespace shared between widgets
    shared(namespace: string): { get<T>(key: string): Promise<T | undefined>; set(key: string, value: unknown): Promise<void> };
  };
  system: {                            // "system.stats"
    stats(): Promise<{ cpu: number; memory: { used: number; total: number };
                       gpu?: number; net?: { up: number; down: number }; disks?: { name: string; used: number; total: number }[];
                       battery?: { percent: number; charging: boolean };  // absent on desktops
                       uptime: number;                                      // seconds since boot
                       network?: { kind: "wifi" | "ethernet" | "none"; name?: string; signal?: number } }>;
    subscribe(intervalMs: number, cb: (s: SystemStats) => void): () => void; // min 1000 ms; paused while hidden
  };
  media: {                             // "media"
    current(): Promise<{ title: string; artist: string; album?: string; app: string;
                         playing: boolean; position?: number; duration?: number; artwork?: string /* data URL */ } | null>;
    onChange(cb: (m: MediaState | null) => void): () => void;
    control(action: "play" | "pause" | "toggle" | "next" | "previous"): Promise<void>;
  };
  launch(target: string): Promise<void>; // "launch": a path or URL, via ShellExecute (no admin)
  theme: { current(): Promise<{ name: string; palette?: { dominant: string; accent: string } }>;
           onChange(cb: (t: ThemeInfo) => void): () => void };             // "theme"
  plugin<T = unknown>(id: string, method: string, params?: unknown): Promise<T>; // declared in "plugins"
  onPlugin(id: string, event: string, cb: (data: unknown) => void): () => void;
  audio: {                             // "audio"
    // 64 to 128 floats in 0..1, log-spaced low to high, ~30 per second, only while visible.
    spectrum(cb: (bins: number[]) => void, opts?: { bins?: number }): () => void;
  };
  user: { info(): Promise<{ name: string; account: string }> };   // "user"
  pickFile(opts?: { extensions?: string[]; title?: string }): Promise<{ url: string; name: string } | null>;
  appIcon(target: string, size?: number): Promise<string | null>; // "launch": a PNG data URL
}
```

- **Units**: `cpu` and `gpu` are percent (0–100) of the whole machine; `memory` and `disks` are
  bytes; `net` is bytes per second, summed over physical interfaces; `disks` lists fixed drives
  (`name` is `"C:"`); `gpu` is the busiest GPU engine and is omitted when unavailable. Media
  `position` and `duration` are seconds. `position` is sampled: change events fire on seeks and
  track changes, not every second, so advance it locally while `playing`. `artwork` is a
  `data:image/…;base64,` URL (omitted above 256 KB). `app` is a readable app name (`Spotify`, not
  `Spotify.exe`). Colors are `#rrggbb`.
- **Battery, uptime, network**: `battery.percent` is 0–100; `battery` is absent on machines without
  a battery (or when Windows can't tell). `uptime` is seconds since boot. `network.kind` is
  `ethernet` when a wired adapter has a link (Windows routes over it first), else `wifi` with the
  network's name (`name`, the SSID) and `signal` (0–100), else `none`. All of it is read only while
  something subscribes or asks: one sample for every subscriber, the Wi-Fi service queried at most
  every 5 s, nothing while the host is held.
- **Errors**: `PermissionDenied`, `NotFound`, `InvalidParams`, `PluginUnavailable`, `Internal`.
- **Visibility**: when the host is paused (by the user, on battery) or a maximized or fullscreen
  window covers the widget's monitor, the widget gets `visibility:false`. Widgets should stop their
  animations. Subscriptions are paused automatically. A covered monitor's WebView is then
  suspended; a paused one stays on screen showing the last frame. A game on another monitor changes
  nothing (see [Pausing](#architecture)).

### Wire protocol (iframe ⇄ shell)

The SDK (`packages/widget-sdk`, also the `mypaper.js` drop-in) speaks this; the shell implements
the other side. All messages carry `"mypaper": 1`: requests are `{ mypaper: 1, id, method, params }`,
answers `{ mypaper: 1, id, result }` or `{ mypaper: 1, id, error: { code, message } }`, and events
`{ mypaper: 1, event, data }`. The shell only accepts a message whose `event.source` is the
instance's iframe and whose `event.origin` is that widget's origin, and forwards it to the host with
the **instance id attached by the shell**, never by the widget. The host enforces permissions from
the manifest on disk, never from the message.

**Handshake.** On load the widget sends `hello` (`params: { sdk: 1 }`). The shell answers with
everything a widget needs synchronously afterwards:
`{ instance, widget /* the manifest */, settings, size, visible, editing, appearance, themeCss, plate }`
(see [Appearance](#appearance)). `mypaper.ready`
resolves with that, and every other call waits for it. (A widget that gets no answer within
10 s fails `ready`: it is not inside mypaper.)

**Requests** (widget → shell, answered with `result` or `error`). The shell checks the permission
from the manifest *before* forwarding to the host (the host checks again, from disk):

| method | params | result | needs |
|---|---|---|---|
| `hello` | `{ sdk }` | the handshake state above | |
| `storage.get` | `{ key, shared? }` | the value, or `null` when missing | `storage.shared:<shared>` when `shared` is set |
| `storage.set` | `{ key, value, shared? }` | `null` (a missing `value`, which is what `undefined` becomes, deletes the key; 1 MB per instance, or per shared namespace) | `storage.shared:<shared>` when `shared` is set |
| `system.stats` | | `SystemStats` | `system.stats` |
| `media.current` | | `MediaState` or `null` | `media` |
| `media.control` | `{ action }` | `null` | `media` |
| `launch` | `{ target }` | `null` | `launch` |
| `theme.current` | | `ThemeInfo` | `theme` |
| `plugin` | `{ plugin, method, params? }` | the plugin's result | `plugin` listed in `plugins` |
| `user.info` | | `{ name, account }` | `user` |
| `settings.set` | `{ values }` | `null` (`InvalidParams` when a key isn't declared or a value doesn't fit) | |
| `pickFile` | `{ extensions?, title? }` | `{ url, name }`, or `null` when cancelled | |
| `appIcon` | `{ target, size? }` | a PNG data URL, or `null` | `launch` |
| `plate` | `{ rect, radius }` or `null` | `null`; answered by the shell, never forwarded (see [Liquid glass](#liquid-glass)) | |
| `subscribe` | see below | `null` | per topic |
| `unsubscribe` | `{ sub }` | `null` | |

**Subscriptions.** `subscribe` params are `{ sub, topic: "system", intervalMs }`,
`{ sub, topic: "media" }`, `{ sub, topic: "theme" }`, `{ sub, topic: "audio", bins? }` or
`{ sub, topic: "plugin", plugin, event }`, where `sub` is an id the widget makes up. Values arrive
as `{ mypaper: 1, event: "sub", data: { sub, value } }`:

- `system`: one sample right away, then one per `intervalMs` (never faster than 1000).
- `media`, `theme`: on change only (read the current value with `media.current` / `theme.current`).
- `audio`: `value` is an array of `bins` floats, see [Audio spectrum](#audio-spectrum).
- `plugin`: each matching plugin event's `data`.

While a widget is not visible (`visibility: false`) nothing is delivered to it; `system` sends a
sample as soon as it is visible again.

**Pushed events** (shell → widget, always on, no subscription): `visibility` (`boolean`),
`editmode` (`boolean`), `settings` (the full settings object), `size`
(`{ cells, px, dpi }`) and `appearance` (`{ appearance, themeCss, plate }`).

**Plugin errors.** A JSON-RPC `error` from a plugin becomes a wire error as follows: if
`error.data.code` is one of the five names above it is used; otherwise `-32601` is `NotFound`,
`-32602` is `InvalidParams` and anything else is `Internal`. A plugin that isn't running, can't
start or crashed mid-call is `PluginUnavailable`. The message is passed on.

## Audio spectrum

`mypaper.audio.spectrum(cb, { bins })` (permission `audio`) delivers what the default output device
is playing, as a spectrum, for visualizers.

- **Value**: a plain `number[]` of `bins` floats in `0..1` (default 64; 64 to 128, other values are
  clamped), index 0 the lowest frequency. Bins are log-spaced from about 40 Hz to 16 kHz, with
  level mapped from roughly -70..0 dB to 0..1, attack-fast and decay-slow smoothing applied by the
  host (a widget can draw the values as they come). Silence is all zeros (sent once, then nothing
  until sound returns, and a subscriber gets a frame every ~33 ms while audio plays).
- **Rate**: ~30 Hz (one frame every 33 ms), one shared FFT for every subscriber, whatever the
  count. Each subscriber gets its own `bins` resolution.
- **Wire**: `subscribe` with `{ sub, topic: "audio", bins? }`; values arrive as the usual `sub`
  event. Needs `audio` in `permissions`, else `PermissionDenied`.
- **Host cost model**: capture is WASAPI loopback of the default render endpoint
  (`IAudioClient` initialized with `AUDCLNT_STREAMFLAGS_LOOPBACK`) on a dedicated thread, started
  by the first *visible* subscriber and stopped as soon as there is none: a subscription of a
  widget that isn't visible (paused, covered, a hidden layer) doesn't count; a held host (user
  pause, battery, lock, display off) stops capture and resumes it with the subscribers; the thread
  and the audio client are released when capture stops, so the idle cost is zero. The default
  device changing (headphones plugged in; the thread looks once a second) restarts the capture on
  the new one at once, and a device that fails or goes away (`AUDCLNT_E_DEVICE_INVALIDATED`) is
  retried after 1 s, 2 s, 4 s, 8 s, then every 10 s, never faster.
- **Silence**: a silent device delivers no packets, so the host feeds the time that passes as
  silence; the bars decay (about 2 s from full) and snap to zero, one all-zero frame is sent, and the
  thread then polls every 100 ms (instead of every 33 ms) without computing or sending anything
  until sound returns. A subscriber that subscribes during silence gets nothing until sound plays.
- **Measured** (release build, the capture thread's own CPU time over 15 s): a tone playing, 30.2
  frames/s at 0.21% of one core; silence, 0.000% (one frame sent).
  End to end on the real host (debug build, the `visualizer` at 6x2 cells on a 2560x1440 monitor,
  a test tone playing): the host process 2.6% of one core in a *debug* build (the unoptimized FFT;
  release is the 0.21% above) and **WebView2 12 to 20% of one core** for the canvas at 30 fps (12.5%
  `minimal`, 14.8% `neon` with no surface, 19.8% `neon` on glass: the backdrop blur and the glow add
  to it). Silent, the whole thing is under 1% (the widget stops drawing and the capture thread
  idles). So the feed is cheap and the canvas isn't free: a visualizer is for listening sessions;
  keep it small, and prefer `surface: none` or `solid` over `glass` for it.
- **Math** (pure and tested in `crates/widgets/src/audio.rs`): a 2048-sample Hann-windowed radix-2
  FFT of the mono mix at the device's rate, magnitudes to dB, log-spaced bins (each the max of its
  FFT bins; low bins interpolate), then smoothing.
- **Mock harness**: the dev harness plays a fake spectrum (a few moving bumps over a tilted
  floor) while *Visible* is on.

## Picking files

Widgets can't browse the disk, so the host does it:

- `mypaper.pickFile({ extensions?, title? })` opens the system file dialog (filtered to the
  extensions, without dots, for example `["jpg", "png"]`) and resolves with `{ url, name }`, or
  `null` when the user cancels. The host **copies** the file into the instance's data folder,
  `%APPDATA%\mypaper\widget-data\<instance-id>\files\<name>` (a numeric suffix when the name is
  taken; 64 MB at most, else `InvalidParams`), and `url` is
  `https://<widget-id>.widget.mypaper/__data/<instance-id>/<name>`, which the host serves (and only to
  that widget's own origin, with the usual headers). The widget keeps the URL, usually in
  `storage` or as a `file`/`image` setting. It needs no permission: the dialog is the consent, and
  the widget gets nothing but the copy.
- **Names and serving.** The stored name is the file's name made safe as one Windows path component
  (no directory part, forbidden characters become `_`, reserved device names such as `nul` get a `_`
  prefix, at most 100 characters with the extension kept); a taken name gets `-2`, `-3`, …. The
  host serves only `/__data/<instance>/<name>` for an `<instance>` of that widget id and a name that
  is exactly such a stored name, so nothing can reach another widget's files or leave the folder.
  `__data` is therefore a reserved first path segment in a widget's own folder. `extensions` that are
  not letters and digits, more than 32 of them, or a title over 200 characters are `InvalidParams`;
  one dialog is open at a time (a second call is `Internal`).
- The `file` and `image` **settings** call the same thing from the settings form: the shell sends
  the host `{ type: "pickFile", instance, key, extensions }`; the host shows the dialog, copies the
  file and stores the URL as that setting's value (a `state` follows). The dialog's filter comes
  from the manifest (the setting's `extensions`, or the image formats), never from the message.
  Removing the instance (in edit mode, or by deleting it from `widgets.json`) removes its data
  folder; duplicating an instance copies its files and points the copy's file settings at them;
  **Clear** only resets the setting.
- `mypaper.appIcon(target, size = 64)` (permission `launch`) returns the app's icon as a PNG data
  URL: `target` is an `.exe`, `.lnk` or any file path, a folder, or an AUMID written as
  `shell:AppsFolder\<AUMID>` (what `launch` also opens). `size` is clamped to 16 to 256. `null` when
  Windows has no icon. The host makes icons on a worker thread (started on first use), caches them
  by target and size, and doesn't cache a failure.
- `mypaper.user.info()` (permission `user`) returns `{ name, account }`: the display name
  (`GetUserNameExW(NameDisplay)`, falling back to the account name) and the account name. It is read
  on a worker thread once and cached.

## Placement presets

A manifest's `preset` says what kind of surface the widget is:

| `preset` | Is |
|---|---|
| `desktop` | The default: on the desktop layer, below app windows, laid out on the grid. |
| `overlay` | A small always-on-top widget (a now-playing pill, a clock chip). It has a cell and size on the grid like any widget, and the user edits it in edit mode with everything else, but outside edit mode it is shown in a separate **topmost, click-through-except-content** layer, one per monitor, that exists only while that monitor has a visible overlay instance. |
| `popup` | Opens from a bar module (the bars host owns this; widgets-host does not place it, and does not show it on the desktop, in edit mode or in the palette). The manifest field and the SDK are defined here so widget authors and the bars agent share the contract: the bar passes a size and anchor, the widget renders into its iframe like any other. Everything else (settings, appearance, permissions) is as for `desktop`. |

Overlay details:

- **Window**: a top-level `WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE |
  WS_EX_NOREDIRECTIONBITMAP` window with a WebView2 (the same shell page, told which instances to
  show), whose region is the union of the overlay instances' rectangles, so clicks everywhere else
  go to the windows below. Having no redirection bitmap, it has no surface of its own: DWM composes
  the page's transparent pixels with whatever is behind (without it, a widget sat on an opaque box
  the size of its cell). It never takes focus from the window you are using.
- **Toggle**: hotkey **Ctrl+Alt+Shift+O**, the tray item **Show overlay widgets** (in
  mypaper-core's tray menu, which posts the message below with `wParam` 2; not in this crate), or
  `mypaper-widgets --overlay on|off|toggle` (the registered window message `mypaper.widgets.overlay`,
  `wParam` 1, 0 or 2, like edit mode). Off hides the topmost windows and releases their WebViews
  immediately; the choice is remembered in `widgets.json` as `"overlay": true|false` (default
  `true`). Overlay instances are shown nowhere while it is off, except in edit mode.
- **State**: each layer's shell page gets its own `state` with `layer` set. The desktop layer's
  `instances` leave out overlay-preset instances (unless editing) and popup ones (always); the overlay
  layer's hold only the overlay-preset ones. `counts` and the palette (`widgets`, which leaves out
  popups) are the same in both. The host refuses `add` and `duplicate` of a popup widget, and of a
  widget whose `instances` rule is used up, with the unchanged state and a warning in the log.
- **Pausing**: like the desktop layer, per monitor: a maximized or fullscreen window covering the
  monitor hides that monitor's overlay (an overlay never draws over a game); a global hold freezes
  it; lock and display off suspend it. In edit mode the overlay layer is hidden and the overlay
  instances are drawn and edited in the desktop layer instead, so each instance is only ever
  running once.
- **Palette**: `desktop` and `overlay` widgets are offered; `popup` ones aren't.

## Host ⇄ shell

The shell talks to the host with WebView2 web messages (`chrome.webview.postMessage` and
`PostWebMessageAsJson`), one JSON object per message with a `type`. The host only accepts messages
from the shell's top-level document. Lengths are CSS pixels (DIPs), relative to the monitor's
top-left corner.

```ts
type HostToShell =
  | { type: "state";
      monitor: { device: string; dpi: number; width: number; height: number };
      work: { x: number; y: number; width: number; height: number };   // the work area
      grid: { origin: [number, number]; cell: [number, number]; gap: number; cols: number; rows: number };
      instances: { instance: string; widget: string; cell: [number, number]; size: [number, number];
                   settings: Record<string, unknown>;      // resolved: defaults plus saved values
                   appearance: Appearance;                  // resolved: every layer merged, every field set
                   own: Partial<Appearance> }[];            // only what the instance sets (the top layer)
      widgets: WidgetManifest[];                           // every installed widget (the palette)
      appearance: { base: Appearance;                      // built-in defaults < theme skin (accent may be "auto")
                    user: Partial<Appearance>;             // the user's layer (widgets.json "appearance")
                    accent: string };                      // the color "auto" stands for (palette.json, else #ffffff)
      themeCss: string | null;                             // the theme's widgets style sheet, as text
      backdrop: { url: string; width: number; height: number } | null; // monitor-aligned wallpaper snapshot (DIPs)
      glass: { intensity: number; refraction: number; highlight: number; tint: number; blur: number };
      layer: "desktop" | "overlay";                        // which window this shell page lives in; `instances`
                                                           // holds only what that layer shows (overlay: the
                                                           // overlay-preset instances, outside edit mode)
      counts: Record<string, number>;                      // widget id -> instances on ALL monitors (the
                                                           // palette's `instances` rule needs it)
      editing: boolean; visible: boolean;
      error?: string }                                     // widgets.json doesn't parse
  | { type: "reply"; instance: string; id: number | string; result?: unknown;
      error?: { code: string; message: string } }
  | { type: "event"; instance: string; event: "sub"; data: { sub: string; value: unknown } }
  | { type: "reload"; widget: string };                   // the widget's files changed

type ShellToHost =
  | { type: "ready" }                                      // the host answers with "state"
  | { type: "hello"; instance: string }                    // a frame (re)loaded: drop its subscriptions
  | { type: "call"; instance: string; id: number | string; method: string; params?: unknown }
  | { type: "layout"; changes: { instance: string; cell: [number, number]; size: [number, number] }[] }
  | { type: "add"; widget: string; cell: [number, number]; size: [number, number] }
  | { type: "duplicate"; instance: string; cell: [number, number] }
  | { type: "remove"; instance: string }
  | { type: "settings"; instance: string; values: Record<string, unknown> }
  | { type: "pickFile"; instance: string; key: string;     // a `file`/`image` setting: the host shows the
      extensions?: string[] }                                // dialog, copies the file, sets the setting
  | { type: "appearance"; instance?: string;                // the user's layer, or an instance's own
      values: { [K in keyof Appearance]?: Appearance[K] | null } }  // null clears a field ("Use default")
  | { type: "edit"; on: boolean }                          // Done / Esc
  | { type: "glass"; drawn: number };                      // decoded snapshot plates; on count changes, for diagnostics
```

- The host sends `state` after `ready` and whenever anything in it changes. The shell reconciles by
  instance id and never reloads an iframe that is still there. It answers each frame's `hello`
  from the state (and tells the host), and pushes `editmode`, `visibility`, `settings` and `size`
  to widgets when those change in the state.
- `call` carries every other wire request unchanged; the host answers with `reply`, and delivers
  subscription values as `event` (`sub`). While a monitor is hidden its widgets get nothing.
- Layout edits (`layout`, `add`, `duplicate`, `remove`, `settings`, `appearance`) are applied,
  clamped, saved, and answered with a fresh `state`, which is authoritative: a refused edit just gets
  the old state back. The shell resolves overlaps and finds free cells before sending. An
  `appearance` edit is merged into its layer (a `null` value deletes the field), validated, and
  refused whole if anything is out of range.
- Appearance reaches widgets through the shell: its `hello` answer and its `appearance` event carry
  `state`'s resolved `appearance` and `themeCss` for that instance. While the Appearance panel is
  open the shell previews edits by sending the `appearance` event itself, merged locally from the
  same layers (`base`, `user`, `own`, `accent`), and drops the preview at the next `state`.
- The host also injects a small hardening script (`widgets/shell/dist/harden.js`) into every frame
  at document creation (`AddScriptToExecuteOnDocumentCreated`); see [Network](#network).

## Network

Two layers, so a widget reaches exactly the origins its manifest lists.

**The browser's own policy.** Everything served from `https://<id>.widget.mypaper/` carries

```text
Content-Security-Policy: default-src 'self' 'unsafe-inline' 'unsafe-eval' data: blob: <each network: origin>;
                         style-src 'self' 'unsafe-inline' data: blob: <each network: origin>;
                         object-src 'none'; base-uri 'self'
```

(`style-src` repeats the default on purpose: inline styles must stay allowed so the widget kit can
insert the theme's style sheet.)

Chromium enforces it on every connection the widget's documents and workers make (`fetch`, XHR,
WebSockets, EventSource, beacons, images, fonts, media, frames), including about:blank and srcdoc
frames the widget creates, which inherit it. As in CSP, an `https://host` origin also admits
`wss://host`.

**The host's filter.** Every WebView has a `WebResourceRequested` filter for `*` (every context
and, where the runtime supports it, every request source, including workers); it also serves the
widget hosts. The host attributes each request to the frame that made it from the `Origin` header,
or failing that the `Referer` header, which Chromium sets from the requesting document and which
pages can't forge. Then:

| Request to | Allowed when the initiator is |
|---|---|
| `https://<id>.widget.mypaper/…` | the shell (loading the iframe) or the same widget |
| anything else (`http`, `https`) | a widget whose manifest lists `network:<that origin>` |

Everything else is answered with a `403` without touching the network: requests with no
attributable initiator, a `null` origin, `no-cors` requests (images, scripts) from a page that
strips its referrer (`no-referrer` breaks the widget, by design), service-worker scripts, and other
schemes. `data:` and `blob:` URLs never reach the network and aren't filtered. The shell's mapped
host (`shell.mypaper`) and WebSocket handshakes never reach `WebResourceRequested`, which is why the
policy above exists. WebRTC isn't covered by either, so the hardening script removes
`RTCPeerConnection` (and its relatives) from every frame before the page's own scripts run.

## Plugins

A plugin is a folder with `plugin.json` and an executable. It runs as a separate process, which
the host starts on first use and stops when no widget needs it (after 30 s idle) or while the host
is held.

```json
{
  "id": "weather",
  "name": "Weather",
  "version": "1.0.0",
  "command": "weather.exe",
  "args": [],
  "methods": ["current", "forecast"],
  "events": ["updated"],
  "permissions": ["network:https://api.open-meteo.com"]
}
```

- **Transport**: JSON-RPC 2.0, one JSON object per line (UTF-8, `\n`-delimited) over stdin and
  stdout. stderr goes to the mypaper log.
- **Handshake**: the host sends
  `{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"plugin":"weather","dataDir":"…"}}`.
  The plugin replies `{"jsonrpc":"2.0","id":0,"result":{"protocol":1}}` within 10 s. Calls made
  before that are queued.
- **Calls**: the host sends `{"jsonrpc":"2.0","id":N,"method":"current","params":{…}}` and the plugin
  replies with `result` or `error` within 30 s (or the widget gets `PluginUnavailable`). Only
  methods listed in `methods` are forwarded (others are `NotFound`).
- **Events**: the plugin sends notifications `{"jsonrpc":"2.0","method":"event","params":{"event":"updated","data":{…}}}`,
  and widgets subscribed via `onPlugin(id, event)` receive them. Only events listed in `events`
  can be subscribed to.
- **Lifecycle**: the host sends `{"jsonrpc":"2.0","method":"suspend"}` and `{"jsonrpc":"2.0","method":"resume"}`
  notifications on hold and release, and `{"jsonrpc":"2.0","id":N,"method":"shutdown"}` before
  terminating it (with a 2 s grace period). A plugin is in use while a widget has a call pending or
  an `onPlugin` subscription to it. A plugin that exits while widgets listen to it is restarted,
  backing off from 2 s to a minute.
- **Command**: `command` is an executable in the plugin folder, else a program on `PATH` (for
  example `python`, with `"args": ["plugin.py"]`).
- **Reserved names**: `initialize`, `shutdown`, `suspend`, `resume` and `event` can't be listed in
  `methods`. The host only forwards calls to methods listed in `plugin.json`.
- **Errors**: a standard JSON-RPC `error`; put one of mypaper's error names in `error.data.code`
  (for example `{"code":-32000,"message":"no such city","data":{"code":"NotFound"}}`) to choose how
  the widget sees it (see Wire protocol).
- **Isolation**: plugins run in a Job object (killed with the host), have no console, and are
  started with the plugin folder as their working directory.
- `permissions` is documentation for users and Studio. Plugins are native code, so only install
  plugins you trust (Studio says so).

## Bundled examples

- **Widgets** (17, all on the widget kit, so every one follows the [Appearance](#appearance) and
  every [style](#styles)). Variants are faces (`variant`) and sizes of one widget, not widgets:
  - Time: `clock` (`user`; faces Digital, Flip, Analog, Greeting and World with a `zones` list),
    `calendar` (month, agenda with local events in `storage`, strip), `timer` (pomodoro and
    countdown), `countdown` (a big number or a ring).
  - System (`system.stats`): `system-monitor` ("System": CPU, memory, GPU, network, drives and
    battery as rings, bars or graphs), `quick-toggles` (`launch`: tiles that open the matching
    `ms-settings:` page).
  - Media and web: `now-playing` (`media`, `audio`; card, artwork backdrop, strip, and a
    Visualizer look that captures audio only while it shows), `media-pill` (`media`;
    `preset: overlay`, `instances: single`), `weather` (the weather plugin, `launch`; Automatic
    location or a picked town, a `place` setting), `rss` (the rss plugin, `launch`), `crypto`
    (`network:` to CoinGecko), `quote`, `photo` (picked images, full bleed, matted or polaroid, with a
    Ken Burns drift).
  - Personal: `launcher` (`launch`: real app icons through `appIcon`, as a dock, grid or list,
    under an optional search bar), `notes` (a sticky note, storage), `todo` and `timer` (both log
    into `storage.shared:activity`), `heatmap` (a contribution heatmap of that history).
- **Replaced widgets.** A manifest may list the ids it took over in `"replaces"`, each with the
  settings an instance of the old widget gets: `{ "analog-clock": { "variant": "analog" } }`. When
  `widgets.json` loads (and when widgets are rescanned), an instance whose widget isn't installed
  but is replaced moves onto the new widget: its id (and so its storage) stays, the implied
  settings win over its saved ones, and picked-file URLs move to the new widget's origin. The host
  saves the migrated file (never one that doesn't parse), so Studio sees the new ids. An installed
  folder with the old id wins over the mapping. The bundled mapping: `analog-clock`, `world-clock`,
  `greeting` → `clock`; `system-info`, `disk`, `network`, `battery` → `system-monitor`;
  `app-shortcuts`, `search` → `launcher`; `visualizer` → `now-playing`; `photo-frame` → `photo`.
- **Plugins**: `weather` (Rust, open-meteo, no API key) and `rss` (Rust, fetches and parses feeds).
  Each is built once and its exe copied next to its `plugin.json`:
  `cargo build --release -p mypaper-plugin-weather` and `-p mypaper-plugin-rss`, then copy
  `weather.exe` / `rss.exe` into `plugins/weather/` / `plugins/rss/`.
- **Tooling**: `packages/widget-sdk` has the typed SDK, the `mypaper.js` drop-in, the JSON
  Schemas and a mock host for developing widgets in a browser. Tutorials:
  [writing-widgets.md](writing-widgets.md) and [writing-plugins.md](writing-plugins.md).

## Performance budget

- **Host idle (no widgets)**: no WebView created and 0% CPU.
- **With widgets, visible**: one WebView2 per monitor that has widgets. `system.subscribe` is
  driven by one host timer for all subscribers.
- **A monitor covered by a window**: its WebView suspended, then released after 30 s; the other
  monitors' layers are unaffected.
- **Paused by the user or on battery**: layers frozen (no widget animation, no sampling, plugins
  suspended); the host waits on its message queue.
- **Locked or display off**: WebViews suspended, then released, plugins suspended, the host at 0%
  CPU.

## How to run

```sh
pnpm -C widgets/shell install
pnpm -C widgets/shell build          # → widgets/shell/dist (index.html + harden.js)
cargo build --release -p mypaper-plugin-weather
copy target\release\weather.exe plugins\weather\   # the weather widget's plugin
cargo build -p mypaper-widgets
target\debug\mypaper-widgets.exe     # finds widgets/, plugins/ and widgets/shell/dist by walking up
target\debug\mypaper-widgets.exe --edit toggle   # or Ctrl+Alt+Shift+W
```

- Debug builds log to their console (`RUST_LOG=debug` for more); release builds log to
  `%LOCALAPPDATA%\mypaper\mypaper.log`.
- `MYPAPER_WIDGETS_DEVTOOLS=1` enables the WebView2 devtools, the default context menu (right-click
  → Inspect) and the browser shortcut keys (F12, F5).
- Saving a file in a widget folder reloads that widget's frames, so a widget's dev loop is just
  "save". The shell itself reloads on `pnpm -C widgets/shell build`.
- To try it without touching your real setup, point `APPDATA` (and `LOCALAPPDATA`) at a temporary
  folder holding `mypaper\widgets.json` and your test widgets in `mypaper\widgets\<id>\`.
- `MYPAPER_WIDGETS_PARK_SECS=<n>` changes how long a hidden layer waits before it is released
  (default 30), for testing.
- `MYPAPER_WIDGETS_IGNORE_COVERED=1` keeps monitors awake while a maximized window covers them,
  for capturing the widget layer in tests (`PrintWindow(GetShellWindow(), PW_RENDERFULLCONTENT)`
  renders the desktop, widgets included, without touching the windows in front).
- `pnpm -C widgets/shell test` runs the grid math, permission and appearance-layer tests (vitest).
- To drive the shell page from a script, set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`
  before starting the host and attach over the DevTools protocol (`http://127.0.0.1:9333/json/list`).
