# Top bar, dock and taskbar: the contract

This is the spec for mypaper's shell bars: a **top bar**, a **dock** that can replace the Windows
taskbar (as a macOS-style dock or a full-width Windows-style taskbar), and options for the **native
taskbar**. It's the source of truth for `crates/bars` (the `mypaper-bars` binary), its page
`bars/shell`, the wallpaper engine's snapshots for wallpaper glass (`crates/wallpaper/src/backdrop.rs`),
core's supervision and tray entries, and the Studio settings. Change it deliberately, in the same
commit as the code.

## Principles

- **Our own windows, supported APIs.** Bars are our own windows (one transparent WebView2 per bar per
  monitor). The native taskbar is only shown, hidden or set to auto-hide (`ShowWindow`,
  `SHAppBarMessage`) and aligned (its documented registry value). No code goes into Explorer.
- **Always reversible.** Anything done to the native taskbar is journaled
  (`%LOCALAPPDATA%\mypaper\taskbar.json`) and undone when the bars exit, on the panic hotkey, and at
  the next start after a crash.
- **Reserved space, like a real taskbar.** Docked bars register as app bars (`ABM_NEW` / `ABM_SETPOS`),
  so maximized windows and the widget grid stop at them. An auto-hiding dock reserves nothing and
  slides in at the screen edge.
- **A look is data.** Layout, materials, states and motion are `bars.json` fields (typed, checked,
  layered over a preset); the page turns them into CSS variables. Themes add CSS and tunable
  variables on top.
- **Light.**
  - Window lists come from WinEvent hooks; nothing polls.
  - The clock ticks once a minute, aligned to the minute (or once a second if seconds are shown).
  - Stats, media, network, volume, the tray, custom modules and the Recycle Bin run only while a
    module that shows them is on screen.
  - Every animation is CSS (transitions and keyframes the compositor runs); there are no
    `requestAnimationFrame` loops, and nothing animates forever: attention pulses a few times and
    stops, a launch bounce stops when the window appears (or after 4 s).
  - While a fullscreen app is in front, that monitor's bars hide, and the bars elsewhere stop
    their periodic work (stats, custom modules); the clock and event-driven modules keep going.
  - Measured (debug build, two 1440p monitors, two top bars and a dock): 0.03% of the machine's
    CPU at idle; about 520 MB working set for the host and WebView2's processes (one browser, one
    GPU process, one renderer per bar).
  - Measured with phase 2 (release build, two 1440p monitors, a top bar and a dock on each, the
    `monochrome` preset with wallpaper glass over an animated shader): 0.05% of the machine's CPU
    at idle over 15 s for the bars host and its WebView2 processes; 685 MB working set (4 bars).
    Each wallpaper snapshot costs the engine's frame thread 0.04–0.08 ms (copying the read-back
    pixels); encoding and writing it takes 1.7–2.2 ms on a side thread; the GPU pyramid and
    readback took about 1 ms on the integrated GPU in a headless test. Nothing runs while held.

## Process

```text
mypaper-core ── supervises (optional, like the widget host) ──► mypaper-bars (crates/bars)
   ├─ per monitor: top bar window and/or dock window, each a WebView2 on https://bars.mypaper/
   ├─ system model (Rust): windows, apps, icons, media (SMTC), volume, network, battery, stats, clock,
   │    attention (shell hook), Windows' accent and light/dark mode, custom modules, Recycle Bin
   ├─ native taskbar control + undo journal (also a library: core replays it)
   └─ bridge: page ⇄ host JSON messages (below)
mypaper-wallpaper ── writes %LOCALAPPDATA%\mypaper\backdrop\<DEVICE>.bmp ──► read by the bars
```

- **Single instance.** It uses the named mutex `Local\mypaper-bars`. The message-only control window
  has class `mypaper.bars.host`. **Posting `WM_CLOSE` to it** makes the bars restore the native
  taskbar, remove their app bars and exit; that's how core stops them.
- **Broadcasts.** A hidden top-level window (class `mypaper.bars.broadcast`) hears what
  message-only windows can't: `TaskbarCreated`, display, power, time and setting changes
  (`WM_SETTINGCHANGE`: colors, light/dark mode, animation effects). It's also the **shell hook
  window** (`RegisterShellHookWindow`, a documented API: Windows sends it `HSHELL_*` messages, nothing
  runs in Explorer) and receives the Recycle Bin's change notifications (`SHChangeNotifyRegister`).
- **Config.** It watches `%APPDATA%\mypaper\bars.json` and `config.json` (hold reasons, same rules
  as the wallpaper), the active theme's folder for its `bars` skin, `palette.json`, the wallpaper
  snapshots folder, and `bars/shell/dist` (a rebuilt page reloads every bar).
