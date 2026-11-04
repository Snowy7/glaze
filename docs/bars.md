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
  `SHAppBarMessage`), veiled while hidden (a layered window style), and aligned (its documented
  registry value). No code goes into Explorer.
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
  - Measured with designs and glass (release build, two 1440p monitors, the macOS top bar and dock
    on each, the Aurora shader animating behind, 10 s per row): idle bars 0.36% CPU / 0.28% GPU 3D
    for WebView2, 0.02% CPU for the host; a liquid Control Center open 0.37% / 0.13% (its lens is
    drawn once, then kept); opening and closing it 12 times in 10 s 0.81% / 0.50%, the host 0.15%.
    DWM's own share (2.4–9.3% 3D) moves with the wallpaper, not with the glass. A liquid capture
    (the screen behind a pane) took 3.6–15 ms (a 576×109 dock, a 2560×38 menu bar), writing it
    under 1 ms.

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
    "design": "macos",
    "enabled": true,
    "monitors": "all",
    "layout": "strip",
    "position": "top",
    "height": 32,
    "left":   ["start", "workspaces", "app"],
    "center": ["clock"],
    "right":  ["media", "tray", "network", "volume", "battery", "quick", "power"],
    "clock":  { "seconds": false, "format": "ddd d MMM  HH:mm" },
    "look":   { "glass": { "style": "liquid", "refraction": 0.5 } }
  },
  "dock": {
    "design": "macos",
    "enabled": true,
    "monitors": "primary",
    "style": "floating",
    "position": "bottom",
    "size": 48,
    "autohide": false,
    "magnify": { "scale": 1.5, "range": 3 },
    "running": true,
    "indicator": "dot",
    "look": { "glass": { "highlight": 0.4 } },
    "animation": { "open": { "style": "slide" } },
    "pinned": [
      { "path": "C:\\Windows\\explorer.exe", "name": "File Explorer" },
      { "path": "shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", "name": "Terminal" }
    ]
  },
  "launcher": { "winKey": true, "layout": "win11", "files": true },
  "tray": { "host": false },
  "calendar": { "zones": [{ "zone": "America/New_York" }, { "zone": "Asia/Tokyo", "label": "Tokyo office" }] },
  "audio": { "switchOutput": false },
  "look": { "background": { "type": "wallpaper" }, "tint": 0.6 },
  "animation": { "curve": "smooth", "bar": { "style": "fade" } },
  "modules": { "clock": { "click": { "middle": { "exec": "notepad.exe" } } } },
  "appearance": { },
  "overrides": { "DISPLAY2": { "dock": { "position": "left" } } }
}
```

- **Every field is optional**; a missing one takes the bar's design's or the preset's value, else
  the default shown above, except `dock.enabled` (default `false`) and `dock.pinned` (below). `{}`
  is the same as no file.
- **Each bar is styled on its own.** `top.design` and `dock.design` pick each bar's design (see
  "Designs and presets"); `top.look` / `dock.look` and `top.animation` / `dock.animation` are each
  bar's own look and motion. The root `look` and `animation` are shared: they apply to both bars,
  under each bar's own. So any top bar goes with any dock: a macOS menu bar over a Windows 11
  taskbar is `{ "top": { "design": "macos" }, "dock": { "design": "windows11", "enabled": true } }`.
- **Layers**, lowest first (`presets::layered`): each bar's **design**, the **preset** (a pairing of
  designs), the file's shared `look` and `animation` (folded under each bar), the file's bar
  fields. Objects merge key by key, lists and values are replaced, and an object whose `type`
  differs from the one below (a background switching kinds) replaces it whole. Per-monitor
  **overrides** go over all that (see "Per-monitor overrides"). A theme adds `appearance`, CSS and
  variables (see "Look").
- **Files from before designs** need no change: their `preset` now names the designs it was made
  of, so `{ "preset": "islands", "dock": { "enabled": true } }` looks as it did.
- **`taskbar.mode`**: `keep` (untouched), `autohide`, or `hide` (hidden completely, on every
  monitor; its space goes to our bars).
  - Changing it takes effect live. Leaving `hide` or `autohide` restores the exact previous state from
    the journal.
  - `hide` also makes the taskbar auto-hide first, so the work area is freed even if Explorer re-shows
    it.
  - **In `hide` the taskbar is never seen, not even for a frame.** Explorer shows it again on some
    events (leaving a fullscreen app, auto-hide being reset, display and resolution changes,
    `TaskbarCreated`); a hide that comes after the show can't beat a frame of composition (measured
    with the unfixed build forcing a show: two bursts of 1 to 2 frames of taskbar pixels per show,
    about 70 ms apart). So the hidden windows also carry a **veil**: `WS_EX_LAYERED` at alpha 0, plus
    `WS_EX_TRANSPARENT` so they never take a click. A layered window at alpha 0 composes to nothing,
    whether or not Explorer has it shown, moved or re-clipped. Explorer never touches the extended
    style (measured on build 26300: it survived Start, Search, Quick Settings, Task View and fullscreen
    exits, and the same run with the veil showed 0 changed pixels where the unfixed one showed 2
    bursts). Other ways, tried: `DWMWA_CLOAK` is refused cross-process (`E_ACCESSDENIED`), and an empty
    window region is replaced by auto-hide's own. The veil goes on before the hide and comes off before
    the show, and is part of the journal's `hidden`: every restore removes it (only if it's ours: a
    taskbar window that someone else layered is left alone). Restoring is exact, and checked live (the
    windows come back with the extended style `0x88` they had).
  - The bars also watch the taskbar windows with out-of-context WinEvent hooks (`EVENT_OBJECT_CREATE`
    and `EVENT_OBJECT_SHOW`, and `EVENT_OBJECT_LOCATIONCHANGE` for an auto-hidden taskbar sliding in),
    veil and hide them again as the event arrives (a taskbar Explorer builds anew, for a monitor that
    came back, is born visible: that one can show for the length of the hook's delay). They never give
    up; re-hides are only spaced 50 ms apart, so a fight with Explorer stays cheap. Each re-hide also
    checks that auto-hide is still set, and sets it again if not.
  - **Finding the taskbar windows.** `FindWindowEx` by class (`Shell_TrayWnd`,
    `Shell_SecondaryTrayWnd`, owned by `explorer.exe`), never `EnumWindows`: while Start, Search,
    Quick Settings or Task View is open, Explorer moves the main taskbar into z-band 6
    (`GetWindowBand`; it's 1 otherwise), and `EnumWindows` doesn't list windows in higher bands.
    A taskbar found that way could not be hidden then, nor restored afterwards, and it was found
    that way before this fix (the primary taskbar was left sitting on screen over the dock).
  - Presets never set `taskbar`: replacing the native taskbar is always the user's choice. The
    `windows11` preset with `"taskbar": { "mode": "hide" }` is a complete taskbar replacement.
- **`taskbar.alignment`**: `center` or `left` (the `TaskbarAl` value Settings writes, announced with
  `WM_SETTINGCHANGE` `"TraySettings"`). Omit it to leave Windows' setting alone; removing it
  restores the original value.
- **`top` / `dock` `monitors`**: `all`, `primary`, or a list of device names (`\\.\DISPLAY2` or
  `DISPLAY2`, any case).
- **`top.clock.format`** tokens: `dddd` `ddd` `d` `dd` `MMMM` `MMM` `MM` `M` `yyyy` `yy` `HH` `H`
  `hh` `h` `mm` `ss` `tt`; `'quoted text'` and anything else is literal (a `\n` makes two lines).
  Default `ddd d MMM  HH:mm`, or `ddd d MMM  HH:mm:ss` with `seconds`. At most 64 characters. A
  `taskbar` dock's clock has its own, `dock.clock`. With no explicit `dock.clock.format`, it shows
  Windows' short time and short date on two lines, including Regional format overrides and the
  user's 12/24-hour choice. `seconds` uses Windows' time format with seconds. The host reads the
  NLS patterns and formats them in Windows; `clock-format.ts` still renders explicit token formats.
- **`dock.button`** (16–240): a `taskbar` task button's length along the bar (missing: the icon plus
  16, or 168 with `titles`; Windows 11 is 40, Windows 10 48).
- **`dock.pinned[].path`** is an exe, a shortcut (`.lnk`), a folder, or an `shell:AppsFolder\<AUMID>`
  path (Store apps). **Missing `pinned`** means the native taskbar's pinned shortcuts
  (`%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\*.lnk`, in name order;
  Store apps pinned there aren't shortcuts and don't come along), or File Explorer when there are
  none. An empty list is a choice and stays empty.
- **Module lists** (`top.left|center|right`, `dock.left|right`) hold built-in modules,
  `custom:<id>` and `|` (a break: a new island in the `islands` layout, a thin separator elsewhere).
  Each module is listed at most once per bar.
- **`launcher`** (see "Flyouts" → "Launcher"):
  - `winKey` (default `true`): a lone Windows key opens the launcher, and Win+N and Win+A open the
    notifications and quick settings flyouts, only while `taskbar.mode` is `hide`.
  - `layout`: `win11` (default), `grid`, `compact` or `dashboard`.
  - `pinned`: the launcher's pins, in the shape of `dock.pinned`; missing means the dock's pins.
  - `files` (default `true`): search recent files too.
- **`tray.host`** (default `false`): run the tray host even with the native taskbar kept. With
  `taskbar.mode: hide` it always runs.
- **`calendar`**: `zones` (at most 4) are extra clocks: `{ "zone": "<IANA name>", "label"? }`.
  `firstDay` is `monday`, `sunday` or `saturday`; missing means the locale's.
- **`audio.switchOutput`** (default `false`): lets the mixer switch the default output device. This
  uses `IPolicyConfig`, an **undocumented** COM interface (the one Sound settings uses). Without it,
  the mixer lists the devices and links to Sound settings.
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

## Designs and presets

Each bar has a **design**: its whole character, as a partial `top` (or `dock`) with its own look
and motion (`crates/bars/presets/top/<id>.json`, `presets/dock/<id>.json`), plus CSS the page
bundles (`bars/shell/src/designs/`, scoped by `data-design`). Metrics follow the system each one
is after. Designs never decide whether a bar shows, where, or its pins.

| `top.design` | What it is |
|---|---|
| `monochrome` (default) | a black wallpaper-glass strip, 32, white type |
| `macos` | the macOS (Tahoe) menu bar: 24, no surface, white type on the wallpaper, the app's name in bold, SF-style monochrome status icons, the time on the right; its flyouts are the macOS family in liquid glass |
| `fluent` | a Windows 11 bar: 32, Mica-like wallpaper glass, Segoe UI Variable 12, Segoe Fluent Icons, 4 DIP subtle-fill buttons, network/volume/battery as one button opening quick settings |
| `capsule` | a floating liquid-glass pill hugging its modules, 36, iPadOS/visionOS style; its flyouts morph out of their button |
| `minimal` | no surface: the clock in the middle, workspaces and a few status icons, a soft text shadow |
| `islands` | a transparent rail with one solid pill per group |
| `neon` | floating dark glass with a magenta-to-cyan edge and glow |
| `floating` | a detached, rounded bar |

| `dock.design` | What it is |
|---|---|
| `monochrome` (default) | a black glass pill with dots |
| `macos` | the macOS dock: a clear liquid-glass plate with a thin rim, 20 corners, 48 icons, magnify 1.65, 3.5 DIP running dots, the trash past a divider |
| `windows11` | the Windows 11 taskbar: 48, 40×40 buttons with 4 DIP corners, centered with Start and a pill search box, 6×3 running pills (16 and accent in front), press squeezes the icon, network/volume/battery as one button, two-line clock, bell, show-desktop sliver |
| `windows10` | the Windows 10 taskbar: 40, left-aligned, 48 DIP square buttons, underline indicators, the light "Type here to search" field, two-line clock |
| `chromeos` | the ChromeOS shelf: 48, a launcher circle, centered apps with dots, the status area (time, network, volume, battery) as one pill |
| `fluent` | the Windows 11 taskbar as a floating, rounded acrylic island hugging its contents |
| `islands` | a solid pill with bar indicators |
| `neon` | dark glass, glow indicators |
| `floating` | a detached, rounded pill |

A **preset** is a pairing (`crates/bars/presets/<name>.json`): which design each bar starts from,
and whether each bar is on. `preset` picks it; a bar's own `design` beats it.

| `preset` | Top bar | Dock |
|---|---|---|
| `monochrome` (default) | `monochrome` | `monochrome` |
| `macos` | `macos` | `macos` |
| `windows11` | off (`fluent` when turned on) | `windows11`, on, every monitor |
| `windows10` | off (`fluent`) | `windows10`, on, every monitor |
| `chromeos` | off (`minimal`) | `chromeos`, on, every monitor |
| `islands` | `islands` | `islands` |
| `neon-glass` | `neon` | `neon` |
| `floating` | `floating` | `floating` |

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
- **`family`**: what the flyouts and menus this bar opens are drawn after: `mono` (mypaper's own,
  default), `fluent` (Windows 11: acrylic, 8 DIP corners, accent toggles, quick-settings tiles with
  their label under them, Start's footer band), `macos` (Control Center modules with round glyph
  badges, 22 pt sliders, accent menu rows, a red today) or `material` (ChromeOS: 20 DIP bubbles, pill
  tiles, thick sliders). `data-family` on `<html>`.
- **`glyphs`**: the module glyph set: `default` (mypaper's), `fluent` (the Segoe Fluent Icons font,
  pixel-exact on Windows 11; Segoe MDL2 Assets on 10) or `symbols` (SF Symbols-style outlines). An
  icon pack's keys win over the set's.
- **`font`**: `{ family (a CSS font-family list), size (9–24), weight (100–900) }`: the bar's type.
- **`glass`**: the material: `style` `liquid` (see "Liquid glass"), `frosted` (wallpaper glass or
  live blur, by `background`) or `none`, with the widgets' knob names, each 0–1: `intensity`
  (scales the whole effect), `refraction` (how far the rim bends), `highlight` (the specular edge),
  `tint` (the tint over liquid glass; missing: 0.06), `blur` (DIPs 0–32, light), and `sheen` and
  `depth` (a soft top light and an inner shadow; off unless a design asks). A string is the style
  alone: `"glass": "liquid"`. Missing `style`: `liquid` when the theme's appearance style is
  `liquid` (the widgets' Liquid Glass), else `frosted`, so one theme switch makes the whole desktop
  liquid. The macOS designs default to liquid; the Windows designs to frosted.
  The theme's `bars.glass` knobs (`intensity`, `refraction`, `highlight`, shared with the
  widgets) fill in what a bar's `look.glass` leaves out.
- **`flyouts`**: `{ blur, tint }`: whether this bar's flyouts are glass (default on; off: solid)
  and their tint's opacity (missing: the family's).
- **`background: { "type": "blur", "brightness", "blur" }`** is live blur: Windows' own blur of
  whatever is behind (see "Live blur"). An auto-hiding dock with wallpaper glass draws live blur
  instead whenever it can, since it slides in over windows.

The page sets the appearance and the look as CSS variables and `data-*` attributes on `<html>`
(see "Styling"), plus `data-bar="top|dock"`, `data-edge`, `data-preset`, `data-design`, and inserts its generated
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

### Live blur

A WebView can't see behind its window, so `backdrop-filter` only blurs the page's own pixels.
Windows can (`crates/bars/src/blur.rs`):

- Each bar window opts into host backdrop brushes (`DWMWA_USE_HOSTBACKDROPBRUSH`, Windows 11) and
  gets a `Windows.UI.Composition` target *under* its child windows, so under the transparent
  WebView. The page reports its glass as panes (`glass`: rounded rects with an opacity and a
  material, `bars/shell/src/glass.ts`): every element whose computed `--glass` is 1 (surfaces with
  a `blur` fill, and the flyouts while `data-fly-blur` is on). It sends them whenever they change,
  and every frame while an animation or transition moves one (a flyout popping in, the dock's pill
  stretching), so the blur moves with them.
- Each pane is a sprite of the host backdrop (DWM's own acrylic blur), through an optional extra
  Gaussian blur and a color matrix (saturation, brightness), clipped to an anti-aliased rounded
  rect (a squircle's radius is scaled to the circle that hugs it). The page draws the tint, rim and
  grain over it, so there are no grey or black fringes. It renders while the window is inactive.
- It's on (`mode.blur`) when the window got a target, transparency effects are on (Settings →
  Personalization → Colors, `EnableTransparency`, followed live), and performance mode is off.
  Otherwise the page uses an opaque tint. Nothing polls: DWM redraws a sprite only when what's
  behind it changes. `examples/glass-spike.rs` is the spike that proved it.

### Liquid glass

Liquid Glass (macOS Tahoe, iOS 26) is clear: what's behind bends along a thick rim, the middle is
nearly clear, and one thin specular edge catches the light. That needs the pixels behind, and a page
has none, so the host captures them (`crates/bars/src/capture.rs`) and the page lenses them
(`bars/shell/src/liquid.ts`):

- **Capture.** The page asks (`capture { rect }`). The host copies that part of the screen with GDI
  (`BitBlt` from the screen DC) while every bar window is excluded from capture
  (`SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`, documented; it takes effect at once). The
  copy is the wallpaper's current frame over the desktop, or the windows behind a flyout. It's
  written as a 24-bit BMP to `%LOCALAPPDATA%\mypaper\backdrop\behind\` (two per bar, emptied at
  start) and served from `backdrop.bars.mypaper`. There's no capture session and no border. While a
  bar is excluded (a few milliseconds), a screen recording doesn't show it; `examples/capture-spike.rs`
  measured the copy and the exclusion.
- **When.** A pane is captured when it appears, when it moves or grows past its capture, and when
  the host says what's behind changed (`behind`: the bar's window moved, or the wallpaper's
  snapshot did; liquid bars ask the engine for snapshots every 10 s, like wallpaper glass). A
  flyout is captured as it opens; until then it shows live blur.
- **Lens.** Each pane (an element whose computed `--liquid` is 1) gets a `.lens` layer showing the
  capture aligned with the screen, through an SVG filter: a displacement map drawn from the pane's
  rounded-rect distance field (outward at the rim, fading across a convex bezel to none in the
  middle), applied per color channel at slightly different strengths (chromatic fringing where it
  bends), after a light blur. Edges flush with the screen, or closer to it than the lens reaches,
  get no rim. A `.shine` layer draws the one thin specular edge, brighter on hover and press.
- **Cost.** A map is drawn when a pane's size changes, a capture made as above; in between nothing
  runs, and Chromium keeps the filtered layer as it is (see "Principles" for the measurements).
- **Flyouts** are frosted Liquid Glass (Apple's regular variant: a stronger lens, a 12 DIP blur, the
  family's tint), so text stays legible over anything.
- It's on (`mode.liquid`) when transparency effects are on and performance mode is off, for
  surfaces over a fill it can stand in for (`wallpaper`, `blur`, or the plain tint); a solid,
  gradient or picture fill stays as it is.

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
- **Per bar.** `top.animation` and `dock.animation` are whole trees of this shape, layered like
  looks: the bar's design's, the shared `animation`, then the bar's own.
- **Fields**: `enabled`, `duration` (ms, 0–5000), `curve`, `style` (`slide`, `fade`, `pop`, `morph` (flyouts: grow out of the button),
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
| `start` | a mypaper mark | opens the launcher (see "Flyouts"); the `native-start` action opens the native Start menu |
| `search` | a search glyph (a search box in `windows11`) | Windows Search (Win+S, on the user's click) |
| `workspaces` | virtual desktops: count and current (`VirtualDesktopIDs` / `CurrentVirtualDesktop` under `HKCU\…\Explorer\VirtualDesktops`, watched with `RegNotifyChangeKeyValue`; absent values mean one desktop) | switch desktop: Win+Ctrl+arrow, sent only on the user's click (once per step) |
| `app` | the foreground app's name and icon (the last app window in front: clicking a bar doesn't change it) | — |
| `clock` | time and date (format in `bars.json` → `top.clock`, or `dock.clock` on a taskbar) | opens the notification center, with the month under it (as Windows 11's clock and macOS's date do); the `calendar` action opens the calendar |
| `media` | now playing (SMTC): title and artist | play/pause; scroll for next and previous |
| `tray` | the pinned notification-area icons, then a chevron (see below) | clicks go to the icon's app; the chevron opens the overflow grid |
| `network` | wifi signal and name (WLAN API) / ethernet (the interface table: an up, physical, wired interface) / offline; changes from `NotifyIpInterfaceChange` and WLAN notifications | opens the network flyout |
| `volume` | level and mute (Core Audio `IAudioEndpointVolume` on the default output, with its change callback) | opens the mixer; middle click mutes; scroll to change (raising it unmutes) |
| `battery` | percent and charging (`GetSystemPowerStatus`, refreshed on `PBT_APMPOWERSTATUSCHANGE`); hidden on desktops | power settings |
| `quick` | a settings glyph | opens quick settings |
| `power` | a power glyph | menu: lock, sleep, sign out, restart, shut down (restart and shut down ask first) |
| `stats` | CPU / RAM mini gauges (sampled every 2 s) | Task Manager |
| `language` | the keyboard layout of the app in front (`ENG` over `US`), followed on foreground changes and the shell's `HSHELL_LANGUAGE` | opens the layout list |
| `notifications` | a bell with the number of notifications (re-counted every 15 s while shown: change events need package identity) | opens the notifications flyout |
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
  `calendar`, `network`, `mixer`, `native-start`, `native-notifications`, `native-quick-settings`,
  `task-manager`, `settings`, `show-desktop`, `media-toggle`, `media-next`, `media-previous`,
  `volume-up`, `volume-down`, `volume-mute`, `workspace-next`, `workspace-previous`, `lock`,
  `explorer`), to `{ "exec": program, "args": [...] }` (run as the user, detached) or to
  `{ "open": path-or-URI }`. Unbound inputs keep the module's behavior. The page binds them in the
  capture phase on the module, so only the bound inputs are taken over; wheel notches closer than
  120 ms count once.
  **Flyout actions and native actions.** The actions that open a panel open ours by default; each
  module's own click is the same action (`start` → launcher, `clock` → calendar, `network` →
  network, `volume` → mixer, `quick` → quick settings, `notifications` → notifications), and the
  flyout opens from the module the action is bound on:

  | Action | Opens | If the flyout can't open, the host logs it and opens |
  |---|---|---|
  | `start` | the launcher (the one the Windows key opens: from the `start` module if the bar has one, else from the bound module; pressing again closes it) | the native Start menu |
  | `notifications` | the notifications flyout | the notification center (`ms-actioncenter:`) |
  | `quick-settings` | quick settings | the control center (`ms-actioncenter:controlcenter/`) |
  | `calendar` | the calendar | the notification center |
  | `network` | the network flyout | `ms-settings:network` |
  | `mixer` | the volume mixer | `ms-settings:sound` |
  | `native-start` | Windows' Start menu | |
  | `native-notifications` | Windows' notification center (`ms-actioncenter:`) | |
  | `native-quick-settings` | Windows' control center (`ms-actioncenter:controlcenter/`) | |

  Use a `native-*` action to keep Windows' own panel on a module, e.g.
  `"quick": { "click": { "left": "native-quick-settings" } }`.
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

**The tray** comes from the tray host (see "Flyouts" → "Tray host") while the native taskbar is
replaced (`taskbar.mode: hide`, or `tray.host`): the real icons, pinned ones in the bar and the rest
behind the chevron, as the user arranged them in Windows. Otherwise, with Explorer's taskbar kept,
the module reads Explorer's tray through UI Automation (a supported accessibility API), on a worker
thread, refreshed on UIA structure-changed events. What Windows 11 (build 26300, "26H2") exposes to
UI Automation:

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
- A hidden taskbar has no tray in the UIA tree (Explorer removes its whole XAML content; measured:
  no `SystemTray.*` element at all), which is why `hide` uses the tray host instead.
- If UI Automation is unavailable or finds no tray, the module shows a single `overflow` button that
  tries the native overflow flyout (Win+B, Enter). The fallback is logged.

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

## Flyouts

What the top-bar modules open: our own Start menu (the launcher), quick settings, network, the
mixer, keyboard layouts, the calendar, notifications, the tray overflow and the power menu. They
replace the taskbar's own menus; nothing goes into Explorer.

- **The page side** (`bars/shell/src/flyouts`) builds each on `flyout.ts`. Each big one is its own
  chunk, loaded the first time it opens; its DOM is built on open and dropped on close.
  `surface.ts` places it: under the top bar's strip, or off the dock toward the middle of the
  screen.
- **Room.** A flyout taller than the bar's `ROOM` asks the host for more first (`room`): the host
  grows the bar's window toward the middle of the screen (clipped to the monitor) and the page waits
  for the resize. The window goes back to its usual size 450 ms after the last flyout closed. The
  region keeps everything outside the flyout click-through.
- **Keyboard.** The launcher asks for the keyboard (`keyboard`): the bar becomes the foreground
  window and the WebView takes focus, and the host stops handing the foreground back while it's
  open. On close the page says whether to hand it back (`keyboard { on: false, handBack }`):
  - closed by a key (Escape, Enter on a result), with no pointer press in the last 500 ms: the
    host gives the foreground back to the app before it, unless an app already took it;
  - closed by a press (another module, whose flyout then opens, or a tile): the press decides
    what's in front.
- **Providers** (`crates/bars/src/flyouts`) run on lanes, one thread each (COM in the apartment the
  API wants). A lane starts on the first request and stops after 60 s unused; while something it
  feeds is watched it counts as used. Slow requests (DDC/CI, WLAN scans, WinRT reads) answer *later* through the main
  loop, which never blocks on them. **Nothing polls while every flyout is closed**, except the
  notification badge while a `notifications` module is on screen (below).
- **Watching.** An open flyout `watch`es what it shows live: its provider's events (`mixer`,
  `wifi`, ...) or a core topic (`media`, `stats`, ...). A core topic then runs as if a module showed
  it, for that bar only. A bar's page reload drops its watches, its room and its keyboard, and the
  answers still due to the old page (each deferred reply carries the page load that asked).
- **Styling.** Every flyout's root is `.layer > .flyout[data-flyout="<name>"]` (`launcher`, `quick`,
  `network`, `mixer`, `language`, `calendar`, `notifications`, `tray`, `menu`). The shared pieces
  are `.fly-panel`, `.fly-header`, `.fly-row`, `.fly-tile[data-on]`, `.fly-slider`, `.fly-switch`,
  `.fly-list`, `.fly-notice` and `.fly-footer`. Each flyout's parts are prefixed with its name
  (`.launcher-tile`, `.qs-tiles`, `.net-item`, `.mix-app`, `.cal-day.is-today`, `.notif-card`, `.notif-stack`,
  `.tray-cell`, `.lang-badge`). They're drawn from the appearance tokens like the bars, and
  monochrome by default: "on" is the text color as a fill. Durations and curves are `--fly-duration`
  and `--ease`, so presets retime them. The flyouts' glyphs are in `flyouts/glyphs.ts`.

### Launcher

The `start` module (and the Windows key, below) opens the launcher.

- **Index.** Every app the Start menu lists, from `shell:AppsFolder`
  (`SHGetKnownFolderItem(FOLDERID_AppsFolder)` enumerated with `BHID_EnumItems`):
  - packaged apps by AUMID, and the Start Menu folders' shortcuts (user and common);
  - names from the shell; icons from `IShellItemImageFactory` (64 px, in the icon cache);
  - each shortcut's target exe as a search keyword (`msedge` finds Microsoft Edge);
  - uninstallers, readmes and web links left out.

  It's built on its lane at background priority, 8 s after start (or at the first `apps` request).
  It's cached in `%LOCALAPPDATA%\mypaper\apps.json`, and rebuilt when the Start Menu folders change
  (a file watch, settled 3 s) or when the launcher opens more than 10 minutes after the last build
  (packaged installs don't touch those folders).
- **Search** (`flyouts/launcher/fuzzy.ts`) covers apps, about 70 Windows Settings pages
  (`settings-pages.ts`, `ms-settings:` URIs with keywords) and, with `launcher.files`, the recent
  files (`%APPDATA%\Microsoft\Windows\Recent`, read when the launcher opens).
  - Ranking: exact > prefix > a word's prefix > initials (`vsc`) > substring > in-order letters
    (only from 3 typed). Apps come before settings, and settings before files.
  - What the user runs often and lately goes first (`launcher.json`: each run's count and time).
  - Results show as a best match, then apps (8), settings (5) and files (5).
- **Keyboard first.** Typing goes to the search box from anywhere, the arrows walk the list (and
  the pinned grid in two dimensions), Enter opens, Escape closes.
- **Layouts** (`launcher.layout`):

  | Layout | Shows |
  |---|---|
  | `win11` | search, pinned (6 columns, "All apps" A–Z), recommended (recent files, else recent apps) |
  | `grid` | a big grid of every app (Launchpad) |
  | `compact` | a search bar, recent apps until you type (Raycast, Flow) |
  | `dashboard` | search, pinned, a widgets row (time and date, now playing with controls, CPU / memory / battery) and the clipboard history |

  Every layout but `compact` has a footer: the user (`GetUserNameExW(NameDisplay)`), Settings, and
  power (lock, sleep, sign out, restart, shut down; the last two ask first).
- **Pins.** Right-click an app to pin it to the launcher (`launcher.pinned`, which starts as the
  dock's pins), unpin it, or pin it to the dock.
- **Clipboard history** is Windows' own (Win+V, `Clipboard.GetHistoryItemsAsync`). It answers an
  unpackaged process on build 26300 (`examples/winrt-spike.rs`), so there's no history of our own.
  Picking an entry makes it the clipboard's content. With history off, the launcher links to its
  setting.

**The Windows key.** With `launcher.winKey` and `taskbar.mode: hide`, a low-level keyboard hook
(`WH_KEYBOARD_LL`, `flyouts/winkey.rs`) runs in our process, on a thread of its own; nothing is
loaded anywhere else.
- It lets every key through. The one exception is the release of a **lone** press of the Windows
  key: the hook replays it after the unassigned key `vkE8` (AutoHotkey's "menu mask key"), with
  its own scan code and extended flag, and swallows the real one only once `SendInput` took all
  three. Explorer then sees a combination and keeps Start closed, and the launcher opens.
- **Win+N and Win+A** open the notifications and quick settings flyouts instead of Windows'
  panels (same `launcher.winKey` switch, same conditions). When N or A is the first key after the
  Windows key and no Shift, Ctrl or Alt is down, the hook injects the mask key, swallows the
  letter (its press, repeats and release), and opens the flyout on the Windows key's release,
  replayed like a lone press's. Explorer sees Win, `vkE8`, Win up: a combination, so neither Start
  nor its own panel opens. (Opening at the letter was tried first: with the user's own Win key
  still down, Windows refused the bar the foreground, and the flyout closed at once on blur.)
  The flyout opens on a bar with its own module (`notifications`, `quick`), else the last module
  of the bar (the right end of a top bar); pressing the shortcut again, or Escape, closes it, and
  the app that was in front gets the foreground back. Without a bar to show it, the native panel
  opens (`ms-actioncenter:`).
- Other combinations (Win+E, Win+Shift+S, Win+Shift+N, ...) pass untouched.
- The release passes and the native Start opens when:
  - a fullscreen app is in front (`SHQueryUserNotificationState`, or a window covering its
    monitor; the main loop keeps it in an atomic the hook reads);
  - no bar can show the launcher;
  - the main loop is hung (`IsHungAppWindow` on its window), since it's what opens the launcher;
  - the replay didn't go in (the key must never stay down).
- A press the main loop gets to more than 500 ms late is dropped rather than opened late.
- Locking or unlocking the session starts the hook's state over (a press it saw go down may have
  come up on the secure desktop).
- The keys the bars inject themselves carry `dwExtraInfo` `OURS`, and the hook ignores them. Other
  injected input counts like a keyboard's (on-screen keyboards, remote desktop).
- The launcher opens on the monitor of the app in front, on a bar there with the `start` module
  (else its top bar). The host makes the bar the foreground window: our replayed input is the last
  input, so Windows allows it. The page opens it with the same function as the `start` action
  (`toggleLauncher`, `flyouts/index.ts`), so both open the same launcher, from the `start` module
  when the bar has one; if it can't open, the page falls back to the native Start menu like any
  flyout (see "Modules" → "Flyout actions and native actions").

### Quick settings

- **Wi-Fi and Bluetooth** on/off: `Windows.Devices.Radios` (`RequestAccessAsync` was `Allowed` for
  the unpackaged process). The Wi-Fi tile's chevron opens the network flyout.
- **Power mode** cycles efficiency → balanced → performance: the Settings slider's overlay schemes,
  `PowerGetEffectiveOverlayScheme` / `PowerSetActiveOverlayScheme` from powrprof.dll (not in the
  SDK headers).
- **Battery saver** (on machines with a battery), **do not disturb**
  (`ToastNotificationManager.GetDefault().NotificationMode`) and **focus**
  (`FocusSessionManager.IsFocusActive`) show their state. Clicking opens their Settings page:
  changing them has no documented API. **Night light** opens its page for the same reason.
- **Brightness:** the built-in panel through WMI (`WmiMonitorBrightness`,
  `WmiMonitorBrightnessMethods.WmiSetBrightness`), else external monitors through DDC/CI
  (`GetMonitorBrightness` / `SetMonitorBrightness`; one slider for all of them). The slider is
  hidden without either.
- **Volume** with mute and a chevron to the mixer, the battery level, and a link to Settings.

### Network

- **Wi-Fi:** the radio's switch and the networks in range (`WlanGetAvailableNetworkList`).
  - One entry per SSID: the connected one, else the saved one, else the strongest. Each shows its
    signal and security.
  - It scans on open (`WlanScan`); the WLAN scan-complete notification updates the list.
- **Connecting:**
  - A saved network connects with its profile (`WlanConnect`).
  - A new WPA/WPA2/WPA3-Personal or open network gets a profile written here
    (`wifi::profile_xml`, the schema `netsh wlan export profile` writes; the SSID also in hex),
    saved with `WlanSetProfile`, then connected.
  - Enterprise and WEP networks link to Wi-Fi settings.
  - The connected network offers Disconnect.
- **Location consent (24H2 and later):** desktop apps see Wi-Fi networks only with location access
  on. `WlanGetAvailableNetworkList` returns `ERROR_ACCESS_DENIED`, and the flyout explains this and
  links to `ms-settings:privacy-location`.
- **Ethernet** shows when wired (the `network` topic). **VPN** links to `ms-settings:network-vpn`.

### Mixer

- **Output:** the device in use and the master volume and mute (the `volume` topic).
- **Devices** (`EnumAudioEndpoints`, friendly names). They're switchable only with
  `audio.switchOutput` (`IPolicyConfig::SetDefaultEndpoint` for every role, an undocumented
  interface); otherwise there's a link to Sound settings.
- **Apps:** every app playing on the default output (`IAudioSessionManager2` sessions; expired
  ones left out), system sounds first, then playing apps.
  - Each has its own slider and mute (`ISimpleAudioVolume`).
  - Names come from the session's display name (indirect strings resolved), else the exe's
    `FileDescription`. Icons come from the exe.
- **Live:** new sessions (`IAudioSessionNotification`), per-session volume and state changes
  (`IAudioSessionEvents`) and default-device changes (`IMMNotificationClient`) refresh the open
  mixer. Rows are kept across refreshes, so a slider being dragged isn't rebuilt under the pointer.

### Language

- The installed layouts come from `GetKeyboardLayoutList`. Each is named by its locale
  (`GetLocaleInfoEx`: "English (United States)", `ENG`, `US`) and its layout's registry name (the
  KLID from the HKL's high word: a substitute layout, an IME, or an `Fxxx` index mapped through
  `Layout Id`; `Layout Display Name` resolved).
- Picking one posts `WM_INPUTLANGCHANGEREQUEST` to the app in front, the documented request (its
  default window procedure activates the layout). Console and some UWP windows ignore it.

### Calendar

The `calendar` action opens it; the clock opens the notification center, which has the month too.

Today (weekday, date, time), a month grid you page through (buttons, the wheel, "Today"), and the
extra clocks of `calendar.zones` (local time, day, offset from here). Everything comes from Intl,
and the first weekday is the locale's unless `calendar.firstDay` says otherwise. "Calendar" opens
`outlookcal:`.

### Notifications

The open spike (research 4.2, 7), measured with `cargo run -p mypaper-bars --example winrt-spike`
on build 26300, unpackaged:
- `UserNotificationListener.GetAccessStatus()` and `RequestAccessAsync()` both return `Allowed`.
- `GetNotificationsAsync(Toast)` returns every toast (52 that day), with app names.
- `NotificationChanged` refuses to register: `0x80070490` ("Element not found"), needing package
  identity. So the flyout re-reads the list every 4 s while it's open, and the `notifications`
  module's badge is re-counted every 15 s while the module is on screen (a periodic topic, which
  stops for fullscreen apps).
- Removing one toast (`RemoveNotification`) works unpackaged; `ClearNotifications` may not, so
  "Clear all" falls back to removing them one by one.

No sparse package was needed, so none is shipped. Each toast includes its display name (`app`),
AppUserModelID (`aumid`) and, when packaged, `packageFamilyName`. The flyout groups by identity
(falling back to the name when no identity is available), newest first
(the app's logo from `AppDisplayInfo.GetLogo`, the title and body from the toast's text elements),
with dismiss, clear all and a link to the notification center. With access turned off in Settings
it says so and links to `ms-settings:privacy-notifications`.

**Where it opens from.** The `notifications` module (a bell with the count), the `notifications`
action, and Win+N (see "The Windows key"). The default top bar has **no** `notifications` module
(its right side is media, tray, network, volume, battery, quick, power), so before Win+N opened the
flyout there was no way to reach it at all. Checked live on build 26300 (unpackaged `mypaper-bars`):
Win+N opened it under the top bar and listed the machine's real toasts (57 at the time), grouped by
app, with Clear all. The first open is the slow one (a lane starts, then one logo is read per app),
so the panel shows "Loading…" for a moment.

**The view** (`bars/shell/src/flyouts/notification-center.ts`): toasts grouped by app, newest
group first; a group of several is a stack (the newest card, the rest peeking under it) until it's
opened. Each card has its app's icon: `AppInfo.DisplayInfo.GetLogo`, else the Start Menu shortcut
whose `System.AppUserModel.ID` matches the AUMID (user before common), else its AppsFolder item.
Shortcut/exe images use the dock's shell image factory and PNG cache at `icons.bars.mypaper`.
A true miss keeps the monogram; display-name prefixes never select another app's icon. A swipe sideways or the ×
that shows on hover dismisses one; a group's × clears the group; "Clear all" clears everything. The
header shows Do Not Disturb (it opens its setting: changing it has no documented API). The month
sits under the list, as on Windows 11. Scrollbars are thin and in the text color in every flyout.

### Tray host

How the `tray` module gets the real notification-area icons without injection, the ManagedShell /
RetroBar way (research 2.5 and 4.1; `crates/bars/src/tray_host.rs`, wire formats in
`tray_data.rs`).

**When it runs.** While some bar is configured with a tray module **and** the native taskbar is
replaced (`taskbar.mode: hide`, or `tray.host: true`), on its own thread. That's configuration,
not what's on screen: a fullscreen game hiding the bars doesn't stop it, since every start and
stop makes every app re-add its icons. While Explorer's tray isn't there it waits (looking every
3 s). Then:
1. It creates a hidden, topmost window of class `Shell_TrayWnd` (with a `TrayNotifyWnd` child),
   sized like the band of the bar that shows the tray (on screen and on the primary monitor
   first; a dock's band and edge too). It's never shown: a show is taken out of
   `WM_WINDOWPOSCHANGING`, and `WM_CLOSE` is ignored, as Explorer's tray does.
   - `Shell_NotifyIcon` and `SHAppBarMessage` find the first `Shell_TrayWnd` in Z-order, so the host
     keeps its window above Explorer's.
   - It re-checks on Explorer's own window events (out-of-context WinEvents filtered to Explorer's
     process), and with one `FindWindow` every 3 s. Measured: hiding the taskbar makes Explorer
     raise its tray a dozen times in the first second, and the events catch each one.
2. It broadcasts `TaskbarCreated`, so every running app adds its icons again, now to the host. That
   is the initial enumeration; the bars ignore their own broadcast.
3. It keeps each icon: owner window, uID or GUID, callback message, `NOTIFYICON_VERSION`, tooltip,
   hidden state, and a **copy** of the `HICON` (apps destroy theirs right after the call) as a PNG
   `data:` URL. A balloon's `hBalloonIcon` is copied too, for Explorer. Each message is answered
   at once (`ReplyMessage`). The bars get the list once a burst settles, at most once a second
   (animated icons change many times a second); app names (`FileDescription`) are read on a
   helper thread, the file name standing in until then.
4. **Everything is passed on to Explorer's real tray**, so Explorer's state stays correct:
   - balloons still become toasts;
   - `NotifyIconSettings` keeps learning about new icons;
   - nothing is lost when the host goes away.

   Icon messages are queued and forwarded one at a time (with the host's copies of the icons). App-bar
   messages are forwarded synchronously, with `ABM_QUERYPOS`/`ABM_SETPOS` re-homed: `SHAppBarMessage`
   shares its data with the receiving process, so the host copies it into a block shared with
   Explorer and copies the answer back. A forward Explorer didn't take in time keeps what it points
   at (the icon copies, the shared block): Explorer may still read it later, and a small leak beats
   a read of freed memory. The target is the primary `Shell_TrayWnd` owned by the desktop shell's
   Explorer process, independent of Z-order; its class and owner are rechecked before forwarding,
   and this process is never a target. `ABM_GETAUTOHIDEBAR` / `ABM_GETAUTOHIDEBAREX` read a snapshot
   of Explorer's auto-hide owners, for all four edges of each monitor. Chromium repeats those
   lookups from app-bar callbacks; forwarding them synchronously nests the same lookup indefinitely.
   The snapshot is captured before hosting and refreshed off the window-procedure stack after
   state/registration changes, geometry changes or a new lookup (and when a lookup is over 3 s old).
   The depth guard remains as a safety net. `ABM_GETTASKBARPOS` is answered with the bar's rect, so
   apps open their popups near our tray. `WM_COMMAND`, `SC_TASKLIST` and the `WM_USER` range go to
   Explorer as they came (sent or posted). Registered messages don't: they're broadcasts Explorer
   gets itself.

**Clicks** go to the icon's owner as its callback message (`SendNotifyMessage`), after
`AllowSetForegroundWindow(owner)` (the click on the bar gave us the foreground to pass on).
- Version 4 packs the pointer into `wParam` and the message and uID into `lParam`; older versions
  send the uID and the message.
- Left: mouse move, down, up, then `NIN_SELECT` (v3+). Right: move, down, up, then `WM_CONTEXTMENU`
  (v3+). Middle and double click too.
- `Shell_NotifyIconGetRect` gets the icon's rect from its last click (or the pointer).
- Icons whose owner window is gone, or recycled by another process, are dropped.

**Pins and order** come from `HKCU\Control Panel\NotifyIconSettings`, as the user arranged them in
Windows:
- An icon is matched to its entry by exe and uID or GUID.
- `IsPromoted` icons show in the bar; the rest are in the overflow grid.
- `UIOrderList` sets the order.
- The key is watched (`RegNotifyChangeKeyValue`; re-read 2 s after a change, at most every 5 s),
  so a change in Settings shows within seconds.

**Caveats, and what the host does about them:**
- Explorer's restart (Zebar #301): a second `Shell_TrayWnd` during Explorer's start keeps it from
  building its taskbar. So when Explorer's process exits, or its tray window is destroyed (the
  window itself: `OBJID_WINDOW`, and gone by `IsWindow`), the host removes its window at once,
  without a broadcast (there's no tray for apps to go to; Explorer broadcasts its own). It comes
  back 4 s after Explorer's own `TaskbarCreated`, or 4 s after its 3 s look finds Explorer's tray
  again (that broadcast can be missed), and never before Explorer has run for 4 s.
- Its window destroyed by someone else: the host steps aside the same way, and comes back.
- Stopping (the tray module removed, `taskbar.mode` back to `keep`, quitting) destroys the window
  and broadcasts `TaskbarCreated`, so apps re-register with Explorer.
- A crash leaves nothing behind: the window dies with the process, and Explorer has every icon.
- The bars' own `Shell_TrayWnd` lookups (`taskbar::explorer_tray`, `taskbar_windows`: `FindWindowEx`
  by class, see "Config" → `taskbar.mode`) only take
  `explorer.exe`'s windows, so hiding and restoring the taskbar never touches the host's window
  (or another shell's).
- Apps that don't handle `TaskbarCreated` keep their icon in Explorer only (Seelen and Zebar note
  the same).
- Elevated apps' icons arrive, but our clicks can't reach them unless they let the callback
  message through UIPI.
- Windows' own volume and network icons aren't tray icons on 24H2+; the modules show those. The
  volume icon still arrives as an Explorer icon ("Speakers (USB): 94%").
- Taskbar overlay badges and progress (`ITaskbarList3`) aren't tray messages and stay out of reach
  (research 2.5).

### Power

The `power` module's menu (`menu.ts`): lock, sleep, sign out, restart, shut down; restart and shut
down ask first. The launcher's footer has the same actions.

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
| `start` | — (Windows' own Start menu: the `native-start` action) | — |
| `search` | — | — |
| `showDesktop` | — | — |
| `actionCenter` | `{ panel: "notifications"\|"quick" }` (Windows' own panel: the `native-*` actions) | — |
| `flyoutFailed` | `{ flyout: "launcher"\|"quick"\|"network"\|"mixer"\|"calendar"\|"notifications", error }` | — (the host logs a warning and opens the flyout's native panel, as in "Flyout actions and native actions") |
| `volume` | `{ set?: 0..1, mute?: bool, delta?: -1..1 }` | `{ level, muted }` |
| `media` | `{ action: "toggle"\|"next"\|"previous" }` | — |
| `tray` | `{ id, button: "left"\|"right"\|"middle"\|"double", rect? }` (`rect`: the icon in DIPs from the window's top-left) | — |
| `power` | `{ action: "lock"\|"sleep"\|"signout"\|"restart"\|"shutdown" }` | — |
| `workspace` | `{ direction: "left"\|"right" }` | — |
| `openSettings` | `{ uri }` (an `ms-settings:` URI only) | — |
| `run` | `{ module, trigger: "left"\|"middle"\|"right"\|"wheelUp"\|"wheelDown" }` | — (runs the `exec`/`open` bound in this bar's config; `NotFound` if none) |
| `trash` | `{ action: "open"\|"empty" }` | — |
| `previewOpen` | `{ hwnds }` (at most 16) | `{ hwnd, width, height }[]` (px; windows that are gone are left out) |
| `previewPlace` | `{ windows: { hwnd, rect: [x, y, width, height] }[] }` (DIPs from the window's top-left) | — |
| `previewClose` | — | — |
| `region` | `{ rects: [x, y, width, height][] }` (DIPs from the window's top-left, at most 64) | — |
| `glass` | `{ panes: [{ rect: [x, y, width, height], radius, opacity, saturation, brightness, blur }] }` (at most 32): where live blur goes (see "Live blur") | — |
| `capture` | `{ rect }` (DIPs from the window's top-left): what's behind it, for liquid glass | `{ url, rect }` (the copy, and the part of the screen it covers, clipped to the monitor) |

`pin` of an item that's already pinned moves it to `index`. `reorder`'s paths are matched
case-insensitively; pins it doesn't list keep their order after the listed ones.

The flyouts' methods (`crates/bars/src/flyouts/mod.rs` → `METHODS`; the page's side is
`bars/shell/src/flyouts/protocol.ts`). Requests marked *later* are answered by a provider's lane
once it has the answer, so the reply may come after other messages:

| Method | Params | Result |
|---|---|---|
| `watch` / `unwatch` | `{ topic }`: `mixer`, `wifi`, `launcherApps`, `notificationList`, or a core topic (`media`, `volume`, `network`, `battery`, `stats`, `clock`, `language`, `notifications`) | — |
| `room` | `{ extent }`: DIPs past the band the window must reach (0: back to `ROOM`) | — |
| `keyboard` | `{ on, handBack? }`: the bar becomes the foreground window with the page focused, or lets it go (`handBack`: back to the app before it) | — |
| `apps` | — | `{ apps: [{ id, name, path, icon?, keywords?, packaged }], recent: [{ id, path, count, last }], user: { name } }` |
| `launcherRun` | `{ id, path }` | `{ recent }` (opens it and records the run) |
| `recentFiles` | — | *later*: `[{ name, path, at, icon? }]` (newest first, at most 40) |
| `clipboard` / `clipboardPaste` | — / `{ index }` | *later*: `{ enabled, entries: [{ kind: "text"\|"link"\|"image"\|"other", text, image? }] }` / — |
| `quick` | — | *later*: `{ radios: [{ kind, name, on }], brightness?, power?, saver?, dnd?, focus? }` |
| `setRadio` / `setBrightness` / `setPowerMode` | `{ kind: "wifi"\|"bluetooth"\|"cellular", on }` / `{ level: 0..100 }` / `{ mode: "efficiency"\|"balanced"\|"performance" }` | *later*: the quick state / — / the quick state |
| `wifi` | `{ scan? }` | *later*: `{ networks: [{ ssid, signal, security, connected, known }], error?: "noAdapter"\|"location"\|"unavailable" }` |
| `wifiConnect` / `wifiDisconnect` | `{ ssid, password? }` / — | *later*: — |
| `mixer` | — | *later*: `{ devices: [{ id, name, default }], sessions: [{ id, name, icon?, level, muted, system, active }], switchable }` |
| `setSession` / `setOutput` | `{ id, level?, mute? }` / `{ id }` (only with `audio.switchOutput`) | *later*: — / the mixer state |
| `setLayout` | `{ id }` (an HKL in hex) | — |
| `launcherPin` / `launcherUnpin` | `{ path, name?, index? }` / `{ path }` | — (writes `launcher.pinned`) |
| `notifications` / `dismissNotification` / `clearNotifications` | — / `{ id }` / — | *later*: `{ notifications: [{ id, app, icon?, title, body, at }], error?: "denied"\|"unavailable" }` |

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
| `tray` | `{ id, name, icon?, pinned }[]` (`icon`: a PNG `data:` URL from the tray host, a cache URL from UI Automation) |
| `language` | `{ current?, layouts: [{ id, language, layout, short, region }] }` |
| `notifications` | `{ count }` (null when they can't be read) |
| `mixer` / `wifi` / `launcherApps` / `notificationList` | a watched flyout's live data (the shapes above) |
| `launcherPins` | the launcher's pins after an edit |
| `open` | `{ flyout: "launcher" }`: the Windows key (transient) |
| `workspaces` | `{ count, current }` (`current` is 0-based) |
| `stats` | `{ cpu, memory: { used, total } }` |
| `custom` | `{ [id]: { text, icon?, tooltip?, class?, error? } }` |
| `trash` | `{ full }` |
| `appearance` | `{ appearance (with `style`, the appearance's design style by name), themeCss, system: { accent: [base, light-1..3, dark-1..3], scheme }, vars, iconPack, assets }` (`vars`: the theme's variables as CSS values; `iconPack`: the pack with its `base` URL, or null; `assets`: image paths in bars.json → URLs) |
| `mode` | `{ reduceMotion, performance, blur, liquid }` (`blur`: live blur draws for this window; `liquid`: liquid glass may draw) |
| `backdrop` | `{ url, width, height, x, y }` (the monitor's size and the window's offset on it, DIPs), or null |
| `behind` | what's behind the bar may have changed (its window moved, the wallpaper's snapshot changed): liquid glass captures again |
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
- **Designs.** A design is `crates/bars/presets/top/<id>.json` or `presets/dock/<id>.json` (added
  to `TopDesign` or `DockDesign` and its `ALL` list, and to `TOP_DESIGNS` / `DOCK_DESIGNS` in
  `bars/shell/src/protocol.ts`; the tests check that each parses, has a look, and never sets
  `enabled`, `monitors`, `position`, `autohide` or pins) and its rules in
  `bars/shell/src/designs/top.css` or `dock.css`, scoped by `data-bar` and `data-design`.
