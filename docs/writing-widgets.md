# Writing widgets

A widget is a folder with a `widget.json` and a web page. No build step is required, and mypaper
doesn't need rebuilding or restarting to pick up a new folder. The contract is
[widgets.md](widgets.md); this is the practical guide. The six bundled widgets in `widgets/` are
complete, small examples: `clock` is the simplest, `system-monitor` and `weather` show responsive
layouts and subscriptions.

## 1. A widget in two files

Create `%APPDATA%\mypaper\widgets\hello\` (or `widgets/hello/` in the repo for a bundled one).

`widget.json`:

```json
{
  "$schema": "../../packages/widget-sdk/schema/widget.schema.json",
  "id": "hello",
  "name": "Hello",
  "version": "1.0.0",
  "entry": "index.html",
  "size": { "default": [2, 1], "min": [1, 1], "max": [4, 2] },
  "permissions": [],
  "settings": [{ "key": "name", "type": "string", "label": "Name", "default": "world" }]
}
```

`index.html`:

```html
<!doctype html>
<meta charset="utf-8" />
<style>
  html, body { margin: 0; height: 100%; background: transparent; font: 16px "Segoe UI", sans-serif; color: #fff; }
  body { display: grid; place-items: center; }
  main { position: absolute; inset: 0; display: grid; place-items: center;
         background: rgb(16 16 16 / 0.72); border-radius: 20px; backdrop-filter: blur(24px); }
</style>
<main id="out">...</main>
<script src="mypaper.js"></script>
<script>
  mypaper.settings.get().then((s) => (out.textContent = `Hello, ${s.name}`));
  mypaper.settings.onChange((s) => (out.textContent = `Hello, ${s.name}`));
</script>
```

Rules for the folder:

- `id` is lowercase `[a-z0-9-]` and equals the folder name.
- A widget is served from its **own origin** (`https://<id>.widget.mypaper/`) and can't load files
  from outside its folder, so keep everything it needs inside it (including `mypaper.js`).
- The page background must be **transparent**: the WebView is. Only paint what you want to see.
- Add `"$schema"` to get validation and completion in your editor. The host ignores it.

### Getting `mypaper.js`

`mypaper.js` is the drop-in build of the SDK: a single 3 KB script that defines the global
`mypaper`. Build it once and copy it next to your `index.html`:

```text
cd packages/widget-sdk
pnpm install
pnpm build          # dist/mypaper.js, and refreshes the copies in widgets/*/
```

Or, with your own bundler and TypeScript, import the SDK instead:

```ts
import { connect } from "@mypaper/widget-sdk";
const mypaper = connect(); // once per page
await mypaper.ready;
```

Plain JS widgets can still get types: add `// @ts-check` and include
`packages/widget-sdk/types/global.d.ts` in your `jsconfig.json`/`tsconfig.json`. All bundled
widgets are written that way.

## 2. Developing without mypaper: the mock harness

The harness is a Vite page that loads any widget folder in an iframe and plays the shell and the
host with fake data, so you can work in a browser tab (hot reload included).

```text
cd packages/widget-sdk
pnpm install
pnpm dev                                   # http://localhost:5173
$env:MYPAPER_WIDGETS = "C:\path\to\my\widgets"; pnpm dev   # your own folder(s), besides the repo's widgets/
```

`MYPAPER_WIDGETS` is a path list (`;` on Windows). Each entry is either a widget folder or a folder
of widget folders. Saving a file in a widget reloads it.

What it emulates, and how to poke at it (the panel on the left):

| | |
|---|---|
| **Size** | Set the cell size within your manifest's `min` and `max`; also the cell px, gap and DPI. Your `size.onChange` fires like in the shell. |
| **Host state** | **Visible** off is "the host is held" (`visibility: false`, subscriptions stop). **Edit mode** sends `editmode`. |
| **Settings** | A form generated from your manifest, including `list`. Changes are pushed live and remembered per widget in this browser. |
| **Permissions** | Enforced from your manifest: calling something you didn't declare rejects with `PermissionDenied`, as in the real host. |
| **Data** | Fake `system.stats` (a random walk), a fake media player with artwork and working transport, themes with and without a palette. `launch` shows a toast. |
| **Storage** | In memory, per instance; viewable and clearable in the panel. |
| **Plugins** | **Mock** (default) or **real**. See below. |
| **Wire log** | The last messages in both directions. |

Handy URL parameters: `widget`, `cols`, `rows`, `cell`, `gap`, `dpi`, `bg` (`dark`, `light`,
`photo`, `checker`: check your widget over a busy wallpaper), `hidden=1`, `edit=1`,
`settings=<json>`, `storage=<json>` (seed the instance storage), `media=playing|paused|none`,
`theme=<name>`, `plugin=mock|real`, and `ui=0` (hide the panel, for screenshots).

**Plugins.** In *mock* mode the weather plugin is built in, and any widget can ship canned
answers in `mypaper.mock.json` next to its `widget.json`:

```json
{ "my-plugin": { "current": { "value": 42 } } }
```

In *real* mode, calls go to the actual plugin process, found in the repo's `plugins/` (or
`MYPAPER_PLUGINS`, same format as above). Its executable is looked up next to `plugin.json`, then
in `target/release` and `target/debug`, so `cargo build -p mypaper-plugin-weather` is enough. The
plugin gets `suspend`/`resume` when you toggle **Visible**, and stops after 30 s idle, like in the
host.

