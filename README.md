# mypaper

**Total Windows customization at close to zero cost.** Live wallpapers of every kind (shaders,
photos, slideshows, video, 3D scenes, web pages, even a Unity build), interactive desktop widgets
with a plugin API, and native Windows styling (accent, borders, corners, Mica, dark mode). It all
goes to sleep the moment a game or fullscreen app is in front.

> Status: wallpapers (7 kinds), widgets with edit mode and plugins, Windows styling, Studio, the
> tray and smart pausing all work. See the [Roadmap](#roadmap) for what's next.

## Principles

1. **Lightness comes from scheduling.** Render only what's visible, and only when it changes.
   Everything hidden costs 0% CPU and GPU: each process sleeps on its message queue until a window
   event wakes it.
2. **A small native core, with everything visual as content.** Themes, shaders, scenes and widgets
   are folders that reload live. Rust only changes when the engine does.
3. **Separate processes.** The wallpaper, the widget host and every plugin are isolated. One
   crashing never takes the others down, and the supervisor restarts them with backoff.
4. **Always reversible.** The panic hotkey restores the stock desktop. Windows styling keeps an undo
   journal that is replayed on quit and after a crash.
5. **Supported APIs only.** No code is injected into other processes. Windows styling uses public
   DWM and registry APIs, and only touches Windows' own windows (see
   [Windows styling](#windows-styling)).

## Quick start

Requirements: Windows 10/11, [rustup](https://rustup.rs) and [pnpm](https://pnpm.io). The pinned
nightly toolchain installs itself on the first build.

```sh
cargo build --release                         # core, wallpaper, widget host, weather plugin
pnpm -C widgets/shell install && pnpm -C widgets/shell build   # the widget layer's page
cp target/release/weather.exe plugins/weather/                 # the bundled plugin's binary
target/release/mypaper-core                   # run it: wallpaper + widgets + tray icon

# Studio (gallery, new-theme flow, Windows styling editor, settings)
cd apps/studio && pnpm install && pnpm release   # → src-tauri/target/release/mypaper-studio.exe
```

Use `pnpm release`, not a plain `cargo build`: only a Tauri build embeds the UI. A plain build
opens a window pointing at the dev server.

| Shortcut | Does |
|---|---|
| **`Ctrl+Alt+Shift+Backspace`** | Panic: stop everything and restore the stock desktop |
| **`Ctrl+Alt+Shift+W`** | Toggle widget edit mode (also in the tray: **Edit widgets**) |

Themes, widgets and plugins are found in `%APPDATA%\mypaper\{themes,widgets,plugins}` and then in
the bundled `themes/`, `widgets/` and `plugins/` folders beside the exe or any parent folder, so
`target\release\mypaper-core.exe` finds the repo's content. Release builds open no console and log
to `%LOCALAPPDATA%\mypaper\mypaper.log`.

## The dev loop

| You change | What happens | Measured |
|---|---|---|
| A theme file (`.wgsl`, `theme.json`, assets) | Live reload in the running wallpaper. Errors print with exact line numbers, and the last good version keeps running | **25–70 ms** |
| A widget (`widgets/<id>/`) or `widgets.json` | Live reload of that widget or of the layout | instant |
| Any Rust | Incremental rebuild with Cranelift, the parallel frontend and rust-lld | **1.3–2.6 s** |

Widgets can be developed in a browser with the mock host in
[`packages/widget-sdk`](packages/widget-sdk) (see [docs/writing-widgets.md](docs/writing-widgets.md)).
`cargo paper --windowed` renders a theme in a normal window.

**Rust hot-patching** ([subsecond](https://crates.io/crates/subsecond), `scripts/hotpatch.cmd`) is
wired up and builds patches in about 3 s, but patched code crashes on its first call with dx 0.7.10
on Windows. See [docs/engine-notes.md](docs/engine-notes.md).

### Why compiles are fast

- `rust-toolchain.toml` pins **nightly** with the **Cranelift** component.
- `.cargo/config.toml`: Cranelift for our crates, LLVM at `opt-level=2` for dependencies (built once,
  fast at runtime), `-Zthreads=8`, `-Zshare-generics`, and `rust-lld` in place of `link.exe`.
- `debug = "line-tables-only"`, many small crates, and a DX12-only wgpu (no Vulkan, GL or Metal to
  compile).
- Tip: put the repo on a [Dev Drive](https://learn.microsoft.com/windows/dev-drive/) for another large
  speedup.

## Architecture

```text
mypaper-studio      Tauri app: gallery, new theme, Windows styling editor, settings → writes files
mypaper-core        supervisor, tray, panic hotkey, Windows styling (+ undo journal)
  ├─ mypaper-wallpaper   one GPU device, one surface per monitor, behind the icons
  │     ├─ sources        shader · image · slideshow · video · scene · web · app
  │     ├─ pausing        covered monitors; held for fullscreen apps, lock, display off, battery, user
  │     └─ live config    config.json → theme switch; theme files → hot reload; palette → palette.json
  └─ mypaper-widgets     one transparent WebView2 per monitor, above the icons, click-through
        ├─ widgets        sandboxed iframes per widget, permissioned bridge, CSP per manifest
        ├─ edit mode      grid snapping, drag/resize with per-widget min/max, add/remove, settings
        └─ plugins        separate processes, JSON-RPC over stdio, suspended while held
```

| Crate / folder | What it owns |
|---|---|
| [`crates/core`](crates/core) | Supervisor (wallpaper essential, widget host optional), restart policy, tray, panic hotkey, styling lifecycle |
| [`crates/wallpaper`](crates/wallpaper) | The engine: wgpu renderer, sources, frame pacing, live config, palette readback |
| [`crates/widgets`](crates/widgets) | The widget host ([docs/widgets.md](docs/widgets.md)); its page is [`widgets/shell`](widgets/shell) |
| [`crates/tweaks`](crates/tweaks) | Windows styling: DWM attributes, accent color and dark mode, undo journal |
| [`crates/desktop`](crates/desktop) | Win32 plumbing: desktop attach (behind or above the icons), monitors, occlusion, fullscreen detection, hold reasons, watchers |
| [`crates/inject`](crates/inject) | The allowlist (which processes' windows may be styled) and Microsoft signature checks |
| [`crates/config`](crates/config), [`crates/theme`](crates/theme) | `config.json` and the theme format |
| [`packages/widget-sdk`](packages/widget-sdk) | The widget SDK: typed API, `mypaper.js` drop-in, JSON Schemas, mock host |
| [`widgets/`](widgets), [`plugins/`](plugins) | The bundled widgets and the weather plugin |
| [`apps/studio`](apps/studio) | mypaper Studio (Tauri v2 + React + Tailwind) |

### What keeps it light

Release builds on a Ryzen laptop (AMD iGPU and NVIDIA dGPU, two 1440p monitors, 16 threads).

**Visible, per wallpaper:**

| Wallpaper | CPU (machine) | Notes |
|---|---|---|
| `aurora`, shader at 60 fps | **0.17%** | |
| `photo`, image | **0.02%** | drawn once, then idle |
| `gallery`, slideshow | **0.01%** | idle between slides |
| `loop`, 1080p30 video | **0.23%** | hardware decode, zero-copy D3D11→D3D12, 10-bit/HDR tone-mapped |
| `islands`, 3D scene at 30 fps | **0.12%** | 4x MSAA, targets shared across same-size screens |
| `webdemo`, web page at 30 fps | ~1% | Chromium's own frame pipeline |
| 7 widgets visible | **0.07%** | one WebView2 per monitor, one shared stats timer |

**While a game is fullscreen** (measured during a real game), every process is held:

| Process | CPU | Working set |
|---|---|---|
| `mypaper-core` | 0% | 11 MB |
| `mypaper-wallpaper` | 0% | 4 MB (trimmed after load) |
| `mypaper-widgets` | 0% | 20 MB; after 30 s held, its WebView2 processes are closed (0 left) and rebuilt on return |

Private memory stays higher than the working set (about 300 MB for the wallpaper, most of it the GPU
driver's own allocation; a bare wgpu device costs about 240 MB on this machine). The findings are in
[docs/engine-notes.md](docs/engine-notes.md). Binaries: engine 5 MB, widget host 1.4 MB, core 0.8 MB,
Studio 4 MB.

## Config, tray and pausing

Settings live in `%APPDATA%\mypaper\config.json`. Every field is optional, and typos are rejected:

```json
{ "theme": "aurora", "paused": false, "pause_on_battery": true, "pause_on_fullscreen": true }
```

Every process watches it and applies edits live; the Studio, the tray and a text editor all just
write the file.

**Tray.** Right-click for:
- **Pause / Resume**
- **Themes** (the active theme is checked)
- **Edit widgets**
- **Open Studio**
- **Restart wallpaper**
- **Quit** (same cleanup as the panic hotkey)

**Pausing.** Any one of these puts the wallpaper and widgets to sleep at 0% CPU and GPU:

| Reason | When |
|---|---|
| paused by user | `paused` is set |
| on battery | `pause_on_battery`, and the PC is on battery |
| display off / session locked | always |
| fullscreen app in front | `pause_on_fullscreen`, and a fullscreen game or app is in front, **on every monitor** |

Fullscreen detection covers exclusive fullscreen and presentation mode
(`SHQueryUserNotificationState`) and borderless windows that cover their whole monitor. It's
re-checked only on window events, never on a timer. A monitor covered by a maximized window pauses
on its own.

## Themes

A theme is a folder with a `theme.json`:

```json
{
  "name": "Midnight",
  "wallpaper": { "kind": "shader", "source": "midnight.wgsl", "fps": 60 },
  "windows": { "accent": "auto", "border": "auto", "corners": "round", "backdrop": "mica", "dark_mode": true }
}
```

| `kind` | Fields (defaults) | Bundled |
|---|---|---|
| `shader` | `source`, `fps` (60), `scale` (1) | `aurora`, `midnight` |
| `image` | `source`, `fit` (cover). WIC decode, EXIF rotation, downscaled to the largest monitor | `photo` |
| `slideshow` | `folder`, `interval` (300 s), `transition` (1.5 s), `shuffle`, `fit` | `gallery` |
| `video` | `source`, `fit`, `fps` (30), `speed` (1). H.264/HEVC/VP9/AV1 via Media Foundation, 8- and 10-bit, HDR tone-mapped | `loop` |
| `scene` | `source` (.glb/.gltf), `fps`, `scale`, `orbit` (4°/s), `parallax` (0.15), `background` | `islands` |
| `web` | `url` (http(s), or a local .html). WebView2, suspended while hidden | `webdemo` |
| `app` | `command`, `args` (`{hwnd}` = the window to render into; Unity: `-parentHWND {hwnd} delayed`) | see [`themes/app-example`](themes/app-example) |

`fit` is `cover`, `contain`, `stretch` or `center`. Relative paths stay inside the theme folder,
while absolute paths (your own media) are fine. More in
[docs/web-and-app-wallpapers.md](docs/web-and-app-wallpapers.md).

**Shader wallpapers** define one function. The uniforms and entry points live in
[`prelude.wgsl`](crates/wallpaper/src/prelude.wgsl), appended after your code so error line numbers
match your file:

```wgsl
fn wallpaper(frag: vec2f) -> vec4f {
    let uv = frag / paper.resolution;   // also: paper.mouse, paper.time (stops while hidden), paper.frame
    return vec4f(uv, 0.5 + 0.5 * sin(paper.time), 1.0);
}
```

## Widgets

Interactive HTML/JS widgets on the desktop, above the wallpaper and icons and below your apps. They
stay visible after Win+D, and clicks between widgets still reach the icons.

- **Edit mode** (`Ctrl+Alt+Shift+W`, or tray → **Edit widgets**):
  - drag to move, with snapping to a grid that matches Windows' desktop icon grid;
  - resize within each widget's own min and max size (in cells);
  - no overlaps;
  - add from the palette, plus Duplicate, Settings and Remove;
  - keyboard: arrows move, Shift+arrows resize, Esc exits.
- **Bundled**: clock, system monitor, now playing (Windows media controls), weather (via a plugin),
  notes and launcher. On first run, the clock, system monitor and now playing are placed on the
  right edge of the primary monitor. The layout is `%APPDATA%\mypaper\widgets.json`.
- **Write your own**: a widget is a folder with `widget.json` + HTML/CSS/JS, using the `mypaper`
  API for settings, storage, size, system stats, media, launching, the wallpaper palette and plugins.
  Each widget runs in its own sandboxed origin with only the permissions its manifest declares (network
  access is per origin, enforced by a CSP). Start with
  [docs/writing-widgets.md](docs/writing-widgets.md).
- **Plugins**: any executable, in any language, speaking JSON-RPC over stdio. Plugins provide data
  and commands to widgets, are started on demand, and are suspended while mypaper is held. See
  [docs/writing-plugins.md](docs/writing-plugins.md).

## Windows styling

A theme's `windows` section restyles Windows using supported APIs only:

| Field | What it changes |
|---|---|
| `accent` | The system accent color, or `auto` to take it from the wallpaper |
| `border`, `caption`, `caption_text` | Window border and title bar colors (DWM) |
| `corners` | Window corners: `round`, `small` or `square` |
| `backdrop` | `mica`, `acrylic` or `tabbed` |
| `dark_mode` | Dark or light mode |

- **Which windows.** Only Windows' own windows are touched: an allowlist of Windows executables and
  inbox apps, each verified as Microsoft-signed. Games and third-party apps are never touched.
- **Undo.** Every registry value is snapshotted to an undo journal before it changes. It's restored
  on theme change, quit and the panic hotkey, and replayed at startup after a crash.

Details are in [docs/windows-styling.md](docs/windows-styling.md); the Studio has an editor for it.

## Roadmap

1. ✅ **Engine**: wallpapers behind the icons, multi-monitor, pausing, hot reload, supervisor, panic
   hotkey, fast dev loop.
2. ✅ **Every wallpaper kind**: image, slideshow, hardware video (10-bit/HDR), glTF scenes, web, and
   external apps (Unity).
3. ✅ **Studio and tray**: gallery, new theme, Windows styling editor, settings.
4. ✅ **Windows styling**: accent (from the wallpaper), borders, corners, Mica, dark mode, undo journal.
5. ✅ **Widgets and plugins**: the host, edit mode, the SDK and mock host, 6 widgets and a weather plugin.
6. **Next**:
   - a widget gallery and editor in Studio;
   - more widgets (calendar, a dock/launcher bar, a top bar for Linux- and macOS-style layouts);
   - an installer and auto-start;
   - Rust hot-patching once dx works on Windows;
   - interactive app wallpapers (input passthrough).