- **Explorer restart.** On `TaskbarCreated` (after a second's settle) it re-applies the
  native-taskbar settings and registers its app bars again.
- **Displays.** On `WM_DISPLAYCHANGE` / `WM_DPICHANGED` it re-reads the monitors, adds, moves and
  removes bars, and re-applies the taskbar mode (a monitor that comes back has a fresh secondary
  taskbar).
- **The page** is `bars/shell/dist`, found beside the exe or any parent, else under the current
  folder or any parent. Without it no bars are created and **the taskbar is kept**, whatever
  `taskbar.mode` says, so nothing is left without a taskbar.
- **Files.** WebView2 profile: `%LOCALAPPDATA%\mypaper\webview2-bars\` (created in one function,
  `web::create_environment`). Each image folder is its own virtual host the page loads from:

  | Host | Folder | What |
  |---|---|---|
  | `icons.bars.mypaper` | `%LOCALAPPDATA%\mypaper\icons\` | app icons (cache) |
  | `packs.bars.mypaper` | `%APPDATA%\mypaper\icons\` | the user's icon packs |
  | `backdrop.bars.mypaper` | `%LOCALAPPDATA%\mypaper\backdrop\` | wallpaper snapshots |
  | `assets.bars.mypaper` | `%LOCALAPPDATA%\mypaper\bars-assets\` | copies of images `bars.json` names |

  `MYPAPER_BARS_DEVTOOLS=1` enables devtools and context menus.

## Config: `bars.json`

```json
{
  "version": 1,
  "preset": "monochrome",
  "taskbar": { "mode": "keep", "alignment": "center" },
  "top": {
    "enabled": true,
    "monitors": "all",
    "layout": "strip",
    "position": "top",
    "height": 32,
    "left":   ["start", "workspaces", "app"],
    "center": ["clock"],
    "right":  ["media", "tray", "network", "volume", "battery", "quick", "power"],
    "clock":  { "seconds": false, "format": "ddd d MMM  HH:mm" }
  },
  "dock": {
    "enabled": true,
    "monitors": "primary",
    "style": "floating",
    "position": "bottom",
    "size": 48,
    "autohide": false,
    "magnify": { "scale": 1.5, "range": 3 },
    "running": true,
    "indicator": "dot",
    "pinned": [
      { "path": "C:\\Windows\\explorer.exe", "name": "File Explorer" },
      { "path": "shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", "name": "Terminal" }
    ]
  },
  "look": { "background": { "type": "wallpaper" }, "tint": 0.6 },
  "animation": { "curve": "smooth", "bar": { "style": "fade" } },
  "modules": { "clock": { "click": { "middle": { "exec": "notepad.exe" } } } },
  "appearance": { },
  "overrides": { "DISPLAY2": { "dock": { "position": "left" } } }
}
```

- **Every field is optional**; a missing one takes the preset's value, else the default shown
  above, except `dock.enabled` (default `false`) and `dock.pinned` (below). `{}` is the same as no
  file.
- **Layers.** The file is deep-merged over its **preset** (objects merge key by key; lists and
  values are replaced), and per-monitor **overrides** over that (see "Per-monitor overrides"). A
  theme adds `appearance`, CSS and variables (see "Look").
- **`taskbar.mode`**: `keep` (untouched), `autohide`, or `hide` (hidden completely, on every
  monitor; its space goes to our bars).
  - Changing it takes effect live. Leaving `hide` or `autohide` restores the exact previous state from
    the journal.
  - `hide` also makes the taskbar auto-hide first, so the work area is freed even if Explorer re-shows
    it.
  - **In `hide` the taskbar never stays visible.** Explorer shows it again on some events (Start
    and the Windows key, notifications, display and resolution changes, `TaskbarCreated`). The bars
    watch the taskbar windows with out-of-context WinEvent hooks (`EVENT_OBJECT_SHOW`, and
    `EVENT_OBJECT_LOCATIONCHANGE` for an auto-hidden taskbar sliding in) and hide them again as the
    event arrives. They never give up; re-hides are only spaced 50 ms apart, so a fight with Explorer
    stays cheap. Each re-hide also checks that auto-hide is still set, and sets it again if not.
  - Presets never set `taskbar`: replacing the native taskbar is always the user's choice. The
    `windows11` preset with `"taskbar": { "mode": "hide" }` is a complete taskbar replacement.
- **`taskbar.alignment`**: `center` or `left` (the `TaskbarAl` value Settings writes, announced with
  `WM_SETTINGCHANGE` `"TraySettings"`). Omit it to leave Windows' setting alone; removing it
  restores the original value.
- **`top` / `dock` `monitors`**: `all`, `primary`, or a list of device names (`\\.\DISPLAY2` or
  `DISPLAY2`, any case).
- **`top.clock.format`** tokens: `dddd` `ddd` `d` `dd` `MMMM` `MMM` `MM` `M` `yyyy` `yy` `HH` `H`
  `hh` `h` `mm` `ss` `tt`; `'quoted text'` and anything else is literal (a `\n` makes two lines).
  Default `ddd d MMM  HH:mm`, or `ddd d MMM  HH:mm:ss` with `seconds`. At most 64 characters. The
  dock's `taskbar` style uses the same clock settings.
- **`dock.pinned[].path`** is an exe, a shortcut (`.lnk`), a folder, or an `shell:AppsFolder\<AUMID>`
  path (Store apps). **Missing `pinned`** means the native taskbar's pinned shortcuts
  (`%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\*.lnk`, in name order;
  Store apps pinned there aren't shortcuts and don't come along), or File Explorer when there are
  none. An empty list is a choice and stays empty.
- **Module lists** (`top.left|center|right`, `dock.left|right`) hold built-in modules,
  `custom:<id>` and `|` (a break: a new island in the `islands` layout, a thin separator elsewhere).
  Each module is listed at most once per bar.
- **`performance`**: `auto` (on while on battery), `on`, `off`. Performance mode turns off blur,
  grain, glow and every animation, and stops periodic wallpaper snapshots.
- **Unknown fields are rejected**, everywhere (look and animation included). A file that fails to
  parse is logged and ignored, and the last good config keeps running.
- **Writers change only what they set.** The tray, Studio and the dock's pin edits read the file as
  JSON, set their field(s), check that the result is still valid, and write it atomically. They
  refuse to touch a file that doesn't parse, so a typo is never overwritten, and a file never gains
  the defaults it left out.
- **No file means built-in defaults**: top bar on, dock off, taskbar `keep`. The bars stay out of the
  way until enabled, and the tray offers to enable them.

### Per-monitor overrides

`overrides` maps a monitor to a partial `bars.json` merged over the rest for that monitor's bars.
Keys are `primary`, `secondary` (every other monitor) or a device name (`DISPLAY2`); a device's
own override goes over its role's. An override may set `preset`, `top`, `dock`, `look`,
`animation`, `modules`, `performance` and `appearance`, but not the global fields (`version`,
`taskbar`, `overrides`, `vars`, `icons`) nor `dock.pinned` (pins are shared). Every override must
still make a valid config, checked when the file loads.

```json
"overrides": {
  "secondary": { "top": { "enabled": false } },
  "DISPLAY3":  { "preset": "islands", "dock": { "enabled": true, "position": "left", "size": 40 } }
}
```

## Presets

A preset is a partial `bars.json` (`crates/bars/presets/<name>.json`: layout, look, motion, dock
behavior) plus CSS the page bundles (`bars/shell/src/presets/presets.css`, scoped by
`data-preset`). The user's fields go over it.

| `preset` | Top bar | Dock | Look and motion |
|---|---|---|---|
| `monochrome` (default) | strip, 32 | floating, dots | black wallpaper glass (tint 0.6), white type, fine grain, fade-in, pop flyouts |
| `macos` | strip, 28, clock on the right | floating, 52 px, magnify 1.8, trash | light or dark with Windows (`scheme: auto`), very translucent vibrant glass, squircle 22, spring motion |
| `windows11` | off | `taskbar`: Start, search box, centered task buttons, tray, two-line clock, show-desktop sliver | Mica-like glass by light/dark mode, accent pill indicators, mono icon pack, Fluent timing |
| `islands` | islands, one pill per group | floating, bar indicators | solid dark pills, squircle 12, hover borders, snappy pop-in |
| `neon-glass` | floating, hugging its modules | floating, glow indicators | dark wallpaper glass, magenta→cyan gradient edge, violet glow, neon clock, overshoot motion |
| `floating` | floating, full width | floating, 10 DIP margin | wallpaper glass, squircle 14, soft spring |

## Layouts and geometry

All sizes are DIPs.

- **`top.layout`**: `strip` (edge to edge, square, one hairline toward the windows), `floating`
  (detached by `margin`, rounded by `look.radius`) or `islands` (a transparent rail; each group of
  modules between `|` is its own surface, sized to its content).
- **`top.position`**: `top` or `bottom`.
- **`top.height`** (20–80) is the bar itself. **`top.margin`** (0–64; default 0 for `strip`, 8
  `floating`, 6 `islands`) keeps it off the screen edges, and is also the gap between islands.
  **`top.padding`** (0–48) is inside the bar or each island, **`top.spacing`** (0–32) between modules.
- **`top.width`** (floating): `full` (default), `auto` (hug the modules, centered), DIPs (120–8192)
  or `"10%"`…`"100%"`, centered.
- **The top bar's band** (what it reserves) is `height + 2 × margin`.
- **`dock.style`**: `floating` (a pill of icons centered on its edge, macOS-style) or `taskbar` (a
  full-width bar: `dock.left` modules and the task buttons as one group, centered or at the start
  by `dock.align`, then `dock.right` modules and the extras).
- **`dock.position`**: `bottom`, `top`, `left` or `right`.
- **`dock.size`** (16–128) is the icon size (default 48 `floating`, 24 `taskbar`); **`dock.padding`**
  (default 5 / 12) surrounds the icons; **`dock.margin`** (default 6 / 0) separates the pill from
  the screen edge, and the same room is kept toward the windows; **`dock.spacing`** (default 6 / 4)
  is between items.
- **The dock's band** is `size + 2 × padding + 2 × margin` (70 for the default floating dock, 48 for
  a 24 px taskbar). The margins leave the surface's shadow and glow room: **the window region is the
  band** (plus at least 8 DIPs around the pill), so a shadow or glow larger than the margin is cut
  off at its edge. Presets keep theirs inside.
- The page gets every geometry field resolved (`margin`, `padding`, `spacing`, `width`, `size`).

## Look

Bars layer the same `Appearance` as widgets (see docs/widgets.md, "Appearance"):

1. Built-in defaults: `glass`, tint `#0e0e0e`, opacity 0.6, blur 30, radius 0 for the top bar and 16
   for the dock, text `light`, `border` on, accent `#ffffff`, font Segoe UI Variable, scale 1,
   `shadow` off.
2. The theme's `"bars": { "appearance": …, "css": "bars.css" }`.
3. `bars.json` → `appearance`.

The host resolves these (`accent: auto` takes the palette's accent, else white). On top of the
appearance, **`look`** describes the surfaces (the top bar, each island, the dock's pill) and the
items (module buttons, task buttons):

```json
"look": {
  "scheme": "auto",
  "background": { "type": "wallpaper", "brightness": 0.9 },
  "tint": 0.55,
  "saturation": 1.4,
  "noise": 0.04,
  "radius": 14,
  "corners": "squircle",
  "border": { "rest": { "width": 1, "color": { "angle": 120, "stops": ["#ff2bd6", "#00e5ff"] } },
              "hover": { "width": 1.5 } },
  "glow":   { "hover": { "color": "accent", "size": 12, "opacity": 0.5 } },
  "shadow": { "rest": { "y": 3, "blur": 10, "color": "#00000052" } },
  "items":  { "radius": 8, "hover": { "background": "#ffffff1a", "scale": 1.04 },
              "active": { "background": "#ffffff2b", "color": "#ffffff" } },
  "indicator": { "running": "#ffffff73", "active": "accent", "attention": "#ffb340" }
}
```

- **Colors** are `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, a token (`accent`, `text`, `transparent`,
  `system-accent`, `system-accent-light-1`…`-3`, `system-accent-dark-1`…`-3`), or `var(--name)` (a
  theme variable). **Paints** (borders, item backgrounds) are a color or a gradient
  `{ "kind": "linear"|"radial"|"conic", "angle": 0–360 (default 180), "stops": [color | { "color", "at": 0–1 }] }`
  with 2–8 stops.
- **`scheme`**: `light`, `dark`, or `auto` (Windows' light or dark mode, live). Missing: from
  `appearance.text`. The scheme sets the text colors; a tint made for the other scheme gives way to
  a neutral of this one. `<html data-scheme>` lets CSS do the rest.
- **`background`** (by `type`):
  - `solid` `{ color }`;
  - `gradient` `{ kind, angle, stops }`;
  - `image` `{ path, fit: cover|contain|fill|tile, position (up to two of center/top/bottom/left/right/<n>%), brightness 0–2, blur 0–64 }`:
    `path` is absolute or relative to the active theme's folder; the host copies the file into
    `bars-assets` and serves it;
  - `wallpaper` `{ brightness 0–2, blur 0–64, interval 2–600 s (default 10) }`: wallpaper glass,
    below;
  - `none`; missing: the appearance's tint at its opacity.
- **`tint`** (0–1): the appearance's tint laid over an image, gradient or wallpaper background
  (default: the appearance's opacity for wallpaper glass, 0.25 for images, 0 for gradients).
  **`saturation`** (0–3) of the wallpaper or image behind. **`noise`** (0–0.5): a fine grain
  texture over the surface.
- **`radius`** (0–64; missing: the appearance's) and **`corners`**: `round`, `squircle`, `bevel`,
  `scoop`, `notch` (CSS `corner-shape`). A `strip` top bar is always square.
- **States**: `border`, `glow` and `shadow` each take `rest`, `hover` and `active` (pressed);
  hover and active start from rest. `border` is `{ width 0–8, color: paint }`, `glow`
  `{ color, size 0–64, opacity 0–1 }`, `shadow` `{ color, x, y (±64), blur 0–96, spread ±32 }`.
- **`items`**: `radius` (0–32) and `rest`/`hover`/`active` `{ background: paint, color, border,
  glow, scale 0.5–1.5 }`. Active is pressed, a module whose flyout is open (`.mod.is-open`), and the
  app in front on a task button.
- **`indicator`**: the running indicator's colors (`running`, `active`, `attention`).

The page sets the appearance and the look as CSS variables and `data-*` attributes on `<html>`
(see "Styling"), plus `data-bar="top|dock"`, `data-edge`, `data-preset`, and inserts its generated
CSS (icon pack, module overrides) and then the theme CSS last.

### Wallpaper glass

A WebView can't see what's behind its window, so `backdrop-filter` only blurs the page's own
content. But mypaper owns the wallpaper's pixels, so the engine publishes them:

- **Wallpaper side** (`crates/wallpaper/src/backdrop.rs`). After a screen's frame is drawn, the
  engine copies the swapchain image, shrinks it on the GPU through a bilinear pyramid (1/2 down to
  1/64 and back up to 1/8: a frosted blur of tens of pixels), reads the 1/8 image back and writes
  `%LOCALAPPDATA%\mypaper\backdrop\<DEVICE>.bmp` (`DISPLAY1.bmp`: uncompressed 24-bit, top-down),
  through a `.tmp` file and an atomic rename. For an animated wallpaper the readback is
  asynchronous: a later frame picks the pixels up (a 230 KB copy) and a short-lived thread encodes
  and writes them, so the frame loop never waits on the GPU; only a static wallpaper's single
  publish waits for its frame.
  - Published **once per theme load** per screen, on its first drawn frame.
  - While the wallpaper is **animated**, again every `interval` seconds, but only while
    `backdrop\request.json` (`{ "interval": N }`) exists; the file is read at most once per
    interval and only on frames drawn anyway, so publishing never wakes the engine.
  - **Never while held** (fullscreen, paused, locked, display off) or for a covered screen; never
    for web and app wallpapers, which draw their own windows (their stale snapshot is deleted).
  - A static wallpaper's GPU textures are freed after its one publish.
- **Bars side** (`crates/bars/src/glass.rs`). While a visible bar uses `background.type: wallpaper`
  and performance mode is off, the bars write `request.json` with the shortest interval asked
  (deleted otherwise, and on exit). They watch the folder and send each bar a `backdrop` event: the
  snapshot's URL (with its modification time), the monitor's size and the window's offset on it.
- **The page** draws the snapshot as the surface's background layer with
  `background-attachment: fixed`, sized to the monitor and offset by the window's place on it, so
  it lines up with the real wallpaper behind the bar; the tint, grain and border go over it. Until
  a snapshot exists the surface uses the plain tint.

### Theme variables

A theme's bars CSS can declare user-tunable variables in one comment, a JSON object keyed by
variable name:

```css
/* @mypaper-vars
{
  "--glow":      { "type": "color",  "label": "Glow", "default": "#ff2bd6" },
  "--glow-size": { "type": "length", "label": "Glow size", "min": 0, "max": 40, "step": 1, "unit": "px", "default": 14 },
  "--gap":       { "type": "number", "label": "Gap", "min": 0, "max": 2, "step": 0.1, "default": 0.5 },
  "--labels":    { "type": "bool",   "label": "Show labels", "group": "Dock", "default": true },
  "--shape":     { "type": "select", "label": "Shape", "options": ["soft", "sharp"], "default": "soft" }
}
*/
.pill-surface { box-shadow: 0 0 var(--glow-size) var(--glow); }
@container style(--labels: 0) { .dock-label { display: none; } }
```

- Fields: `type` (`color` `#rrggbb[aa]`, `number`, `length` with `unit` `px` (default) `em` `rem`
  `%` `ms` `deg`, `select` with `options` (CSS keywords), `bool`), `label`, optional `group`,
  `description`, `min`, `max`, `step`, and `default`. Unknown fields are errors.
- **Studio** renders a control per variable (`mypaper_bars::vars::declared(css)` gives the list)
  and writes values to `bars.json` → `vars` (`{ "--glow": "#00ffaa" }`).
- **The host** checks each value against its declaration (type, range, options); a value that
  doesn't fit, or a variable the theme doesn't declare, is logged and the default is used. The page
  sets the results on `:root`: colors as written, numbers bare, lengths with their unit, selects as
  the keyword, bools as `1` or `0` (usable in `calc()` and `@container style()`).

### Windows' accent and light/dark

The page always has Windows' accent as `--system-accent` and `--system-accent-light-1`…`-3`,
`--system-accent-dark-1`…`-3` (from `HKCU\…\Explorer\Accent\AccentPalette`, the palette Settings
writes), and `data-system-scheme="light|dark"` (`SystemUsesLightTheme`). Both follow live changes
(`WM_SETTINGCHANGE`).

## Animation

Every animation is CSS, timed by variables the page sets from the **animation tree**:

```json
"animation": {
  "enabled": true, "duration": 200, "curve": "smooth", "reduceMotion": "system",
  "bar":   { "style": "slide",
             "top":  { "duration": 240 },
             "dock": { "curve": { "mass": 1, "stiffness": 220, "damping": 20 },
                       "magnify": { "duration": 70, "curve": "linear" },
                       "bounce": { "duration": 520 }, "attention": { "curve": "ease-in-out" },
                       "hover": { "enabled": false } } },
  "hover": { "duration": 120 },
  "press": { "curve": "snappy" },
  "open":  { "style": "pop", "duration": 160 }
}
```

- **Nodes**: `animation` (global) → `bar` (both bars) → `bar.top` / `bar.dock`. **Leaves**: `hover`,
  `press` and `open` (flyouts and menus) may sit under any node; `magnify`, `bounce` (launch) and
  `attention` only under `bar.dock`. A node itself is its bar's **show** animation (the top bar
  appearing, the dock's auto-hide reveal). Other shapes are rejected.
- **Fields**: `enabled`, `duration` (ms, 0–5000), `curve`, `style` (`slide`, `fade`, `pop`,
  `scale`, `none`; for shows and `open`).
- **Inheritance**: for each field the most specific value wins, in this order: `animation`, `bar`,
  `bar.<top|dock>`, then the leaf under each of those. Any `enabled: false` on the path turns the
  leaf off. `magnify`, `bounce` and `attention` take their timing only from `bar.dock.<leaf>`
  (defaults 80 ms linear, 560 ms, 900 ms), so a slow global duration never makes magnify lag.
- **Curves**: a name (`linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`, `smooth`, `snappy`,
  `emphasized`, `overshoot`, `spring`, `spring-soft`, `spring-bouncy`), a cubic Bézier
  `[x1, y1, x2, y2]` (x in 0–1, y in −2–3), or a spring `{ "mass" 0.1–10, "stiffness" 1–2000,
  "damping" 0–200 }`. A spring is rendered as a CSS `linear()` easing sampled from the damped
  oscillator (60 points per second, collinear points dropped) and **sets its own duration**: the
  time it takes to settle within 0.1% (at most 3 s).
- **Reduce motion**: `reduceMotion` `system` (default: Windows' "Animation effects",
  `SPI_GETCLIENTAREAANIMATION`, followed live), `always` or `never`. Reduced, every duration is 0.
  **Performance mode** (above) reduces motion too.
- The page sets `--m-<leaf>` (duration) and `--m-<leaf>-ease` for `show`, `hover`, `press`, `open`,
  `magnify`, `bounce`, `attention`, plus `data-m-show`, `data-m-open` (styles) and `data-motion`
  on `<html>`. Flyouts play their `open` style in and fade out (`.leaving`) when they close.

## Modules

These are the items of the top bar and of a `taskbar` dock. Each is a small component in
`bars/shell`. Clicks run host commands; the host does the Windows work.

| Module | Shows | Click |
|---|---|---|
| `start` | a mypaper mark | opens the Start menu (`Shell_TrayWnd` `WM_SYSCOMMAND` `SC_TASKLIST`; the Windows key without a taskbar) |
| `search` | a search glyph (a search box in `windows11`) | Windows Search (Win+S, on the user's click) |
| `workspaces` | virtual desktops: count and current (`VirtualDesktopIDs` / `CurrentVirtualDesktop` under `HKCU\…\Explorer\VirtualDesktops`, watched with `RegNotifyChangeKeyValue`; absent values mean one desktop) | switch desktop: Win+Ctrl+arrow, sent only on the user's click (once per step) |
| `app` | the foreground app's name and icon (the last app window in front: clicking a bar doesn't change it) | — |
| `clock` | time and date (format in `bars.json` → `top.clock`) | opens the calendar / notification center (`ms-actioncenter:`) |
| `media` | now playing (SMTC): title and artist | play/pause; scroll for next and previous |
| `tray` | the notification-area icons (see below) | left/right click forwarded |
| `network` | wifi signal and name (WLAN API) / ethernet (the interface table: an up, physical, wired interface) / offline; changes from `NotifyIpInterfaceChange` and WLAN notifications | opens quick settings |
| `volume` | level and mute (Core Audio `IAudioEndpointVolume` on the default output, with its change callback) | toggle mute; scroll to change (raising it unmutes) |
| `battery` | percent and charging (`GetSystemPowerStatus`, refreshed on `PBT_APMPOWERSTATUSCHANGE`); hidden on desktops | power settings |
| `quick` | a settings glyph | opens quick settings (`ms-actioncenter:controlcenter/`) |
| `power` | a power glyph | menu: lock, sleep, sign out, restart, shut down (restart and shut down ask first) |
| `stats` | CPU / RAM mini gauges (sampled every 2 s) | Task Manager |
| `custom:<id>` | a command's output (below) | only what `modules` binds |

### Per-module options

`modules` maps a module name to its options:

```json
"modules": {
  "clock":  { "click": { "left": "notifications", "middle": { "exec": "notepad.exe", "args": ["todo.txt"] },
                         "right": { "open": "ms-settings:dateandtime" }, "wheelUp": "volume-up" },
              "style": { "color": "accent", "padding": 12, "weight": 650 } },
  "quick":  { "icon": "C:\\Icons\\sliders.svg" },
  "custom:weather": { "command": "curl.exe", "args": ["-s", "wttr.in/?format=%c+%t"], "interval": 900,
                      "click": { "left": { "open": "https://wttr.in" } } }
}
```

- **`click`** binds `left`, `middle`, `right`, `wheelUp`, `wheelDown` to a **built-in action**
  (`default` (the module's own), `none`, `start`, `search`, `notifications`, `quick-settings`,
  `task-manager`, `settings`, `show-desktop`, `media-toggle`, `media-next`, `media-previous`,
  `volume-up`, `volume-down`, `volume-mute`, `workspace-next`, `workspace-previous`, `lock`,
  `explorer`), to `{ "exec": program, "args": [...] }` (run as the user, detached) or to
  `{ "open": path-or-URI }`. Unbound inputs keep the module's behavior. The page binds them in the
  capture phase on the module, so only the bound inputs are taken over; wheel notches closer than
  120 ms count once.
  **`exec` and `open` run from the host's own copy of the config**: the page asks `run { module,
  trigger }` and never sends a command line.
- **`style`**: `color`, `background` (paint), `border`, `padding` (horizontal, 0–32), `radius`
  (0–32), `size` (font, 8–32), `weight` (100–900).
- **`icon`**: an icon-pack key, or an image file (absolute or theme-relative, served from
  `bars-assets`): it replaces the module's glyph (a mask in the text color for SVG, else a picture).

### Custom modules

A `custom:<id>` module (id: 1–32 of `a-z 0-9 - _`) needs `modules."custom:<id>".command`, with
`args` and `interval` (seconds, 1–86400, default 60). While a bar on screen shows it, the host runs
it on a worker thread, with no console window and a 10 s limit (then it's killed), and shows what
it printed: the first line as text, or a JSON object `{ "text", "icon", "tooltip", "class" }`
(`icon`: an emoji, or an icon-pack key; `class`: a word set as `data-class` for themes). A failure
dims the module and shows the error as its tooltip. Like stats, custom modules stop while a
fullscreen app is in front elsewhere. They come to the page as the `custom` event.

### The tray

**The tray is best effort.** Notification-area icons belong to Explorer. The host reads them through
UI Automation (a supported accessibility API) from the (possibly hidden) taskbar, on a worker
thread, and refreshes on UIA structure-changed events. What Windows 11 (build 26300, "26H2")
exposes:

- Each *promoted* icon is a `SystemTray.NormalButton` with AutomationId `NotifyItemIcon`; its Name is
  the tooltip. It supports `Invoke`.
- The overflow chevron is the `SystemTray.NormalButton` with AutomationId `SystemTrayIcon` left of
  them. Hidden icons only exist in the tree while the overflow flyout is open, so they're reached
  through it: it's always listed last, with id `overflow`.
- UIA gives no icons. The host uses the PNG snapshot the shell keeps under
  `HKCU\Control Panel\NotifyIconSettings\<id>\IconSnapshot`, from the entry whose `InitialTooltip`
  starts the button's name, else whose exe's `FileDescription` or file name appears in it. Items
  without a match show a monogram.
- Left click is UIA `Invoke`. Right click focuses the button (UIA `SetFocus`) and sends the
  context-menu key (Shift+F10), which the tray turns into the app's context menu.
- **With `taskbar.mode: hide` there are no tray icons**: Explorer removes a hidden taskbar's whole
  XAML content from the UIA tree (measured: no `SystemTray.*` element at all), so there's nothing
  to read or invoke. The icons are there in `keep` and `autohide` (an auto-hidden taskbar is only
  slid off-screen), so use `autohide` to keep the tray module working.
- If UI Automation is unavailable or finds no tray (as in `hide`), the module shows a single
  `overflow` button that tries the native overflow flyout (Win+B, Enter). The fallback is logged.

### Icon packs

An icon pack is a folder with an `icons.json` and image files, in `%APPDATA%\mypaper\icons\<pack>\`
(or bundled with the page in `bars/shell/public/icons/<pack>\`; `mono` ships: a monochrome set for
the modules). `bars.json` → `icons` names the pack in use (the `windows11` preset uses `mono`).

```json
{ "name": "Mono", "mono": true, "missing": "app.svg",
  "entries": [
    { "umid": "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", "icon": "terminal.svg" },
    { "path": "code.exe", "icon": "code.png", "mono": false },
    { "path": "C:\\Tools\\thing.exe", "icon": "thing.svg" },
    { "extension": "txt", "icon": "text.svg" },
    { "key": "wifi-3", "icon": "wifi-3.svg" }
  ] }
```

- Each entry sets exactly one of `umid` (AppUserModelID), `path` (a full path, or a file name that
  matches in any folder), `extension` (of a pinned file) or `key` (one of the bars' glyphs), and
  `icon`, a file in the folder (`svg`, `png`, `ico`, `webp`, `jpg`). `mono` (pack-wide, or per
  entry) draws the icon as a mask in the text color instead of as a picture. At most 4000 entries;
  unknown fields are errors.
- **Apps** (dock items; the host sends each item's AUMID and exe) match, most specific first: AUMID,
  full path, file name, extension, then `missing` (else the app's own icon).
- **Glyph keys**: `start`, `search`, `wifi-1`…`wifi-4`, `offline`, `ethernet`, `volume-mute`,
  `volume-low`, `volume-mid`, `volume-high`, `battery`, `bolt`, `quick`, `power`, `play`, `pause`,
  `chevron`, `desktop`, `trash`, `trash-full`. Every built-in glyph is an inline SVG tagged with its
  key (`<svg data-glyph="wifi-3">`), so a pack swaps it with generated CSS alone.
- The host loads and checks the pack (`crates/bars/src/icon_pack.rs`) and sends it with its base
  URL; the page matches it (`bars/shell/src/style/icon-pack.ts`).

## Dock

- **Items.** Pinned items come first, in order, then running apps that aren't pinned, in the order
  they started (focusing a window never reshuffles the dock).
  - **`running`**: `true` (all running apps), `false` (pinned only), or `"monitor"`: only apps with
    a window on this dock's monitor (where a minimized window restores to), and pinned items count
    only those windows, like the taskbar's "show apps on the taskbar where the window is open".
  - **Running apps** are top-level windows a taskbar would show: visible, not cloaked, and either
    `WS_EX_APPWINDOW` or unowned, not a tool window and not `WS_EX_NOACTIVATE`. The desktop, the
    taskbars and UWP's inner `Windows.UI.Core.CoreWindow`s never count.
  - **Grouping** is by AppUserModelID (`SHGetPropertyStoreForWindow`, `PKEY_AppUserModel_ID`),
    falling back to the process image. A pin claims the running apps whose AUMID or exe matches it
    (a shortcut's target and AUMID are read from the `.lnk`).
  - **`split`**: one item per window. An app's first window keeps its item; each other window
    follows it as its own item (titled by the window; it activates that window; dragging it
    reorders nothing).
  - **Names**: a pin's `name`, else the shell's display name (a shortcut's or AppsFolder entry's
    name), else the exe's `FileDescription`. A running app with an AUMID takes its Start menu
    entry's name and icon (`shell:AppsFolder\<AUMID>`), which is also what launches it again.
- **Indicators** (`dock.indicator`): `dot` (macOS: a dot, wider in front), `bar` (a line under the
  icon), `glow` (a soft light), `pill` (Windows 11: a short bar, longer in front), `none`. Colors
  per state from `look.indicator` (`running`, `active`, `attention`).
- **`counter`** (default on): a badge with the number of windows on apps with more than one.
- **`titles`** (`taskbar` style, horizontal): task buttons with the window's title (168 DIPs each).
- **Attention.** A window flashing for attention (`FlashWindowEx`, which Windows reports to the
  shell hook window as `HSHELL_FLASH`) marks its item until the window comes to the front (or
  `HSHELL_WINDOWACTIVATED`, or it closes). `dock.attention`: `pulse` (a light behind the icon pulses
  six times), `bounce` (three hops) or `none`; afterwards the indicator keeps the attention color
  (a `taskbar` button keeps a tinted tile, like Windows).
- **`bounce`** (default on): a launched app (clicking an item that isn't running, or "New window")
  hops until it shows a window, at most 4 s.
- **Previews** (`dock.preview`: `true`/`false` or `{ "delay": ms }`, default 450): hovering a
  running item opens the `preview` flyout, a card per window with its title and a close button.
  The host draws each window's **live DWM thumbnail** (`DwmRegisterThumbnail`) into the bar's own
  topmost window, over the card's thumbnail box, once the card has opened (`previewOpen` returns the
  windows' sizes, the page lays the cards out and sends `previewPlace`). Moving to another item
  switches at once; leaving both for 280 ms closes it; a click on a card activates its window.
- **Extras** (`dock.extras`): `desktop` (show desktop: Win+D on the click; a sliver at the far end
  of a `taskbar`) and `trash` (the Recycle Bin: opens it, its menu empties it with the shell's own
  confirmation, and its glyph shows full or empty, from `SHQueryRecycleBinW` and shell change
  notifications on the bin).
- **Icons.** 64 px PNGs from `IShellItemImageFactory` (exe, `.lnk`, AUMID), made on a worker
  thread, cached by key in memory and in `%LOCALAPPDATA%\mypaper\icons\`, served from
  `icons.bars.mypaper`, unless the icon pack has one.
- **Clicks.**
  - **Left**: activate the app's most recent window, or minimize it if it's already in front (and
    bring the next window forward, like the taskbar). If it isn't running, launch it.
  - **Middle**: launch a new instance.
  - **Right**: menu with its windows (activate), Pin/Unpin, New window, Close window(s).
  - **Activation** uses the foreground rights the user's click grants the dock's window (the dock
    takes focus on click; it's not `NOACTIVATE`). "In front" means the last app window that had the
    foreground before the click. A minimized window is restored with `WM_SYSCOMMAND` `SC_RESTORE`
    (Chromium-based windows ignore a cross-process `ShowWindow(SW_RESTORE)`).
  - **The bars never keep the foreground unless clicked.** When a bar is activated without a click
    (WebView2 taking focus as it starts, or Windows picking a topmost bar after the active app
    minimized or closed), the host hands the foreground back to the last app window, or the top
    one, or the desktop.
- **Drag** reorders pinned items, and dragging a running app into the pinned area pins it. Dragging
  a pinned item well off the dock unpins it. All write `bars.json`. Works in both styles.
- **Magnify** (`dock.magnify`: `true`/`false` or `{ "scale" 1–3, "range" 1–8 icons }`) is a
  macOS-like hover zoom with a cosine falloff over the neighbors, following the pointer along the
  dock (CSS transforms with the `magnify` timing). Off in the `taskbar` style and with reduced
  motion.
- **Auto-hide.** The dock reserves no space and sits at the work area's edge (the screen edge
  unless the native taskbar is kept there). A 1 px transparent, non-activating window on that edge
  receives mouse moves; touching it sends the page `reveal`, and the page shows the dock with its
  `show` animation style. It hides after 1 s without the pointer (not while a menu, a preview or a
  drag is open) and then sets an empty region.

### Windows and regions

A bar's window is its band plus 360 DIPs toward the middle of the screen (clipped to the monitor),
for popovers, menus, previews and magnified icons. The page tells the host which parts are visible
and take the pointer (`region`); the host sets that as the window region (`SetWindowRgn`), so the
rest clicks through to the windows behind. The top bar's region is its whole band; the dock's is
its pill plus its margin (at least 8 DIPs), grown toward the middle while hovered (magnify and the
name label), plus any open flyout.

## Bridge (page ⇄ host)

All messages go through WebView2 `postMessage` as JSON. The page's side is
`bars/shell/src/protocol.ts`. The host accepts messages from `https://bars.mypaper/` only, and sends
events to a bar only after it said `hello`.

**Page → host requests**: `{ id, method, params }` → `{ id, result }` or `{ id, error: { code, message } }`
(`code`: `InvalidParams`, `NotFound`, `Unavailable`, `Internal`).

| Method | Params | Result |
|---|---|---|
| `hello` | — | `{ bar: "top"\|"dock", monitor: { device, primary, width, height }, config, look, mode, backdrop, dpi }` (sizes in DIPs; `config` resolved for this monitor; `look`, `mode`, `backdrop` as the events below) |
| `activate` | `{ app }` (an item's `key`) or `{ hwnd }` | — |
| `launch` | `{ path, newInstance? }` | — |
| `minimize` / `close` | `{ hwnd }` | — |
| `pin` / `unpin` / `reorder` | `{ path, name?, index? }` / `{ path }` / `{ paths }` | — |
| `start` | — | — |
| `search` | — | — |
| `showDesktop` | — | — |
| `actionCenter` | `{ panel: "notifications"\|"quick" }` | — |
| `volume` | `{ set?: 0..1, mute?: bool, delta?: -1..1 }` | `{ level, muted }` |
| `media` | `{ action: "toggle"\|"next"\|"previous" }` | — |
| `tray` | `{ id, button: "left"\|"right" }` | — |
| `power` | `{ action: "lock"\|"sleep"\|"signout"\|"restart"\|"shutdown" }` | — |
| `workspace` | `{ direction: "left"\|"right" }` | — |
| `openSettings` | `{ uri }` (an `ms-settings:` URI only) | — |
| `run` | `{ module, trigger: "left"\|"middle"\|"right"\|"wheelUp"\|"wheelDown" }` | — (runs the `exec`/`open` bound in this bar's config; `NotFound` if none) |
| `trash` | `{ action: "open"\|"empty" }` | — |
| `previewOpen` | `{ hwnds }` (at most 16) | `{ hwnd, width, height }[]` (px; windows that are gone are left out) |
| `previewPlace` | `{ windows: { hwnd, rect: [x, y, width, height] }[] }` (DIPs from the window's top-left) | — |
| `previewClose` | — | — |
| `region` | `{ rects: [x, y, width, height][] }` (DIPs from the window's top-left, at most 64) | — |

`pin` of an item that's already pinned moves it to `index`. `reorder`'s paths are matched
case-insensitively; pins it doesn't list keep their order after the listed ones.

**Host → page events**: `{ event, data }`.

| Event | Data |
|---|---|
| `apps` | the dock items: `{ key, name, path, icon?, aumid?, exe?, pinned, windows: [{ hwnd, title, active, minimized, attention }] }[]` (per dock: `running: "monitor"` differs by monitor) |
| `foreground` | `{ name, icon? }`, or null |
| `clock` | `{ now }` (ms since the epoch); the page formats it |
| `media` | `{ title, artist, app, playing }`, or null |
| `volume` | `{ level, muted }`, or null without an output device |
| `network` | `{ kind: "wifi"\|"ethernet"\|"none", name?, signal? }` |
| `battery` | `{ percent, charging }`, or null |
| `tray` | `{ id, name, icon? }[]` |
| `workspaces` | `{ count, current }` (`current` is 0-based) |
| `stats` | `{ cpu, memory: { used, total } }` |
| `custom` | `{ [id]: { text, icon?, tooltip?, class?, error? } }` |
| `trash` | `{ full }` |
| `appearance` | `{ appearance, themeCss, system: { accent: [base, light-1..3, dark-1..3], scheme }, vars, iconPack, assets }` (`vars`: the theme's variables as CSS values; `iconPack`: the pack with its `base` URL, or null; `assets`: image paths in bars.json → URLs) |
| `mode` | `{ reduceMotion, performance }` |
| `backdrop` | `{ url, width, height, x, y }` (the monitor's size and the window's offset on it, DIPs), or null |
| `config` | the parsed `bars.json` for this monitor, defaults filled in |
| `visibility` | bool |
| `reveal` | null: an auto-hiding dock's edge was touched |

Events for a module are only produced while some bar shows that module, and a value is only resent
to a bar when it changed (`appearance`, `mode` and `backdrop` too). After `hello`, and when a hidden
bar comes back, the host sends the current value of everything the bar shows.

## Hold

- **Fullscreen app** (with `pause_on_fullscreen`): the bars on its monitor hide (window hidden,
  WebView suspended, previews closed, `visibility: false` first); the other bars stay up but stop
  periodic work. Detected on foreground and move events (and `ABN_FULLSCREENAPP`), never on a
  timer.
- **Display off or session locked**: every bar is suspended and every source stops.
- **Paused by the user, or on battery**: no effect on the bars themselves (they're the user's
  taskbar), but `performance: auto` turns performance mode on while on battery.

## Core and tray

- **Core** supervises `mypaper-bars.exe` beside it, as an optional child: restarted with backoff, and
  if it's missing or crash-looping mypaper keeps running without bars.
- **At startup**, before starting the bars, core stops a bars process left by an earlier run and
  replays a leftover taskbar journal (a crash's), so the native taskbar is always back first.
- **Tray** gains a **Bars** submenu:
  - Top bar ✓ and Dock ✓ toggle `top.enabled` / `dock.enabled`.
  - Taskbar ▸ Keep / Auto-hide / Hide sets `taskbar.mode`.
- **Panic hotkey and Quit**: core stops the bars child first (`WM_CLOSE` to `mypaper.bars.host`,
  then up to 3 s), which restores the native taskbar from the journal. If the child had to be
  killed, core replays the journal itself (the journal format and replay live in `crates/bars` as a
  library used by both). Core also replays it when the bars stop for good (crash loop, missing
  page).

### The journal

`%LOCALAPPDATA%\mypaper\taskbar.json` holds each thing's original, written before its first change
and kept through later changes, so `keep` always returns to how the user had it:

```json
{ "appbar": { "was": 0 }, "hidden": true, "alignment": { "was": 1 } }
```

`appbar` is the `ABM_GETSTATE` before the bars made the taskbar auto-hide, `hidden` says they hid the
taskbar windows, and `alignment` is `TaskbarAl` before they changed it (`null`: absent). Restoring
puts each back and deletes the file. A corrupt journal is set aside as `taskbar.json.bad`.

## Extending

The page and the host are built so new bars, flyouts and looks are additions, not rewrites.

- **Modules.** `bars/shell/src/top/modules.ts` maps every `Module` to a factory
  (`Record<Module, Factory>`), so a module added to `protocol.ts` (and `config::Module` in Rust)
  fails the build until it has one. Its data is a `Topic` in `crates/bars/src/bridge.rs`
  (`module_topic`): events flow only while a bar shows it. Popovers anchor with `barAnchor(el)`
  (`anchor.ts`), which opens toward the middle of the screen from any bar edge.
- **Dock styles.** `dock.style` picks a layout: `bars/shell/src/dock/styles.ts` maps each
  `DockStyle` to a page factory (both built-in styles share `dock.ts`); `DockConfig::band`
  (`crates/bars/src/config.rs`) gives its band. The window, app-bar, auto-hide, region, item and
  activation machinery is shared. The page sets `data-style` on `<html>`.
- **Presets.** A preset is `crates/bars/presets/<name>.json` (added to `Preset` and its `ALL` list;
  the tests check that every preset parses and never sets `taskbar`, `vars` or pins) and its
  section of `bars/shell/src/presets/presets.css`.
- **Flyouts.** `bars/shell/src/flyout.ts` is the one mechanism for anything that opens over the
  screen. `defineFlyout(id, render)` registers one; modules open it with `toggle(props, { anchor,
  place })`. One is open at a time; it closes on Escape, an outside press and window blur; it's part
  of the window region while open; its `scope` ends its subscriptions on close; it enters with the
  `open` animation and fades out. Menus (`menu.ts`: the power menu, the dock's context menu) are
  the `menu` flyout, previews the `preview` flyout. A flyout must fit in its bar's window: the band
  plus `ROOM` (360 DIPs, `crates/bars/src/bar.rs`; raise it, or make it per bar, for bigger ones).
- **Styling.** Everything is drawn from CSS variables and attributes, and the theme's CSS is
  inserted last (`<style id="theme-css">`, after the generated `<style id="look-css">`), so presets
  and themes restyle without code:
  - on `<html>`: `data-bar="top|dock"`, `data-preset`, `data-edge` and `data-position`
    (`top|bottom|left|right`), `data-layout` (top bar), `data-style`, `data-indicator`,
    `data-attention` (dock), `data-surface`, `data-text`, `data-shadow`, `data-scheme`,
    `data-system-scheme`, `data-fill` (`tint|none|solid|gradient|image|wallpaper`), `data-backdrop`
    (`on|off`), `data-corners`, `data-motion`, `data-m-show`, `data-m-open`, `data-perf`, and the
    classes `paused` (hidden) and `entering` (playing its show animation);
  - appearance variables: `--surface`, `--raised`, `--blur`, `--radius`, `--line`, `--edge`,
    `--t0`…`--t3`, `--fill-1`…`--fill-3`, `--accent`, `--font`, `--scale`, `--drop`, `--shade`;
  - look variables: surfaces `--s-fill`, `--s-fill-color`, `--s-fill-size`, `--s-fill-pos`,
    `--s-fill-repeat`, `--s-filter`, `--s-tint`, `--s-noise`, `--s-radius`, and per state
    (`""`, `-hover`, `-active`) `--s-bw`, `--s-bi`, `--s-glow`, `--s-shadow`; items `--i-radius` and
    per state `--i-bgc`, `--i-bgi`, `--i-fg`, `--i-bw`, `--i-bi`, `--i-glow`, `--i-scale`;
    indicators `--ind-running`, `--ind-active`, `--ind-attention`; `--system-accent*`; wallpaper
    glass `--backdrop`, `--backdrop-size`, `--backdrop-pos`; motion `--m-<leaf>`,
    `--m-<leaf>-ease`; the theme's own variables. A rule that retunes a look variable sets it on
    the elements that use it (`.surface`, `button.mod`), since `<html>` has them inline;
  - surfaces: `.surface` (with `.bg`, `::before` tint and grain, `::after` border ring): `.top`
    (strip and floating), `.island`, `.pill-surface`; every band carries `data-band`;
  - top bar: `.top[data-width]`, `.zone[data-zone="left|center|right"]`, `.sep`,
    `.mod[data-module="<name>"]` (one per module; e.g. `.ws-dot.is-current`, `.tray-item`,
    `.gauge`, `.clock`, `.search-label`, `.mod.custom[data-class][data-error]`);
  - dock: `.dock.floating|.taskbar`, `.pill`, `.tb-group`, `.tb-zone.tb-start|.tb-end`, `.items`,
    `.slot[data-state="none|running|active"][data-pinned][data-attention][data-name="<app>"]`
    (`.launching` while bouncing), `.tile`, `.icon`, `.pic`, `.count`, `.title`, `.dot`, `.divider`,
    `.extras`, `.slot.extra.desktop|.trash[data-full]`, `.dock-label`;
  - flyouts: `.layer > .flyout[data-flyout="<id>"]` (`--origin` is the side toward the anchor;
    `.leaving` while closing), `.menu`, `.menu-item`, `.menu-sep`, `.menu-header`, `.hud`,
    `.preview`, `.pv-card`, `.pv-head`, `.pv-title`, `.pv-close`, `.pv-thumb`.

  Glyphs are inline SVGs from one map (`bars/shell/src/icons.ts`), each tagged with its icon-pack key.

## Interaction with widgets

App-bar registration changes the work area. The widget host must re-lay out on `WM_SETTINGCHANGE`
with `SPI_SETWORKAREA` (and on display changes), so widgets never sit under a bar.

## How to run

```sh
pnpm -C bars/shell install
pnpm -C bars/shell build           # → bars/shell/dist
pnpm -C bars/shell test            # vitest: look, motion and springs, presets, icon packs, dock model and drag, magnify, layout, clock
cargo build -p mypaper-bars
target\debug\mypaper-bars.exe      # or let mypaper-core start it
```

- Without a WebView2 host, `pnpm -C bars/shell dev` (or the built page in a browser) runs against a
  mock host that resolves presets from the same bundles: `?bar=top|dock&preset=<name>&pos=bottom|top|left|right&autohide=1&scheme=light|dark&wall=dark|light|vivid&reduce=1&perf=1`,
  plus `poke=<selector>` (the pointer arrives there: magnify, labels, previews) and `dump=<selector>`
  (element boxes into `body[data-dump]`) for headless screenshots.
- To try it without touching your real setup, point `APPDATA` and `LOCALAPPDATA` at a temporary
  folder holding `mypaper\bars.json`.