What it can't show: the real per-widget origin, `DENY_CORS`, the network filter for
`network:<origin>` and the real click-through behavior. Test those in mypaper.

## 3. The API in practice

```js
await mypaper.ready;                      // instance and widget are valid after this
mypaper.instance;                         // "hello-1"

// Settings: defaults come from your manifest; the user's values arrive at load and on every change
const settings = await mypaper.settings.get();
const off = mypaper.settings.onChange((s) => apply(s));   // every subscription returns an unsubscribe

// Size, in cells and CSS pixels
const { cells, px, dpi } = await mypaper.size.get();
mypaper.size.onChange(({ cells, px }) => layout(px));

// Lifecycle
mypaper.visible;                          // current value
mypaper.on("visibility", (visible) => (visible ? start() : stop()));
mypaper.on("editmode", (editing) => {});  // pointer events are the shell's while editing

// Per-instance storage (JSON values); `undefined` when missing
await mypaper.storage.set("text", "hi");
const text = await mypaper.storage.get("text");

// "system.stats"
const stop = mypaper.system.subscribe(2000, (s) => draw(s.cpu, s.memory, s.gpu, s.net)); // min 1000 ms

// "media"
const now = await mypaper.media.current();              // null when nothing plays
mypaper.media.onChange((m) => {});
await mypaper.media.control("toggle");                  // play, pause, toggle, next, previous

// "launch": a path or URL, opened like Win+R would (no admin)
await mypaper.launch("https://github.com");

// "theme"
const { name, palette } = await mypaper.theme.current(); // palette may be missing

// plugins listed in the manifest
const w = await mypaper.plugin("weather", "current", { city: "Oslo" });
mypaper.onPlugin("weather", "updated", () => refresh());
```

Errors reject with a `MypaperError` whose `code` is one of `PermissionDenied`, `NotFound`,
`InvalidParams`, `PluginUnavailable`, `Internal`. With the ES module:

```ts
import { isMypaperError } from "@mypaper/widget-sdk";
try { await mypaper.plugin("weather", "current", { city }); }
catch (e) { if (isMypaperError(e, "NotFound")) showHint("Unknown city"); else throw e; }
```

Subscriptions (`system.subscribe`, `media.onChange`, `theme.onChange`, `onPlugin`) can't reject
(they return the unsubscribe function straight away), so a denied one is logged to the console
instead. The harness's wire log shows why.

## 4. Settings

