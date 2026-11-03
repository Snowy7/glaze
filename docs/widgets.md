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
  (`mypaper_desktop::DesktopHost::icons_parent`). In the layered (24H2+) layout it's z-ordered
  **above** `SHELLDLL_DefView`, so widgets draw over the icons. In the legacy layout it's a child of
  the DefView's parent, above DefView. It covers the monitor and is `WS_EX_NOACTIVATE` until a widget
  is clicked. The WebView's rasterization scale is set from the monitor's own DPI.
- **Which monitors.** A monitor gets a window and a WebView only while it has widget instances, or
  while edit mode is on (so widgets can be added to any monitor). With no instances and edit mode
  off, the host creates no WebView at all.
- **Click-through.** The window's region (`SetWindowRgn`) is the union of the widget rectangles,
  plus the whole monitor while edit mode is on. Clicks outside widgets go to the desktop (icons,
  right-click menu, rubber-band selection).
- **Transparency.** WebView2 `DefaultBackgroundColor` is fully transparent, and the shell page's
  background is transparent. Chromium paints an opaque backdrop behind an iframe whose
  `color-scheme` differs from its embedder's, so widgets should leave `color-scheme` at its default.
- **Isolation.** Widgets are served from per-widget virtual hosts
  (`SetVirtualHostNameToFolderMapping("<id>.widget.mypaper", <widget folder>, DENY_CORS)`).
  Iframes use `sandbox="allow-scripts allow-same-origin"` (same-origin means *their own*
  origin). Widgets can't read each other or the shell. The shell is served from
  `shell.mypaper` → `widgets/shell/dist` (`DENY`). Popups, top-level navigation away from the shell,
  script dialogs and every permission prompt (camera, location, notifications, clipboard read, …)
  are refused.
- **Pausing.** On hold, every WebView2 gets `put_IsVisible(false)` and `TrySuspend`. On release,
  `Resume` and `put_IsVisible(true)`. Widgets see `visibility` events (below). A monitor that is
  completely covered by a maximized or fullscreen window is paused the same way, on its own.

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
- Required: `id`, `name`, `size.default`. Optional: `version`, `author`, `description`, `entry`
  (default `index.html`, a relative path inside the folder), `size.min` (default `[1, 1]`),
  `size.max` (default: unbounded), `resizable` (default `true`), `permissions`, `plugins`,
  `settings`. Unknown fields and unknown permissions are errors, so typos fail loudly.
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

  A saved value that doesn't fit its setting (wrong type, a number outside `min`/`max`, a select
  value not in `options`, a color that isn't `#rrggbb`, a list longer than `maxItems` or with an
  invalid field) is replaced by the default. List items get missing fields filled from the
  field's default (or `false`, `0`, `""`, the first option, `#ffffff`) and unknown fields dropped.
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
      "settings": { "format24": true }
    }
  ]
}
```

- **The grid.**
  - `cell: 0` means "match the desktop icon grid": horizontal and vertical icon spacing from
    `HKCU\Control Panel\Desktop\WindowMetrics` `IconSpacing`/`IconVerticalSpacing`. Negative values
    are twips (`px = -value / 1440 × DPI`, so `-value / 15` DIPs); positive values are DIPs. Missing
    values fall back to 96 DIPs. Cells may therefore be rectangular.
  - A positive `cell` is a fixed size in DIPs.
  - `gap` and `margin` are in DIPs. The grid starts at the monitor's **work area** (excluding the
    taskbar) plus `margin`. A cell `[c, r]` sits at `origin + [c, r] × (cell + gap)`, and a widget
    `[w, h]` cells wide is `w × cell + (w − 1) × gap`. The grid has as many whole cells as fit in
    the work area minus the margins.
- `instance` ids are `[a-z0-9-]`, unique; the host names new ones `<widget>-<n>`.
- `monitor` is the device name (`MONITORINFOEXW.szDevice`). If that monitor is missing, the
  instance is shown on the primary monitor at its cell, clamped, and its saved monitor isn't
  changed.
- On display, sizes are clamped to the manifest (`min`/`max`, or `default` when not resizable) and
  cells to the grid. Instances of widgets that aren't installed are kept in the file but not shown.
- The host writes the file atomically (temp file plus rename) and watches it, so Studio or a text
  editor can edit it live. If the file doesn't parse, the host keeps showing the last good layout,
  refuses edits (so it never overwrites your file), and the shell shows the error in edit mode.

## Edit mode

- **Toggle**:
  - from the tray (**Edit widgets**);
  - from Studio;
  - with the hotkey **Ctrl+Alt+Shift+W**;
  - by posting the registered window message `mypaper.widgets.edit` (`RegisterWindowMessageW`)
    with `wParam` 1 (on), 0 (off) or 2 (toggle) to the host's message-only window, whose class is
    `mypaper.widgets.host`. `mypaper-widgets --edit on|off|toggle` does exactly that and exits.
- **What it shows**: the grid (subtle dots or lines on the work area), each widget with a frame,
  a drag area, resize handles (edges and corners) and a ⋯ menu (Settings, Duplicate, Remove), plus
  a **+ Add widget** palette listing every installed widget. The window region covers the whole
  monitor while editing, on every monitor.
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
- Edit mode never takes effect while a fullscreen app is in front (the host is held then anyway).
  Requests to turn it on while held are ignored, and becoming held leaves edit mode.

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
  readonly visible: boolean;        // false while the host is held
  readonly editing: boolean;        // edit mode is on
  settings: {
    get(): Promise<Record<string, unknown>>;
    onChange(cb: (s: Record<string, unknown>) => void): () => void;
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

- **Units**: `cpu` and `gpu` are percent (0–100) of the whole machine; `memory` and `disks` are
  bytes; `net` is bytes per second, summed over physical interfaces; `disks` lists fixed drives
  (`name` is `"C:"`); `gpu` is the busiest GPU engine and is omitted when unavailable. Media
  `position` and `duration` are seconds. `position` is sampled: change events fire on seeks and
  track changes, not every second, so advance it locally while `playing`. `artwork` is a
  `data:image/…;base64,` URL (omitted above 256 KB). `app` is a readable app name (`Spotify`, not
  `Spotify.exe`). Colors are `#rrggbb`.
