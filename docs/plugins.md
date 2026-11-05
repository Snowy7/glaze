# Plugins (SDK v1)

A plugin is a small native process that feeds mypaper data and **declarative views**. It never draws
pixels: it sends a view tree built from the kit's vocabulary and the shell draws it with the kit, so
every plugin looks like the rest of mypaper. Plugins depend on one crate, `crates/plugin-sdk`
(`mypaper-plugin-sdk`), and live in `plugins/<id>/` until they move to their own repos.

The older data-only plugins (`weather`, `rss`) speak the same protocol at `protocol: 1` and keep working
unchanged; everything below is additive.

## Folder and manifest

```
plugins/<id>/            bundled, built with the workspace
%APPDATA%\mypaper\plugins\<id>\   installed (downloaded)
  plugin.json
  <id>.exe
```

`plugin.json` (parsed by `mypaper_widgets::manifest::PluginManifest`; the SDK has the same types as
`mypaper_plugin_sdk::Manifest` for plugins and tooling):

```json
{
  "id": "t3", "name": "T3 Code", "version": "0.1.0",
  "sdk": "1.0",
  "command": "t3.exe",
  "faces": [
    { "id": "running", "name": "Running now", "size": { "default": [2, 2], "min": [1, 1], "max": [4, 4] } },
    { "id": "limits", "name": "Limits", "size": { "default": [2, 2] } }
  ],
  "permissions": ["secrets", "process:t3", "network:ws://127.0.0.1", "launch", "notifications"],
  "settings": []
}
```

- `sdk`: the SDK major.minor the plugin was built against. The host refuses a plugin whose major isn't
  the host's (`mypaper_plugin_sdk::SDK_MAJOR`); a missing `sdk` means a protocol-1 data plugin.
- `faces`: widgets the plugin offers. They appear in the "+" picker as `<plugin>.<face>`; a placed
  instance keeps its instance id, so layouts survive a provider moving from built-in to plugin.
- `panels`, `bars`: reserved for panel and bar-module faces (same shape as `faces`).
- `settings`: the same setting schema widgets use (`docs/widgets.md`).

## Process model and lifecycle

- The shell starts a plugin **lazily**, when a face of it is placed (or a data call arrives), inside a
  kill-on-close Job object, and stops it 30 s after its last face closes. Nothing placed → no process.
- A crash fails pending calls, shows the last view with a "Restarting…" footer, and restarts after
  2 s, doubling per crash up to a minute. A crash never reaches the shell.
- While the desktop is held (fullscreen app, locked), the host sends `suspend`; `resume` when it's back.
- The shell caches each instance's last view tree and redraws from it; a plugin that sends nothing
  costs no frames ("nothing changes → nothing drawn").

## Protocol

JSON-RPC 2.0, one JSON object per line on stdio (`\n`-framed; serde escapes newlines in strings).

Host → plugin:

| message | kind | params |
|---|---|---|
| `initialize` | request | `{ protocol: 2, sdk: "1.0", plugin, dataDir }` → `{ sdk: "1.0" }` |
| `shutdown` | request | — (exit promptly) |
| `suspend`, `resume` | notification | — |
| `face.open` | notification | `{ instance, face, size: "small"\|"medium"\|"large", settings }` |
| `face.resize` | notification | `{ instance, size }` |
| `face.close` | notification | `{ instance }` |
| `action` | notification | `{ instance, action, value? }` (button id; `value` for text fields) |
| any declared `methods` | request | protocol-1 data calls, unchanged |

Plugin → host:

| message | kind | params |
|---|---|---|
| `view` | notification | `{ instance, tree: View }`: replaces the instance's view |
| `event` | notification | `{ event, data }` (protocol 1, unchanged) |
| `secret.get` | request | `{ key }` → `{ value: string\|null }` (needs `secrets`) |
| `secret.set` | request | `{ key, value: string\|null }` (`null` deletes; needs `secrets`) |
| `launch` | request | `{ target }` opens a URL/file/app (needs `launch`) |
| `notify` | request | `{ title, body }` toast (needs `notifications`) |