`settings` in the manifest are rendered by the shell (and Studio) as a form. Types: `boolean`,
`string`, `color`, `select`, `number` and `list` (a list of records, like the launcher's
shortcuts). Treat the values as untrusted: they come from a JSON file the user can edit. The
bundled widgets keep only values whose type matches the default's (`kit.js`: `trackSettings`).

Use `string` when "empty" must be a valid value (the weather widget's optional latitude and
longitude): a `number` setting always has a number.

## 5. Layout and style

Your widget is exactly the size of its cells, and it changes while the user drags the handles in
edit mode. The most robust approach is CSS container units; the page itself is the container:

```css
body { container-type: size; }
.time { font-size: min(34cqh, 17cqw); }
@container (min-aspect-ratio: 3/1) { .card { flex-direction: row; } }
```

Use `size.onChange` for what CSS can't do: choosing how many items to show, resizing a canvas.
Make sure the widget works at **every** size between its manifest's `min` and `max`: check the
corners in the harness.

The house style of the bundled widgets (copy `packages/widget-sdk/widget-kit/base.css` and `kit.js`
into your folder to use it):

- Monochrome: neutral grays with clear steps (`--t0` white for emphasis, `--t1`, `--t2`, `--t3`),
  no accent hues. Offer the wallpaper's accent as an opt-in setting (`useWallpaperAccent`, needs
  `theme`).
- A soft translucent dark surface with `backdrop-filter`, a hairline border and generous radius.
- Segoe UI Variable, tabular numbers for anything that changes, light weights for big numbers.

## 6. Permissions

Anything not listed in `permissions` is denied.

| permission | gives |
|---|---|
| `system.stats` | `mypaper.system.*` |
| `media` | `mypaper.media.*` |
| `launch` | `mypaper.launch` |
| `network:<origin>` | `fetch` to that origin (for example `network:https://api.open-meteo.com`) |
| `theme` | `mypaper.theme.*` |
| `clipboard.write` | reserved: no `mypaper` call uses it yet |
| `plugins: [...]` (not a permission, a separate list) | calls to those plugins |

Ask for the least you need: users see the list.

## 7. Performance rules

Many widgets are on screen all day, and a held host (fullscreen game, locked, display off) must
cost nothing.

- **Stop everything on `visibility: false`**: timers, `requestAnimationFrame` loops,
  subscriptions, pending retries. Start again on `true`. `kit.js` has `onVisible(start, stop)` and
  `every(ms, fn)` (runs now, then each interval, only while visible). CSS animations are paused
  too in `base.css` (`html[data-hidden]`), but prefer not to have infinite ones.
- **No busy loops.** Never poll in a tight `setInterval`. Align clocks to the boundary they show:
  a clock without seconds wakes once a minute, with `setTimeout` to the next minute.
- **Don't poll `system.stats`.** Use `system.subscribe`: the host drives one timer for all widgets.
  Pick the slowest interval that looks right (2000 ms for a sparkline).
- **Use `requestAnimationFrame` only while something animates**, and stop it when the animation
  ends. A progress bar that moves once a second needs a 1 s timer (and only while playing), not a
  frame loop.
- **Cache and debounce.** Save notes after a pause in typing, not per keystroke, and flush when
  hidden. Fetch network data at most every few minutes (the weather plugin caches for 10).
- **Keep it small.** No frameworks for a clock. Avoid large `filter: blur()` areas and big images;
  decode artwork once and reuse it.
- **Unsubscribe what you subscribed**, especially when the user changes a setting that makes a
  subscription pointless.
- Respect `prefers-reduced-motion`.

## 8. Checking your work

- Manifest: the `$schema` line in your editor, or the `test/manifests.test.ts` approach in
  `packages/widget-sdk` (Ajv against the schema).
- Types: `// @ts-check` in your JS with `global.d.ts`.
- Layout: the harness at your min, default and max size, over the `photo` and `checker` backgrounds,
  hidden on and off.
- Headless screenshots, without opening a window, for a quick look:
  `msedge --headless=new --screenshot=out.png --window-size=248,144 "http://localhost:5173/?ui=0&widget=hello&cols=2&rows=1"`.