- **Errors**: `PermissionDenied`, `NotFound`, `InvalidParams`, `PluginUnavailable`, `Internal`.
- **Visibility**: when the host is held, widgets get `visibility:false` and then the WebView is
  suspended. Widgets should stop their animations. Subscriptions are paused automatically. A
  monitor covered by a maximized or fullscreen window is paused the same way, on its own.

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
`{ instance, widget /* the manifest */, settings, size, visible, editing }`. `mypaper.ready`
resolves with that, and every other call waits for it. (A widget that gets no answer within
10 s fails `ready`: it is not inside mypaper.)

**Requests** (widget → shell, answered with `result` or `error`). The shell checks the permission
from the manifest *before* forwarding to the host (the host checks again, from disk):

| method | params | result | needs |
|---|---|---|---|
| `hello` | `{ sdk }` | the handshake state above | |
| `storage.get` | `{ key }` | the value, or `null` when missing | |
| `storage.set` | `{ key, value }` | `null` (a missing `value`, which is what `undefined` becomes, deletes the key; 1 MB per instance) | |
| `system.stats` | | `SystemStats` | `system.stats` |
| `media.current` | | `MediaState` or `null` | `media` |
| `media.control` | `{ action }` | `null` | `media` |
| `launch` | `{ target }` | `null` | `launch` |
| `theme.current` | | `ThemeInfo` | `theme` |
| `plugin` | `{ plugin, method, params? }` | the plugin's result | `plugin` listed in `plugins` |
| `subscribe` | see below | `null` | per topic |
| `unsubscribe` | `{ sub }` | `null` | |

**Subscriptions.** `subscribe` params are `{ sub, topic: "system", intervalMs }`,
`{ sub, topic: "media" }`, `{ sub, topic: "theme" }` or
`{ sub, topic: "plugin", plugin, event }`, where `sub` is an id the widget makes up. Values arrive
as `{ mypaper: 1, event: "sub", data: { sub, value } }`:

- `system`: one sample right away, then one per `intervalMs` (never faster than 1000).
- `media`, `theme`: on change only (read the current value with `media.current` / `theme.current`).
- `plugin`: each matching plugin event's `data`.

