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
  "permissions": ["network:https://api.open-meteo.com", "timers", "storage"],
  "limits": { "memory_mb": 16 }
}
```

- `component` is the `.wasm` component, relative to the folder. A manifest with `component` is a
  WASM plugin, and one with `command` is a [legacy native plugin](#legacy-native-plugins). A
  transitional plugin may have both (the weather plugin keeps its exe for the built-in widget's
  data calls while its faces run as a component).
- `api` is the WIT package major.minor it was built against. The host refuses another major.
- The contributions are `faces` (desktop widgets in the "+" picker, as `<plugin>.<face>`),
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

1. **Install.** The host validates the manifest, checks that the component's imports match its
   permissions, and **AOT-compiles** it to `%LOCALAPPDATA%\mypaper\plugins\cache\<id>-<hash>.cwasm`.
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
`notify`, `secrets`, `storage`, `system`, `native` and `log`. It exports `lifecycle`. Each
import is a separate interface, so a component that doesn't use one doesn't import it, and the
install check compares the imports against the permissions.

| interface | functions | permission |
|---|---|---|
| `lifecycle` (export) | `init(info) -> result<_, string>`, `on-event(event)`, `save() -> list<u8>` | — |
| `ui` | `set-view(instance, face)`, `clear(instance)` | — |
| `style` | `apply(patch) -> result<_, error>`, `reset(surface)` | the surface in `styles` |
| `timers` | `after(id, ms)`, `every(id, ms)`, `cancel(id)` | — |
| `events` | `subscribe(list<topic>)`, `unsubscribe(list<topic>)` | `windows` for window topics |
| `http` | `fetch(request) -> result<u64, error>`, with the reply arriving as `event::http` | `network:<origin>` |
| `websocket` | `connect(url) -> result<u64, error>`, `send`, `close`, with `event::socket` | `network:<origin>` |
| `pipes` | `open(name) -> result<u64, error>`, `write`, `close`, with `event::pipe` | `pipe:<name>` |
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

Token names are the theme's (`docs/ui.md` › "Tokens"). The shell applies a patch to its surface
through the same path as the user's bar and theme settings, so it themes, animates and follows
DPI like a user change. The user's own setting for the same token wins, and a plugin can't
override an explicit choice.

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

## Shell ↔ helper protocol

The bridge starts `mypaper-plugins.exe` lazily when the first WASM plugin's face opens. It runs
inside the shell's kill-on-close Job object with the same restart backoff as other plugins, and
it stops when the last plugin unloads. Frames are `[u32 len][postcard(HostMsg|HelperMsg)]`
(`mypaper_plugin_api::wire`):

- shell → helper: `Open { plugin, instance, face, size, settings }`, `Resize`, `Close`,
  `Action { instance, action, value }`, `Command { plugin, id }`, `Held(bool)`, `Shutdown`.
- helper → shell: `View { instance, face }` (the kit tree, changed faces only), `Status
  { plugin, status }`, `Style { plugin, patch }`, `Unloaded { plugin }`, `Stats { plugin,
  memory, fuel_ms }`.

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
use mypaper_plugin_sdk::guest::{self, Event, Plugin, View, Role, export};

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

Build it with `cargo build --release --target wasm32-wasip2` and put the `.wasm` beside
`plugin.json`. `scripts/deploy-local.ps1` builds and bundles the in-repo plugins.

## Legacy native plugins

SDK v1 plugins (`"command": "x.exe"`, JSON-RPC 2.0 lines on stdio) still run, one process each,
through `crates/widgets/src/plugins.rs`, unchanged: `initialize`, `face.open/resize/close`,
`action`, `view`, `secret.get/set`, `launch`, plus protocol-1 data `methods`/`events` (the
built-in weather and RSS widgets use these). They aren't sandboxed. New plugins should be
components, and T3, OBS, Docker, Tailscale, Grok and Discord will be ported (see the status
below). The native SDK types (`View`, `Face`, `Manifest`, `protocol`) stay in
`mypaper_plugin_sdk` for them and for the host.

## Status (2026-11-05)

The runtime, WIT world, guest SDK, `hello` and the weather component are in place. Window,
monitor, media and notification topics, websockets and the canvas renderer are wired in WIT but
stubbed in the host (they return `error::unsupported` or never fire). See the "Stubbed" list
in the host's `README` section of `crates/plugins/src/main.rs`.