A request the plugin may not make is answered with error `-32003` and `data.code: "PermissionDenied"`;
an unknown one with `-32601`.

## Views

A `View` is a tree of kit nodes, serialized with `"kind"` tags. Sizes are in kit points; the host
scales them. Colors are **roles** (`label`, `secondary`, `tertiary`, `accent`, `warning`,
`destructive`, `positive`) or an explicit `#rrggbb` for brand fills, so themes keep working.

| kind | fields |
|---|---|
| `stack` / `row` | `children`, `gap?` (vertical / horizontal) |
| `grid` | `children`, `columns`, `gap?` |
| `text` | `text`, `role` (`TextRole`: `title`, `headline`, `subheadline`, `body`, `callout`, `caption`, …), `color?`, `align?` |
| `icon` | `icon` (kit icon name), `size?`, `color?` |
| `button` | `label`, `action`, `icon?`, `primary?` |
| `field` | `placeholder`, `action` (sends the text as `value` on submit) |
| `list_row` | `title`, `detail?`, `icon?`, `trailing?`, `action?` |
| `stat` | `label`, `value`, `detail?` |
| `ring` | `fraction`, `label?`, `color?` |
| `limit_bar` | `label`, `fraction` (used), `pace?`, `caption?`, `color?` |
| `segmented_bar` | `segments: [{ fraction, color }]` |
| `sparkline` | `values`, `color?` |
| `area_chart` | `series: [{ label, values, color? }]`, `height?` |
| `empty` | `icon?`, `title`, `detail?` |
| `spacer` | — (takes the free space) |

A face sends one tree per size class (`small`, `medium`, `large`); the host asks with `face.open`/
`face.resize`. Every face is wrapped in the standard integration chrome (icon + title header,
footer), so a view describes only the body. `footer?` on the root sets the footer caption.

## Permissions

Declared in `plugin.json`, shown before install, and enforced by the host for everything that goes
through it:

| permission | grants |
|---|---|
| `secrets` | `secret.get`/`secret.set`, stored in Windows Credential Manager as `mypaper/plugin/<id>/<key>` |
| `launch` | `launch` |
| `notifications` | `notify` |
| `network:<origin>` | outbound connections to that origin |
| `process:<name>` / `pipe:<name>` | local-app access: running that CLI, opening that named pipe |

Plugins are native code. The host enforces the brokered capabilities above (secrets, launch,
notifications) and refuses undeclared requests; `network:`, `process:` and `pipe:` are declared,
shown, and checked by the SDK's helpers (`Host::allows`), but a plugin that bypasses the SDK isn't
sandboxed yet (AppContainer is the follow-up for downloaded plugins).

## Secrets

`host.secret("key")` / `host.set_secret("key", Some(value))`. The host namespaces keys per plugin, so
one plugin can't read another's. Values never touch disk in plain text or the log.

## Timers

`ctx.after(id, Duration)` / `ctx.every(id, Duration)` schedule `Event::Timer(id)` inside the SDK's
event loop. Plugins stay event-driven: they wake on host messages, their own I/O, or a timer, and
send a `view` only when what they show changed.

## Versioning

`SDK_MAJOR` / `SDK_MINOR` in `mypaper-plugin-sdk`. Minor bumps only add optional fields and view
kinds (an unknown view kind draws as nothing, and an unknown field is ignored), so older plugins keep
working. A major bump is refused by older hosts at load, with a "needs a newer mypaper" note in the
picker.

## Writing one

```rust
use mypaper_plugin_sdk::{Ctx, Event, Plugin, View, run};

struct Hello;
impl Plugin for Hello {
    fn on_event(&mut self, ctx: &mut Ctx, event: Event) {
        if let Event::Open { instance, .. } = event {
            ctx.view(&instance, View::text("Hello", "headline"));
        }
    }
}
fn main() { run(Hello) }
```
