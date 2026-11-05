# Plugins: the platform (WIT `mypaper:plugin@0.1.0`)

A mypaper plugin is a **WebAssembly component** (WASI preview 2, the component model). Every
plugin runs in **one helper process**, `mypaper-plugins.exe`, which hosts **one Wasmtime engine**
for all of them. Plugin code never runs inside the shell. It never draws pixels, and it never
touches the OS except through the capabilities its manifest declares.

- The interfaces are in `crates/plugin-api/wit/` (`mypaper:plugin@0.1.0`). This doc explains
  them, and the WIT is the source of truth.
- The Rust guest SDK is `crates/plugin-sdk` (`mypaper_plugin_sdk::guest`, built with
  `--target wasm32-wasip2`).
- The host is `crates/plugins` (the `mypaper-plugins` binary). The shell bridge is
  `crates/widgets/src/plugins.rs` (process and protocol) plus `crates/shell/src/widgets/views/plugin.rs`
  (the kit renderer).
- The proof plugins are `plugins/hello` and `plugins/weather/component`.

The SDK v1 native plugins (one exe per plugin, JSON-RPC over stdio: `weather`, `rss`, and `t3`
and `obs` before they're ported) keep working through the same host bridge. See
[Legacy native plugins](#legacy-native-plugins).

## Rules

1. **Plugins describe, mypaper renders.** UI is a declarative tree of kit components. The shell
   draws it with `crates/ui`, so it gets the same look, themes, glass, DPI and accessibility as
   everything else. The kit handles hover, press, focus and scroll locally, and plugins receive
   only **actions**. The host diffs successive trees and sends and repaints only faces that
   changed (`mypaper_plugin_api::diff`). Animations are declarative (`animated` nodes use the
   kit's springs). `canvas` is the escape hatch: a list of drawing commands (shapes, paths, text,
   images by id) that the shell still rasterizes natively.
2. **Styling goes through tokens.** A plugin restyles a surface (`top-bar`, `dock`, `panel`,
   `menu`, `widget`, `glass`) with a **style patch** of theme tokens. It never draws the bar
   itself. Patches stack in plugin load order and are removed when the plugin is disabled.
3. **Event-driven.** A plugin runs only inside an event: open, resize, action, a timer, a
   subscribed system event, or an I/O completion. Between events it costs no CPU.
4. **Least privilege.** Every capability is gated by a manifest permission. Permissions are shown
   at install and checked by the host on each call. An undeclared call fails with
   `error::denied`.

## Manifest (`plugin.json`)

```json
{
  "id": "hello", "name": "Hello", "version": "0.1.0",
  "component": "hello.wasm",
  "api": "0.1",
  "faces":  [{ "id": "main", "name": "Hello", "size": { "default": [2, 2], "min": [1, 1] } }],
  "bars":   [{ "id": "clock", "name": "Hello clock", "size": { "default": [1, 1] } }],
  "panels": [],
  "menu":   [{ "id": "say-hi", "label": "Say hi" }],
  "commands": [{ "id": "refresh", "label": "Refresh hello", "shortcut": "Ctrl+Alt+H" }],
  "styles": ["top-bar"],
  "permissions": ["network:https://api.open-meteo.com", "storage"],
  "limits": { "memory_mb": 16 }
}
```

- `component` is the `.wasm` component, relative to the folder. A manifest with `component` is a
  WASM plugin, and one with `command` is a [legacy native plugin](#legacy-native-plugins). A
  transitional plugin may have both (the weather plugin keeps its exe for the built-in widget's
  data calls while its faces run as a component).
- `api` is the WIT package major.minor it was built against. The host refuses another major.
- The contributions are `faces` (desktop widgets in the "+" picker, as `<plugin>--<face>`),
  `bars` (top-bar modules), `panels` (flyouts), `menu` (desktop context-menu items) and
  `commands` (palette and shortcuts). They are static, so the shell lists them without
  starting the plugin.
- `styles` lists the surfaces the plugin may patch. Patching any other surface is refused.
- `limits.memory_mb` may *lower* the host's per-plugin cap, never raise it.

Plugins are installed from `plugins/*` (bundled) and `%APPDATA%\mypaper\plugins\*` (user). A
folder's name must be the plugin's id.

## Lifecycle

```
installed ─(face placed / bar module on / command)→ loading → active ─(nothing visible 30 s)→ saved+unloaded
                                                       ↑                                         │
                                                       └──────────── state restored ─────────────┘
```

1. **Install.** The host validates the manifest (known permissions and surfaces, `api` major,
   the component inside the folder) and **AOT-compiles** the component to
   `%LOCALAPPDATA%\mypaper\plugins\cache\<id>-<hash>.cwasm` (`mypaper-plugins --compile <root>`;
   `deploy-local.ps1` runs it for the bundled plugins).
   The hash covers the component bytes and the engine's compatibility hash. Boot only
   deserializes this file, so there is no JIT at boot. A changed `.wasm` recompiles once.
2. **Load.** The host instantiates the component and calls `lifecycle.init(info)` with the
   plugin's settings and its **saved state** (`option<list<u8>>`, `none` the first time).
3. **Active.** The host delivers events (`lifecycle.on-event`), and the plugin answers through
   imports (`ui.set-view`, `timers.after`, `http.fetch`, …).
4. **Unload.** When nothing of the plugin is visible for 30 s (no face placed, no bar module
   showing, desktop held), the host calls `lifecycle.save() -> list<u8>`, stores the result in
   `%APPDATA%\mypaper\plugin-data\<id>\state.bin`, and drops the instance. The next load gets it
   back in `init`.
5. **Runtime.** The helper process starts when the first plugin loads and exits 30 s after the
   last one unloads. With no active plugin, there is no helper process.

A **trap** (panic, out of memory, or exceeding the CPU budget) kills only that plugin's instance.
The host reinstantiates it from its last saved state and keeps its faces on their last view with
a "Restarting…" footer. After 3 traps in 5 minutes the plugin is **stopped**. Its faces then show
"Stopped: <reason>" until it's re-enabled or updated. Other plugins never notice.

## Interfaces (WIT)

`world plugin` imports `ui`, `style`, `timers`, `events`, `http`, `websocket`, `pipes`, `launch`,
`notify`, `secrets`, `storage`, `system`, `native` and `log`. It exports `lifecycle`. The host
links every interface and checks the permission **on each call**, so an undeclared capability
fails with `error::denied` and the plugin keeps running.

| interface | functions | permission |
|---|---|---|
| `lifecycle` (export) | `init(info) -> result<_, string>`, `on-event(event)`, `save() -> list<u8>` | — |
| `ui` | `set-view(instance, face)`, `clear(instance)`, `put-image(width, height, bgra) -> result<u32, error>`, `drop-image(id)` | — |
| `style` | `apply(patch) -> result<_, error>`, `reset(surface)` | the surface in `styles` |
| `timers` | `after(id, ms)`, `every(id, ms)`, `cancel(id)` | — |
| `events` | `subscribe(list<topic>)`, `unsubscribe(list<topic>)` | `windows` for window topics |
| `http` | `fetch(request) -> result<u64, error>`, with the reply arriving as `event::http` | `network:<origin>` |
| `websocket` | `connect(url)` or `connect-with({ url, headers, protocols })` `-> result<u64, error>`, `send`, `close`, with `event::socket` | `network:<origin>` |
| `pipes` | `open(name) -> result<u64, error>`, `write`, `close`, with `event::pipe`; `http-request(name, request) -> result<u64, error>` with `event::http` | `pipe:<name>` |
| `launch` | `open(target) -> result<_, error>` (URL, file or app) | `launch` |
| `notify` | `show(notification) -> result<_, error>` | `notifications` |
| `secrets` | `get(key) -> option<string>`, `set(key, value)`, `remove(key)` | `secrets` |
| `storage` | `get(key) -> option<list<u8>>`, `set(key, value)`, `remove(key)` (≤ 1 MiB) | `storage` |
| `system` | `monitors()`, `theme()`, `power()`, `foreground()` | `windows` for `foreground` |
| `native` | `call(module, method, bytes) -> result<u64, error>`, with `event::native` | `native-code` |
| `log` | `write(level, message)` | — |

**I/O is asynchronous.** Calls that can block (`http`, `websocket`, `pipes`, `native`) return a
request id at once, and the result arrives later as an event. A plugin never holds the runtime
while it waits, and the per-event CPU budget measures only the plugin's own code. Fast local
calls (`secrets`, `storage`, `system`) are synchronous.

### Websockets, pipes, notifications, system events and images

- **Websockets** go through WinHTTP, so Windows owns TLS, proxies and certificates.
  `connect-with` adds request headers (`Authorization: Bearer …`, a ticket) and subprotocols
  (`Sec-WebSocket-Protocol`, e.g. obs-websocket 5's `obswebsocket.json`). The URL must be `ws://`
  or `wss://` and covered by a `network:` permission for that exact origin
  (`network:wss://host[:port]`). Events: `opened(id)`, then `message(id, text | binary)` per whole
  message (fragments joined, at most 4 MiB), then `closed(id, reason)` once (`none` after a
  normal close, else the error or the server's close code). `close` sends a close frame and gives
  the peer 3 s.
- **Pipes**: `open` connects to `\\.\pipe\<name>` (manifest `pipe:<name>`). Names may contain `\`
  (Tailscale's is `ProtectedPrefix\Administrators\Tailscale\tailscaled`) but no empty, `.` or `..`
  part, `/` or `:`. Bytes arrive as `data(id, bytes)` chunks in stream order, then `closed(id)`.
  Framing is the plugin's: Discord IPC (`discord-ipc-0`) is `u32 op, u32 len` (LE) and JSON,
  written and parsed as plain bytes. `http-request(name, request)` sends one HTTP/1.1 request over
  the pipe (`Connection: close`, the URL's host as `Host`, its path and query as the target) and
  answers as `event::http` with status, headers and body (`Content-Length`, chunked, or to EOF):
  the Docker engine (`pipe:docker_engine`, `http://docker/v1.43/containers/json`) and Tailscale's
  LocalAPI (`http://local-tailscaled.sock/localapi/v0/status`).
- **Threads and backpressure.** Each open connection has its own host threads, never one per
  plugin: a websocket a reader and a writer (WinHTTP's synchronous receive can't be woken), a pipe
  one thread on overlapped I/O. They sleep in the kernel between messages. `send` and `write`
  queue and return at once, and fail (`failed`, "the send queue is full") while 4 MiB is queued.
  A connection stops reading while 64 of its events wait undelivered, so a slow plugin slows the
  peer instead of growing the helper. A plugin may have 8 websockets and 8 pipes open, and they
  all close when it unloads, traps or is stopped. Request ids are unique across the helper, so a
  late event from a dropped instance never reaches its replacement.
- **Notifications**: `notify.show` raises a Windows toast from mypaper (AppUserModelID
  `mypaper.Plugins`, display name "mypaper", registered under HKCU on first use) with the
  plugin's title and body and "via <plugin name>" as the attribution line, from a worker thread.
  The `action` field isn't delivered back yet.
- **System events**: the shell sends `HostMsg::System(SystemEvent)` frames
  (`Plugins::system_event` in `crates/widgets/src/plugins.rs`). The helper delivers each one only
  to loaded plugins subscribed to its topic, and window events only to those holding `windows`.
  Window moves are coalesced: the latest per window is kept and delivered once a frame (16 ms),
  and an `opened`, `closed` or `focused` for that window replaces its pending move.
  `notification` and `media` carry JSON from the shell's notification listener and media (SMTC)
  provider.
- **Images**: `ui.put-image(width, height, pixels)` (BGRA8 premultiplied, `width * 4` stride,
  1–4096 px a side) copies the pixels into a pagefile-backed named mapping and returns its id.
  `drop-image(id)` releases it, and unloading releases them all. A plugin holds at most 64 images
  and 16 MiB of pixels. The shell's canvas renderer (`views::plugin::canvas`) opens the mapping
  read-only by name:

  ```text
  name    Local\mypaper-image-<helper pid>-<id>    (ids are unique within the helper)
  0..4    "MPI1"
  4..8    width  u32 LE
  8..12   height u32 LE
  12..16  stride u32 LE (width * 4)
  16..    height rows of stride bytes, BGRA8 premultiplied
  ```
- **`system.foreground`** (`windows`): the foreground window's handle as `id`, its title, process
  image name (`code.exe`) and rect.

The tests in `crates/plugins/src/caps_tests.rs` drive every capability through the probe plugin
(`examples/probe`): a refusal without the permission, a round trip against a local websocket
echo server and named-pipe servers, event routing and coalescing, and a native module call and
event end to end (`examples/native-echo`).

### Events

`event` is a variant. A plugin gets each kind once it opts in: faces, actions, timers and its
own I/O always; system topics after `events.subscribe`.

| case | when |
|---|---|
| `open(face-open)`, `resize(face-open)`, `close(instance)` | a face, bar module or panel was placed, resized or removed |
| `action(action)` | a click on a node's `action`, a submitted field's text, a chosen select value |
| `command(id)` | a `commands`/`menu` contribution was invoked |
| `timer(id)` | a timer fired. Timers are **coalesced**: the host rounds deadlines to a 250 ms grid and wakes once for all due timers, and `every` under 1 s is refused |
| `http(http-reply)`, `socket(socket-event)`, `pipe(pipe-event)`, `native(native-event)` | I/O completions |
| `window(window-event)` | `opened`, `closed`, `focused`, `moved` (topic `windows`) |
| `monitor`, `notification`, `media`, `power`, `theme` | the system topics of the same names |
| `suspend`, `resume` | the desktop is held (fullscreen app or locked) or released |

### Views

WIT has no recursive types, so a face is a **flat pre-order list**: `face { nodes, footer,
refresh }`, where each `node { kind, children }` is followed by its `children` direct children
(each a whole subtree). The SDK builds this from a normal tree. The node kinds are the kit
vocabulary SDK v1 already draws:

`stack`, `row`, `list`, `grid` (layout with `gap`), `card` (one child), `text`, `icon`, `button`,
`icon-button`, `select`, `field`, `list-row`, `stat`, `metric`, `ring`, `limit-bar`,
`segmented-bar`, `sparkline`, `area-chart`, `empty`, `spacer`, plus `animated(spring)`, which
animates its child's appearance and changes with the kit's spring, and `canvas(list<draw>)`.

Colors are `tone`: a theme **role** (`label`, `secondary`, `tertiary`, `accent`, `warning`,
`destructive`, `positive`) or `hex("#rrggbb")` for brand fills. Sizes are kit points, and the
shell applies DPI. A face describes only its body. The shell adds the integration chrome
(icon and title header, footer, and a refresh button when `refresh` is set).

**Diffing.** The host keeps each instance's last face and sends a face to the shell only when
it changed. `mypaper_plugin_api::diff` returns the changed node ranges (a node whose kind or
payload changed replaces its subtree, and unchanged siblings are kept). The shell repaints only
the widgets whose face changed.

### Styling

```wit
record style-patch { surface: surface, tokens: list<token> }
record token { name: string, value: token-value }   // e.g. "background", "tint", "radius", "glass.material"
variant token-value { color(string), number(f32), tone(tone), flag(bool) }
```

The shell applies a patch through the same path as the user's bar and theme settings
(`crates/shell/src/bars/plugin_style.rs`): the bar re-resolves its kit tokens and **restyles in
place** (same window, same appbar band, no flicker), so a patch themes, animates and follows DPI
like a user change. Only geometry tokens (`height`) rebuild the bar, because its band changes.

**Precedence.** The user's explicit fields in `bars.json` (what Studio writes, shared `look` or
the bar's own `top.look`/`dock.look`, and the shared `appearance`) win over a plugin. A plugin
wins over the bar's design, the preset and the theme's defaults. Patches stack in plugin load
order, a later plugin's token over an earlier one's, and `style.reset` or disabling the plugin
removes its patch live.

Token names, per surface (`color` is `#rrggbb` or `#rrggbbaa`; a `tone` names a theme role):

| token | value | `top-bar` | `dock` | `menu` | `glass` | user field that wins |
|---|---|---|---|---|---|---|
| `tint` | color | glass tint colour (a transparent design gets its glass back) | same | via `top-bar` | both bars | `appearance.tint` (`look.transparent` keeps it bare) |
| `tint.opacity`, `glass.tint` | number 0..1 | tint strength | same | via `top-bar` | both bars | `look.glass.tint`, `look.tint` |
| `glass.clarity` | number 0..1 | material clarity | same | via `top-bar` | both bars | `look.glass.clarity` |
| `glass.blur` | number ≥ 0 (DIPs) | blur | same | via `top-bar` | both bars | `look.glass.blur` |
| `glass.refraction`, `glass.highlight` | number | Liquid Glass lens | same | via `top-bar` | both bars | `look.glass.*` |
| `radius` | number 0..64 | bar, island and plate corners | same | via `top-bar` | — | `look.radius`, `appearance.radius` |
| `transparent` | flag | no plate, items on the wallpaper | same | — | — | `look.transparent` |
| `background` | color | solid body (styles without glass) | same | via `top-bar` | — | `look.background` |
| `font.size`, `font.weight` | number | the bar's text | same | via `top-bar` | — | `look.font.*` |
| `height` | number 16..96 | bar height (rebuilds the bar) | — | — | — | `top.height` |
| `label`, `secondary`, `tertiary`, `separator`, `surface`, `elevated`, `hover`, `pressed`, `warning`, `destructive`, `positive` | color or tone | the theme colour role | same | the menu's own | — | — |
| `accent` | color or tone | accent | same | same | — | `appearance.accent` |

The desktop and tray menus take the top bar's tokens, then the `menu` surface's colours. Unknown
names are ignored. `panel` and `widget` surfaces are accepted but not applied yet (see Status).

## Permissions

| permission | grants |
|---|---|
| `network:<scheme>://<host>[:port]` | `http.fetch` and `websocket.connect` to that origin (exact scheme, host and port; `*.` allows subdomains) |
| `pipe:<name>` | `pipes.open` of `\\.\pipe\<name>` |
| `launch` | `launch.open` |
| `notifications` | `notify.show` |
| `secrets` | the plugin's own namespace in Credential Manager: `mypaper/integrations/plugin/<id>/<key>` |
| `storage` | `storage.*` under `%APPDATA%\mypaper\plugin-data\<id>\kv\` |
| `windows` | window events and `system.foreground` (titles are personal) |
| `native-code` | `native.call` into the plugin's own native modules (see [Native modules](#native-modules)) |

Timers, views, styles of declared surfaces, `system.monitors/theme/power` and `log` need no
permission. The WASI preview-2 imports a plugin gets are clocks, random, and stdout/stderr
(sent to the log). There are **no** preopened directories, environment variables, sockets or
arguments. A component that imports something the host doesn't provide fails at install with a
clear message.

## Budgets (enforced by the host)

| budget | value | how |
|---|---|---|
| idle CPU | 0% | no polling. The helper blocks on its channel. The epoch ticker thread runs only while a plugin call is executing, and timers are coalesced |
| runtime memory | ~5–10 MB, only while any plugin is active | one `Engine`. The helper exists only while a plugin is loaded |
| per plugin | ~0.5–2 MB typical, **hard cap 32 MiB** of linear memory | pooling allocator slots plus a `ResourceLimiter` (memories, tables, instances) |
| per-event CPU | **50 ms** of plugin code per event | epoch interruption (5 ms ticks). Exceeding it traps the call, which counts as a strike, and the plugin is restarted. After 3 strikes it's stopped |
| startup | no JIT | AOT `.cwasm` cache, deserialized with `Component::deserialize_file` |
| IPC | binary frames | `u32` LE length + `postcard` message on the helper's stdio. Images go through named shared memory (`image-id`) |
| action → repaint | ≪ one frame (16 ms) | measured by `mypaper-plugins --bench` |

Measured 2026-11-05 (release, `mypaper-plugins --bench plugins/hello plugins/weather`, and the
live helper with the weather face placed):

| measure | value |
|---|---|
| idle CPU, live helper with weather loaded, 60 s | 0.0 ms (0.000%), 4 threads |
| live helper, weather loaded | 16.4 MB working set / 7.4 MB private |
| engine + linker + ticker | +0.8 MB WS / +3.2 MB private, 0.8 ms |
| per active plugin (hello / weather) | 1.06 MiB linear memory; +1.2 / +1.4 MB private |
| cold load (AOT compile + instantiate + init) | hello 130 ms, weather 213 ms (once, at install) |
| warm load (cached `.cwasm` + instantiate + init) | 0.62 ms |
| action → view frame decoded, the shell's side included except the draw | p50 0.008 ms, p99 0.017 ms |
| infinite loop stopped (50 ms budget) | 49.5 ms |
| weather: IP lookup + forecast, two HTTP round trips | < 1 s; each reply handled in 0.3–0.5 ms |
| component sizes | hello.wasm 107 KB, weather.wasm 196 KB; `.cwasm` 430 / 740 KB |
| per open websocket / pipe, client side (its threads, buffers and WinHTTP state) | ~230–360 KiB / ~45–85 KiB private, 8 of each, noisy across runs (`cargo test --release -p mypaper-plugins measure -- --ignored --nocapture`) |

## Shell ↔ helper protocol

The bridge starts `mypaper-plugins.exe` lazily when the first WASM plugin's face opens. It runs
inside the shell's kill-on-close Job object with the same restart backoff as other plugins, and
it stops when the last plugin unloads. Frames are `[u32 len][postcard(HostMsg|HelperMsg)]`
(`mypaper_plugin_api::wire`):

- shell → helper: `Open { plugin, dir, instance, face, size, settings }` (a repeat `Open` of a
  placed instance is a resize), `Close`, `Action { instance, action, value }`,
  `Command { plugin, id }`, `Held(bool)`, `Stats`, `Shutdown`.
- helper → shell: `View { instance, face, changed }` (the kit tree as JSON inside the frame,
  changed faces only; `changed` counts replaced subtrees), `Status { plugin, Live | Restarting |
  Stopped | Unloaded }`, `Style { plugin, surface, patch? }`, `Stats { plugin, memory, cpu_us,
  events }`.

The shell reads frames on a thread, queues them, and wakes its loop once per batch through the
existing plugin message (`Msg::Plugin` with the reserved id `HELPER`), so the shell's event loop
didn't change. In the widget layout a component face is the widget `<plugin>--<face>` (the
registry lists one per face, category "Plugins"), so it never collides with a built-in of the
same name: `weather--now` is the component, `weather` the built-in widget (layout widget ids are
`[a-z0-9-]`, so a component plugin id may not contain `--`).

## Packaging

- A plugin folder contains `plugin.json`, the component `.wasm`, and any assets.
- **mypaper ships a DLL only if mypaper itself uses it.** Vendor DLLs (an SDK a native module
  needs) ship inside the plugin that needs them and are loaded only while that plugin is active.
- A future `.mpplugin` is just the folder zipped and signed. The install flow shows the
  permissions and compiles the AOT cache.

## Versioning

The WIT package is semver: `mypaper:plugin@0.1.0`.

- **Minor and patch** (`0.1 → 0.2` once we're at `1.x`, and every bump before then) only *add*
  interfaces, functions, variant cases at the end, or optional record fields as new functions.
  The host keeps linking every older world it has shipped (`wit/v0.1/` stays frozen), so older
  plugins keep working unchanged.
- A **major** bump is a new world beside the old one. The host links both for at least one
  major. Older hosts refuse a newer major at install, with "needs a newer mypaper".
- An unknown node kind from a newer guest can't happen (the guest links against the host's
  world), and the shell draws an unknown legacy view kind as nothing.

## Later: Explorer and app interiors

mypaper's opt-in app styler (`docs/` › "own app styler", after the native shell) will load **the
same runtime** (`mypaper-plugins` as a library, same WIT world, same budgets) to run style
plugins against Explorer and Settings surfaces. Only the `surface` enum grows (`explorer-*`).
Plugins keep describing and never inject code, and the styler applies their token patches
natively.

## Writing one (Rust)

```rust
use mypaper_plugin_sdk::guest::{self, Event, Plugin};
use mypaper_plugin_sdk::{Role, View, export};

#[derive(Default)]
struct Hello { clicks: u32 }

impl Plugin for Hello {
    fn on_event(&mut self, event: Event) {
        match event {
            Event::Open(face) | Event::Resize(face) => self.draw(&face.instance),
            Event::Action(a) if a.action == "click" => { self.clicks += 1; self.draw(&a.instance) }
            _ => {}
        }
    }
}
impl Hello {
    fn draw(&self, instance: &str) {
        guest::ui::set_view(instance, View::stack([
            View::text(format!("Clicked {}×", self.clicks), Role::Headline),
            View::button("Click", "click"),
        ]).face());
    }
}
export!(Hello);
```

The crate is a `cdylib` with its own `[workspace]` (the root `Cargo.toml` excludes guests).
Build it with `cargo build --release --target wasm32-wasip2` and put the `.wasm` beside
`plugin.json`. `scripts/build-plugins.ps1` builds the in-repo guests, and `deploy-local.ps1`
bundles `mypaper-plugins` and AOT-compiles them. A component that also has native modules lists
them in `"native": ["x.dll"]` with the `native-code` permission. The host activates them when
the plugin loads and deactivates them when it unloads or is stopped.

## Legacy native plugins

SDK v1 plugins (`"command": "x.exe"`, JSON-RPC 2.0 lines on stdio) still run, one process each,
through `crates/widgets/src/plugins.rs`, unchanged: `initialize`, `face.open/resize/close`,
`action`, `view`, `secret.get/set`, `launch`, plus protocol-1 data `methods`/`events` (the
built-in weather and RSS widgets use these). They aren't sandboxed. New plugins should be
components, and T3, OBS, Docker, Tailscale, Grok and Discord will be ported (see the status
below). The native SDK types (`View`, `Face`, `Manifest`, `protocol`) stay in
`mypaper_plugin_sdk` for them and for the host.

## Status (2026-11-05)

Working: the runtime (`crates/plugins`) with budgets, AOT cache, traps, unload and reload; the
WIT world; the guest SDK; faces through the shell bridge and picker; HTTP; timers; secrets;
storage; launch; system monitors, theme, power and `foreground`; websockets (headers,
subprotocols), pipes and HTTP over pipes; toasts; system-event routing with move coalescing;
`ui.put-image` shared memory; native modules through `mypaper-native-host`; the `hello` and
`weather` components.

Shell side (2026-11-05, late): style patches restyle the top bar, dock and menus in place
(tokens above); top-bar modules from plugins (`custom:<plugin>--<bar>`, below); the `canvas`
renderer and `animated` springs.

Not yet: plugin panels and flyouts opened from a module; dock modules; the `panel` and `widget`
style surfaces; plugin `menu` items in the desktop menu and `commands` as global shortcuts;
plugin modules in Studio's Bars module list (type `custom:<plugin>--<bar>` in `bars.json` for
now); the shell opening `ui.put-image` mappings for canvases (the renderer side,
`views::plugin::canvas::put_image`, is in place; the bridge has to pass the helper's pid). The
shell's system-event sources: `Plugins::system_event` exists, but the window hooks, monitor
changes, notification listener, media provider, power and theme don't call it yet. A toast's
`action` coming back as an event.

### Bar modules

A component plugin's `bars` contribution is placed in the top bar's `left`, `center` or `right`
list as `custom:<plugin>--<bar>` (`custom:hello--clock`), the same placement model as every
module. The shell opens it as the instance `bar:<plugin>--<bar>` (`event::open` with size
`small`) while it's placed, and closes it when it's removed. The bar draws the view's text as
the module's one line (text nodes joined, a `metric`'s or `stat`'s value) in the bar's own
font and colours, so a plugin module looks like a built-in one under any style. A click sends
the view's first action (a `button`'s, `icon-button`'s or `list-row`'s), or `click` when it has
none. A `custom:<id>` with a `command` in `modules` stays a script module.

### Canvas and animated

`canvas { height, commands }` draws in kit points from the canvas's top left, as wide as its
slot and `height` tall, clipped to it: `rect` (fill or stroke, corner radius), `circle`, `line`,
`path` (SVG `M L H V C Q Z`, absolute and relative; fills are even-odd scanlines with a hairline
edge), `text` (a kit text role) and `image` (by `ui.put-image` id; an id not yet available
draws the theme's well). Damage is the kit's display-list diff, so a moved shape repaints only
around itself. `animated(spring)` springs its child's values to their new ones when they change
(a ring's fraction, a limit bar's level, segmented-bar segments, sparkline points) with the
kit's `Tween`; while a spring moves the widget asks for frames, and at rest nothing ticks.

## Native modules

A component that needs a vendor DLL ships a thin C wrapper and its vendor DLLs in
its own plugin folder. Its manifest adds `"native": ["module.dll"]` and the
`native-code` permission. The platform passes those declarations to
`mypaper_native_host::Supervisor`; an undeclared module or missing permission is
refused. Native code has the user's OS privileges: process separation contains
crashes and hangs, and is not an OS security sandbox.

Only `mypaper-native-host.exe` loads DLLs. The shell and the WASM helper never do.
Constructing the supervisor starts no process. `activate` starts a host only for
a plugin with a nonempty native list. Shared hosting is the default;
`Isolation::Plugin` puts all of one plugin's modules in a separate process. After
three failures attributed to a module, it moves to its own host automatically.
For a background crash with no call in flight, all modules in the affected host
are suspects and repeated crashes isolate them individually. Isolation persists
for the supervisor's lifetime. Hosts restart automatically with exponential
backoff (200 ms initially, capped at 10 s by default). Each affected module gets
the `native-module-restarted` event after the replacement finishes initialization.
Calls in flight fail and are never replayed automatically.

`deactivate` stops unused hosts immediately. Because the ABI deliberately has no
shutdown entry point, libraries and callback contexts remain loaded for the
process lifetime. Removing a module from a shared host recycles that host and
reinitializes its remaining active modules, which also receive restarted events.
Dropping the supervisor kills and reaps its children; a kill-on-close Windows Job
also kills them if the WASM helper crashes. The host has no UI and starts with
crash dialogs disabled. Init, call and free share a deadline (default 5 s,
configurable up to 60 s); expiry kills that host only. Calls are serialized by the
supervisor; put it on the platform's blocking worker lane, never the UI/WASM event
thread.

### C ABI 1

The authoritative header is `crates/native-host/include/mypaper_native.h`.
Wrappers export exactly these three C functions, using the host's architecture:

```c
int mp_native_init(const mp_host *host);
int mp_native_call(const char *method, const uint8_t *in, size_t in_len,
                   uint8_t **out, size_t *out_len);
void mp_native_free(uint8_t *out, size_t out_len);
```

`mp_host` contains `abi_version = 1`, an opaque `context`, and
`emit(context, event_name, bytes, len)`. Init/call/free run serially on the dispatch
thread. Emit is safe from any thread, including during init or call; it copies
borrowed bytes before returning and can block on pipe backpressure. Host table
and context remain valid until process exit. Names are NUL-terminated UTF-8;
payloads are opaque bytes. A null byte pointer requires length zero. Modules
return zero for success and nonzero for errors; no exception or unwind may cross
the boundary. A successful call transfers its output allocation to the host,
which copies it and calls the module's free on the same dispatch thread. Failure
must leave output null/zero. See the header for full ownership rules.

The loader accepts DLL basenames only, canonicalizes the plugin directory and
DLL independently, and refuses traversal, absolute names, alternate streams,
subfolders, and symlinks that resolve outside that directory. The host repeats
the client's checks. `LoadLibraryExW` uses an absolute path with
`LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32`; CWD, PATH and
the executable directory are excluded. Vendor dependencies belong next to the
wrapper; Windows system dependencies may come from System32. Do not modify an
installed plugin folder while active. Native permission holders can themselves
load arbitrary code; these checks constrain the broker's loader.

### Native pipe protocol 1

This pipe connects the WASM helper (server) to a disposable native host (client).
It is local-only, single-instance, uniquely named, and verifies the client's PID
against the spawned process. Overlapped reads and writes permit events while a
call or read is pending. All integers are little-endian:

```text
u32 body_len                         // excludes this length, 16..=8 MiB
u8 version = 1
u8 kind                              // load=1, call=2, shutdown=3, ok=4, error=5, event=6
u64 request_id                       // 0 for unsolicited module events
u16 plugin_len; u8 plugin[plugin_len] // UTF-8
u16 module_len; u8 module[module_len] // UTF-8 DLL basename
u16 name_len;   u8 name[name_len]     // method or event name, UTF-8
u8 bytes[remaining]
```

Load payload: `u32 timeout_ms` followed by the UTF-8 canonical plugin directory.
Call payload: `u32 timeout_ms` followed by opaque input. The host clamps deadlines
to 1..=60000 ms. Ok contains opaque output; error contains a UTF-8 diagnostic.
Replies preserve request/plugin/module/name. Event payload is opaque, without a
timeout prefix. Malformed versions, lengths, kinds and UTF-8 are rejected before
dispatch. The client caps inputs at 8 MiB minus framing headroom. Its public event
vendor-event queue is bounded to 256 items; excess vendor events are dropped.
Restart notifications use a separate priority queue so vendor traffic cannot
hide lifecycle changes. Drain the event pump for the supervisor's lifetime.

### Platform integration

Add `mypaper-native-host = { path = "../native-host" }` to `crates/plugins`.
The library uses only Rust std and Win32, with no WASM or vendor dependencies:

```rust,ignore
use mypaper_native_host::{Supervisor, HostConfig, PluginSpec, Isolation};

let native = Supervisor::new(sibling_native_host_exe, HostConfig::default());
native.activate(PluginSpec {
    id: manifest.id.clone(), folder: plugin_folder,
    native: manifest.native.clone(), permissions: manifest.permissions.clone(),
    isolation: Isolation::Shared,
})?;
// Run this on a blocking worker, then complete the WIT request id with its result.
let output = native.call(&manifest.id, "module.dll", "method", &input)?;
while let Some(event) = native.try_event() {
    // Deliver only to event.plugin as WIT event::native (name/module/bytes).
}
native.deactivate(&manifest.id)?; // when the WASM instance is unloaded
```

`recv_event_timeout` supports a dedicated event pump. `hosts()` exposes live PIDs,
module membership and isolation for diagnostics. The helper should own one
supervisor, wrapping it in `Arc` if its I/O workers share it. Connect the manifest's
per-plugin isolation option to `Isolation::Plugin`; default to `Shared`.
`HostConfig` exposes `call_timeout`, `startup_timeout`, `initial_backoff`,
`max_backoff`, and `isolate_after`. Treat `WouldBlock` as temporary restart backoff.

`examples/native-echo` is a dependency-free Rust C-ABI wrapper demonstrating echo,
a background-thread event, a deliberate crash, and a deliberate hang. The native
host integration suite uses real DLLs, child processes and named pipes to test
round-trips, events, permission/containment checks, shared and isolated lifecycle,
crash recovery, automatic isolation, and hang deadlines. It never drives the
user's desktop. Measurements and the reproducible harness live in
`crates/native-host/MEASUREMENTS.md`.