While the host is held (`visibility: false`) nothing is delivered; `system` sends a sample as soon
as the widget is visible again.

**Pushed events** (shell → widget, always on, no subscription): `visibility` (`boolean`),
`editmode` (`boolean`), `settings` (the full settings object) and `size`
(`{ cells, px, dpi }`).

**Plugin errors.** A JSON-RPC `error` from a plugin becomes a wire error as follows: if
`error.data.code` is one of the five names above it is used; otherwise `-32601` is `NotFound`,
`-32602` is `InvalidParams` and anything else is `Internal`. A plugin that isn't running, can't
start or crashed mid-call is `PluginUnavailable`. The message is passed on.

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
                   settings: Record<string, unknown> }[];  // resolved: defaults plus saved values
      widgets: WidgetManifest[];                           // every installed widget (the palette)
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
  | { type: "edit"; on: boolean };                         // Done / Esc
```

- The host sends `state` after `ready` and whenever anything in it changes. The shell reconciles by
  instance id and never reloads an iframe that is still there. It answers each frame's `hello`
  from the state (and tells the host), and pushes `editmode`, `visibility`, `settings` and `size`
  to widgets when those change in the state.
- `call` carries every other wire request unchanged; the host answers with `reply`, and delivers
  subscription values as `event` (`sub`). While a monitor is hidden its widgets get nothing.
- Layout edits (`layout`, `add`, `duplicate`, `remove`, `settings`) are applied, clamped, saved, and
  answered with a fresh `state`, which is authoritative: a refused edit just gets the old state back.
  The shell resolves overlaps and finds free cells before sending.
- The host also injects a small hardening script (`widgets/shell/dist/harden.js`) into every frame
  at document creation (`AddScriptToExecuteOnDocumentCreated`); see [Network](#network).

## Network

Every WebView has a `WebResourceRequested` filter for `*` (every context and, where the runtime
supports it, every request source, including workers). The host attributes each request to the
frame that made it from the `Origin` header, or failing that the `Referer` header, which Chromium
sets from the requesting document and which pages can't forge. Then:

| Request to | Allowed when the initiator is |
|---|---|
| `https://shell.mypaper/…` | the shell, or nobody (the shell's own navigation) |
| `https://<id>.widget.mypaper/…` | the shell (loading the iframe) or the same widget |
| anything else (`http`, `https`, `ws`, `wss`) | a widget whose manifest lists `network:<that origin>` |

Everything else is answered with a `403` without touching the network: requests with no
attributable initiator, a `null` origin, a widget stripping its referrer (`no-referrer` breaks the
widget, by design), service-worker scripts, and other schemes. `data:` and `blob:` URLs never reach
the network and aren't filtered. WebRTC traffic doesn't pass through the filter, so the hardening
script removes `RTCPeerConnection` (and its relatives) from every frame before the page's own
scripts run.

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

- **Widgets**:
  - `clock`: time and date.
  - `system-monitor`: CPU, RAM, GPU and network sparkline.
  - `now-playing`: media with artwork and controls.
  - `weather`: uses the weather plugin.
  - `notes`: a sticky note, using storage.
  - `launcher`: a grid of shortcuts, using launch.
- **Plugins**: `weather` (Rust, open-meteo, no API key).
- **Tooling**: `packages/widget-sdk` has the typed SDK, the `mypaper.js` drop-in, the JSON
  Schemas and a mock host for developing widgets in a browser. Tutorials:
  [writing-widgets.md](writing-widgets.md) and [writing-plugins.md](writing-plugins.md).

## Performance budget

- **Host idle (no widgets)**: no WebView created and 0% CPU.
- **With widgets, visible**: one WebView2 per monitor that has widgets. `system.subscribe` is
  driven by one host timer for all subscribers.
- **Held**: WebViews suspended, plugins suspended, and the host waiting on its message queue at 0%
  CPU.

## How to run

```sh
pnpm -C widgets/shell install
pnpm -C widgets/shell build          # → widgets/shell/dist (index.html + harden.js)
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
- `pnpm -C widgets/shell test` runs the grid math tests (vitest).
