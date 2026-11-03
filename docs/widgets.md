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
                                 └─ hold logic: the same reasons as the wallpaper (config + fullscreen + lock …)
```

- **Window placement.** Each monitor gets a window that's a child of the desktop host
  (`mypaper_desktop::DesktopHost`). In the layered (24H2+) layout it's z-ordered **above**
  `SHELLDLL_DefView`, so widgets draw over the icons. In the legacy layout it's a child of the
  DefView's parent, above DefView. It covers the monitor and is `WS_EX_NOACTIVATE` until a widget is
  clicked.
- **Click-through.** The window's region (`SetWindowRgn`) is the union of the widget rectangles,
  plus the whole monitor while edit mode is on. Clicks outside widgets go to the desktop (icons,
  right-click menu, rubber-band selection).
- **Transparency.** WebView2 `DefaultBackgroundColor` is fully transparent, and the shell page's
  background is transparent.
- **Isolation.** Widgets are served from per-widget virtual hosts
  (`SetVirtualHostNameToFolderMapping("<id>.widget.mypaper", <widget folder>, DENY_CORS)`).
  Iframes use `sandbox="allow-scripts allow-same-origin"` (same-origin means *their own*
  origin). Widgets can't read each other or the shell. The shell is served from
  `shell.mypaper` → `widgets/shell/dist`.
- **Pausing.** On hold, every WebView2 gets `put_IsVisible(false)` and `TrySuspend`. On release,
  `Resume` and `put_IsVisible(true)`. Widgets see `visibility` events (below).

## Folders

| What | Where |
|---|---|
| Bundled widgets | `widgets/<id>/` beside the exe or any parent (the same search as themes) |
| User widgets | `%APPDATA%\mypaper\widgets\<id>\` |
| Bundled plugins | `plugins/<id>/` beside the exe or any parent |
| User plugins | `%APPDATA%\mypaper\plugins\<id>\` |
| Layout | `%APPDATA%\mypaper\widgets.json` |
| Widget storage | `%APPDATA%\mypaper\widget-data\<instance-id>.json` |

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
  "permissions": ["system.stats"],
  "plugins": [],
  "settings": [
    { "key": "format24", "type": "boolean", "label": "24-hour", "default": true },
    { "key": "accent", "type": "color", "label": "Accent", "default": "#ffffff" },
    { "key": "city", "type": "string", "label": "City", "default": "" },
    { "key": "density", "type": "select", "label": "Density", "options": ["cozy", "compact"], "default": "cozy" },
    { "key": "opacity", "type": "number", "label": "Opacity", "min": 0, "max": 1, "step": 0.05, "default": 1 }
  ]
}
```

- `id`: lowercase `[a-z0-9-]`, unique; it's the folder name too.
- `size` is in **grid cells**, `[columns, rows]`. `min <= default <= max`. `"resizable": false`
  locks the size to `default`.
- `permissions` (anything not listed is denied):
  - `system.stats`: CPU, memory, GPU, network and disk usage.
  - `media`: now-playing info and transport controls (Windows SMTC).
  - `launch`: open files, folders, URLs and apps (`mypaper.launch`).
  - `network:<origin>`: `fetch` to that origin (for example `network:https://api.open-meteo.com`),
    enforced by the host's web resource filter. No other network access.
  - `theme`: read the current wallpaper palette and theme name.
  - `clipboard.write`.