- **Presets.** A preset is `crates/bars/presets/<name>.json` (added to `Preset` and its `ALL` list;
  the tests check that every preset parses and never sets `taskbar`, `vars` or pins) and its
  pairing: which design each bar starts from, and whether each bar is on (no look of its own).
- **Flyouts.** `bars/shell/src/flyout.ts` is the one mechanism for anything that opens over the
  screen. `defineFlyout(id, render)` registers one; modules open it with `toggle(props, { anchor,
  place })`. One is open at a time; it closes on Escape, an outside press and window blur; it's part
  of the window region while open; its `scope` ends its subscriptions on close; it enters with the
  `open` animation and fades out. Menus (`menu.ts`: the power menu, the dock's context menu) are
  the `menu` flyout, previews the `preview` flyout. The launcher, quick settings,
  network, mixer, language, calendar, notifications and tray overflow are further flyouts
  (`bars/shell/src/flyouts`, see "Flyouts").
  - A new one opens through `flyouts/surface.ts` `show()`. That gives it placement, more window
    room when it's taller than `ROOM` (360 DIPs; the `room` request), and the keyboard if it's
    typed into.
  - Its data is a provider on a lane (`crates/bars/src/flyouts/service.rs`): a `Request` variant
    plus its name in `METHODS`, and a `Watched` variant for live data.
- **Styling.** Everything is drawn from CSS variables and attributes, and the theme's CSS is
  inserted last (`<style id="theme-css">`, after the generated `<style id="look-css">`), so presets
  and themes restyle without code:
  - on `<html>`: `data-bar="top|dock"`, `data-preset`, `data-design`, `data-family`,
    `data-glyphs`, `data-glass-style` (`liquid|frosted|none`), `data-live` and `data-fly-blur`
    (live blur on the bar's blur fill and its flyouts), `data-liquid` and `data-fly-liquid` (liquid
    glass on its surfaces and flyouts), `data-edge` and `data-position`
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

- **Mock parameters** besides those below: `top=<design>`, `dock=<design>`, `wall=grid` (a
  high-contrast stand-in, to see refraction), `click=<selector>` (clicks it once the bar is up: opens
  a flyout), `noblur`, `noliquid`, and `embed`: inside another page (Studio), the parent posts
  `{ type: "mypaper-bars", bars: <bars.json or part of one> }` and the bar redraws with it, resolved
  like the host does.
- Without a WebView2 host, `pnpm -C bars/shell dev` (or the built page in a browser) runs against a
  mock host that resolves presets from the same bundles: `?bar=top|dock&preset=<name>&pos=bottom|top|left|right&autohide=1&scheme=light|dark&wall=dark|light|vivid&reduce=1&perf=1`,
  plus `poke=<selector>` (the pointer arrives there: magnify, labels, previews) and `dump=<selector>`
  (element boxes into `body[data-dump]`) for headless screenshots.
- To try it without touching your real setup, point `APPDATA` and `LOCALAPPDATA` at a temporary
  folder holding `mypaper\bars.json`.