- `plugins`: plugin ids whose methods this widget may call (see Plugins).
- `settings`: rendered by the shell in edit mode, and by Studio. Values go to the widget at load
  and on every change.

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
      "settings": { "format24": true }
    }
  ]
}
```

- **The grid.**
  - `cell: 0` means "match the desktop icon grid": horizontal and vertical icon spacing from
    `HKCU\Control Panel\Desktop\WindowMetrics` `IconSpacing`/`IconVerticalSpacing` (twips, so
    `-15 × value / 1440 × DPI` px), falling back to 96 px at 100% scale, then scaled per monitor DPI.
  - A positive `cell` is a fixed size in DIPs.
  - `gap` and `margin` are in DIPs. The grid starts at the monitor's **work area** (excluding the
    taskbar) plus `margin`.
- `monitor` is the device name (`MONITORINFOEXW.szDevice`). If that monitor is missing, the
  instance is shown on the primary monitor at its cell, clamped, and its saved monitor isn't
  changed.
- The host writes the file atomically (temp file plus rename) and watches it, so Studio or a text
  editor can edit it live.

## Edit mode

- **Toggle**:
  - from the tray (**Edit widgets**);
  - from Studio;
  - with the hotkey **Ctrl+Alt+Shift+W**;
  - by posting the registered window message `mypaper.widgets.edit` (`RegisterWindowMessageW`)
    with `wParam` 1 (on), 0 (off) or 2 (toggle) to the host's message-only window, whose class is
    `mypaper.widgets.host`.
- **What it shows**: the grid (subtle dots or lines on the work area), each widget with a frame,
  a drag area, resize handles (edges and corners) and a ⋯ menu (Settings, Duplicate, Remove), plus
  a **+ Add widget** palette listing every installed widget. The window region covers the whole
  monitor while editing.
- **Moving and resizing**: live, with the pointer. Positions snap to cells. Sizes are clamped to the
  manifest's `min`/`max`. Widgets can't overlap: a move that would overlap swaps or shifts the
  other widgets (they move down, then right; never off-grid), or the move is refused with a shake
  animation. The rule is documented in the shell.
- **Saving**: every committed change is written to `widgets.json` at once. **Esc** or **Done**
  leaves edit mode.
- **Keyboard**: arrows move the selected widget one cell, and Shift+arrows resize it.
- Edit mode never takes effect while a fullscreen app is in front (the host is held then anyway).

## Widget JS API (`mypaper`)

The shell injects a tiny bridge into each widget iframe. Widgets can import the typed SDK
(`packages/widget-sdk`) or use the global `mypaper` object. Every call is a promise. A call to a
capability the manifest didn't declare rejects with `PermissionDenied`.

```ts
interface Mypaper {
  readonly instance: string;        // "clock-1"
  readonly widget: WidgetManifest;  // the parsed manifest
  settings: {
    get(): Promise<Record<string, unknown>>;
    onChange(cb: (s: Record<string, unknown>) => void): () => void;
  };
  size: {
    get(): Promise<{ cells: [number, number]; px: [number, number]; dpi: number }>;
    onChange(cb: (s: { cells: [number, number]; px: [number, number]; dpi: number }) => void): () => void;
  };
  on(event: "visibility", cb: (visible: boolean) => void): () => void;  // pause your timers when false
  on(event: "editmode", cb: (editing: boolean) => void): () => void;
  storage: {                           // per instance, persisted
    get<T>(key: string): Promise<T | undefined>;
    set(key: string, value: unknown): Promise<void>;
  };
  system: {                            // "system.stats"
    stats(): Promise<{ cpu: number; memory: { used: number; total: number };
                       gpu?: number; net?: { up: number; down: number }; disks?: { name: string; used: number; total: number }[] }>;
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
}
```

- **Wire format (iframe ⇄ shell)**: `postMessage` with
  `{ mypaper: 1, id, method, params }` → `{ mypaper: 1, id, result } | { mypaper: 1, id, error: { code, message } }`,
  and events as `{ mypaper: 1, event, data }`. The shell checks the sender's `event.source` against
  the instance's iframe and forwards to the host with the **instance id attached by the shell**, never
  by the widget. The host enforces permissions from the manifest on disk, never from the message.
- **Errors**: `PermissionDenied`, `NotFound`, `InvalidParams`, `PluginUnavailable`, `Internal`.
- **Visibility**: when the host is held, widgets get `visibility:false` and then the WebView is
  suspended. Widgets should stop their animations. Subscriptions are paused automatically.

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
  The plugin replies `{"jsonrpc":"2.0","id":0,"result":{"protocol":1}}`.
- **Calls**: the host sends `{"jsonrpc":"2.0","id":N,"method":"current","params":{…}}` and the plugin
  replies with `result` or `error`.
- **Events**: the plugin sends notifications `{"jsonrpc":"2.0","method":"event","params":{"event":"updated","data":{…}}}`,
  and widgets subscribed via `onPlugin(id, event)` receive them.
- **Lifecycle**: the host sends `{"jsonrpc":"2.0","method":"suspend"}` and `{"jsonrpc":"2.0","method":"resume"}`
  notifications on hold and release, and `{"jsonrpc":"2.0","id":N,"method":"shutdown"}` before
  terminating it (with a 2 s grace period).
- **Isolation**: plugins run in a Job object (killed with the host), have no console, and are
  started with the plugin folder as their working directory.
- `permissions` is documentation for users and Studio. Plugins are native code, so only install
  plugins you trust (Studio says so).

## Bundled examples

- **Widgets**:
  - `clock`: time and date.
  - `system-monitor`: CPU, RAM, GPU and network sparkline.
  - `now-playing`: media with artwork and controls.
  - `weather`: uses the weather plugin.
  - `notes`: a sticky note, using storage.
  - `launcher`: a grid of shortcuts, using launch.
- **Plugins**: `weather` (Rust, open-meteo, no API key).

## Performance budget

- **Host idle (no widgets)**: no WebView created and 0% CPU.
- **With widgets, visible**: one WebView2 per monitor that has widgets. `system.subscribe` is
  driven by one host timer for all subscribers.
- **Held**: WebViews suspended, plugins suspended, and the host waiting on its message queue at 0%
  CPU.
