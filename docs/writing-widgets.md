# Writing widgets

A widget is a folder with a `widget.json` and a web page. No build step is required, and mypaper
doesn't need rebuilding or restarting to pick up a new folder. The contract is
[widgets.md](widgets.md); this is the practical guide. The bundled widgets in `widgets/` are complete
examples: `clock` shows faces behind one `variant` setting, `system-monitor` and `weather` responsive
layouts, subscriptions and a `place` setting, `now-playing` the audio feed (its Visualizer look),
`launcher` real app icons. The rest (calendar, timer, todo, notes, rss and more) are good references
for storage, lists, plugins and permissions.

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
<link rel="stylesheet" href="base.css" />
<main class="card" id="out" style="display: grid; place-items: center; font-size: 1.6em">...</main>
<script src="mypaper.js"></script>
<script>
  mypaper.settings.get().then((s) => (out.textContent = `Hello, ${s.name}`));
  mypaper.settings.onChange((s) => (out.textContent = `Hello, ${s.name}`));
</script>
```

`base.css` (see [Getting the kit](#getting-mypaperjs-and-the-kit)) brings the tokens and the optional
background, so this widget already looks right with no background, glass or solid, light or dark
text. There is nothing to paint yourself.

Rules for the folder:

- `id` is lowercase `[a-z0-9-]` and equals the folder name.
- A widget is served from its **own origin** (`https://<id>.widget.mypaper/`) and can't load files
  from outside its folder, so keep everything it needs inside it (including `mypaper.js`).
- The page background must be **transparent**: the WebView is. Only `.card` paints, and only when
  the user chose a surface (see [Appearance](#5-appearance-and-style)).
- Add `"$schema"` to get validation and completion in your editor. The host ignores it.

### Getting `mypaper.js` and the kit

`mypaper.js` is the drop-in build of the SDK: a single small script that defines the global
`mypaper` and applies the user's appearance to the page (see below). Build it once and copy it next
to your `index.html`, together with the kit (`base.css`: tokens and the optional surface; `kit.js`:
helpers like `trackSettings`, `trackSize`, `every`):

```text
cd packages/widget-sdk
pnpm install
pnpm build          # dist/mypaper.js, and refreshes the copies of mypaper.js, base.css and kit.js in widgets/*/
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
| **Appearance** | Every field of the user's appearance (surface, text tone, tint, opacity, blur, radius, accent, scale, border, text shadow) and a box for theme CSS. Changes are pushed live as the `appearance` event. |
| **Host state** | **Visible** off is "the host is held" (`visibility: false`, subscriptions stop). **Edit mode** sends `editmode`. |
| **Settings** | A form generated from your manifest, including `list`. Changes are pushed live and remembered per widget in this browser. |
| **Permissions** | Enforced from your manifest: calling something you didn't declare rejects with `PermissionDenied`, as in the real host. |
| **Data** | Fake `system.stats` (a random walk, plus battery, uptime, network and three drives; the panel switches the battery between none, discharging and charging, and the network between wifi, ethernet and none), a fake media player with artwork and working transport, themes with and without a palette. `launch` shows a toast. |
| **Storage** | In memory, per instance; viewable and clearable in the panel. |
| **Plugins** | **Mock** (default) or **real**. See below. |
| **Wire log** | The last messages in both directions. |

Handy URL parameters: `widget`, `cols`, `rows`, `cell`, `gap`, `dpi`, `bg` (`dark`, `light`,
`photo`, `photo-dark`, `photo-light`, `checker`: check your widget over a busy wallpaper),
`hidden=1`, `edit=1`, `settings=<json>`, `storage=<json>` (seed the instance storage),
`media=playing|paused|none`, `battery=none|discharging|charging`, `net=wifi|ethernet|none`,
`shared=<json>` (seed shared storage: `{"activity":{"todo":{"2026-11-02":3}}}`), `batterypct=<0..100>`,
`theme=<name>`, `plugin=mock|real`, the appearance (`style=minimal|depth|neon|paper|aero`,
`surface=none|glass|solid`, `text=light|dark`,
`scale=<0.5..2>`, `shadow=0|1`, `opacity`, `blur`, `radius`, `accent=<rrggbb>`, `tint=<rrggbb>`, or
`appearance=<json>` with any field), `themecss=<css>`, and `ui=0` (hide the panel, for screenshots).

**The gallery** shows everything at once: `http://localhost:5173/gallery.html?widget=hello` is a
contact sheet with one live copy of the widget per surface (`none`, `glass`, `solid`, as rows) and
size (`min`, `default`, `max`, as columns), over a dark and a light photo-like background (dark text
is picked for the light one). Options: `sizes=min,default,max` or `2x1,4x2`, `surfaces=none,glass`,
`bgs=photo-dark,photo-light`, `text=light|dark`, `scale`, `speed=40` (makes `system.subscribe` tick
40 times faster, so sparklines fill up within a screenshot), plus the data and appearance parameters
above. `styles=minimal,depth,…|all` makes the rows style x surface. **The wall**,
`http://localhost:5173/wall.html?style=neon&surface=glass&bg=photo-dark`, shows every widget at once
at one size (`size=default|min|max|3x2`, `only=a,b`, `skip=a`, `settings={"<id>":{…}}`), one headless
screenshot to review the whole set in a style. The mock harness also fakes the audio spectrum
(while *Visible* is on), `user.info` ("Ada Lovelace"), `appIcon` (generated tiles), `pickFile` (a
browser file picker; the URLs only last until reload) and shared storage. `node scripts/shot.mjs <name> "/gallery.html?widget=hello" 1400x900` (in `packages/widget-sdk`,
with `MYPAPER_HARNESS=http://localhost:5173`) takes a headless Edge screenshot of any harness URL and
prints the png's path.

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

// Appearance: mypaper.js already applies it to <html> (see "Appearance and style"); read it only
// for what CSS can't do. Needs no permission.
const { appearance, themeCss } = await mypaper.appearance.get();   // appearance.surface === "none" ...
mypaper.appearance.onChange(({ appearance }) => redrawCanvas());

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

// "audio": 64..128 floats in 0..1 (low to high frequency), ~30 per second while you are visible
const stopAudio = mypaper.audio.spectrum((bins) => draw(bins), { bins: 96 });

// "user": the signed-in user's display name
const { name, account } = await mypaper.user.info();

// The system file dialog (no permission: the dialog is the consent). The host copies the file into
// your data folder and gives you a URL served from your own origin; keep it in storage.
const picked = await mypaper.pickFile({ extensions: ["jpg", "png"], title: "Choose a photo" });
if (picked) img.src = picked.url;      // null when the user cancelled

// "launch": the icon of an exe, file, folder or shell:AppsFolder\<AUMID>, as a PNG data URL (or null)
const icon = await mypaper.appIcon("C:\\Windows\\System32\\notepad.exe", 64);

// "storage.shared:<ns>": a storage namespace shared with every widget declaring the same permission
const activity = mypaper.storage.shared("activity");
const todo = await activity.get<Record<string, number>>("todo");
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
`string`, `color`, `select`, `number`, `slider`, `font`, `file`, `image` and `list` (a list of
records, like the launcher's shortcuts). Treat the values as untrusted: they come from a JSON file
the user can edit. The bundled widgets keep only values whose type matches the default's
(`kit.js`: `trackSettings`).

Use `string` when "empty" must be a valid value (the weather widget's optional latitude and
longitude): a `number` setting always has a number.

The richer types (the full table is in [widgets.md](widgets.md#widget-manifest-widgetjson)):

```json
{ "key": "variant", "type": "select", "label": "Layout", "group": "Layout", "default": "flip",
  "options": [ { "value": "desk", "label": "Desk", "icon": "▭" },
               { "value": "flip", "label": "Flip", "icon": "▤" } ] },
{ "key": "size", "type": "slider", "label": "Size", "min": 50, "max": 150, "step": 5, "unit": "%", "default": 100 },
{ "key": "glow", "type": "color", "label": "Glow", "alpha": true, "default": "#7aa2ffcc" },
{ "key": "face", "type": "font", "label": "Font", "default": "Segoe UI Variable Display" },
{ "key": "photo", "type": "image", "label": "Photo", "default": "" }
```

- `slider` previews live in your widget while it is dragged (a `settings` event per move, nothing
  saved until the user lets go), so apply settings changes cheaply and idempotently.
- `color` with `alpha` gives `#rrggbbaa`; use it as is in CSS.
- `image`/`file` values are `""` or a URL on your own origin that the host serves; use them in
  `<img src>` or CSS `url()`. Never assume the file still exists: handle `onerror`.
- `group` puts settings under a heading; keep a group's settings next to each other.
- **Layout variants**: give a showpiece widget 2 or 3 layouts with a `variant` select, set
  `document.documentElement.dataset.variant` from it, and let CSS do the rest
  (`html[data-variant="flip"] .time { … }`), so a switch costs nothing.

## 5. Appearance and style

### The user decides how a widget looks

Users and themes restyle **every** widget without touching its code: the surface behind it, the
text tone, the accent, the font and the scale. (The fields and layers are in
[widgets.md, Appearance](widgets.md#appearance).) Your job is to use the variables, never to pick
colors. `mypaper.js` applies the appearance to your page by itself; `base.css` turns it into
a card and into legible text. You get:

| On `<html>` | Values | Use |
|---|---|---|
| `data-surface` | `none` (the default), `glass`, `solid` | Rules like `html[data-surface="none"] .foo { ... }`. |
| `data-text` | `light` (for dark wallpapers), `dark` | Rarely needed: the tokens already flip. |
| `data-shadow` | `on`, `off` | `base.css` adds the text shadow; you don't. |
| `data-widget` | your id | Theme CSS targets `[data-widget="clock"]`. |

| Variable | Is |
|---|---|
| `--t0` .. `--t3` | Text, from emphasis to quietest. Always use these for text. |
| `--fill-1` .. `--fill-3` | Fills and hairlines on top of what's behind (bars, tracks, dividers, hover). |
| `--line` | The card's border (transparent when the user turned borders off). |
| `--accent`, `--on-accent` | The accent, and the ink that reads on it. Use sparingly: a progress fill, a ring, one highlight. |
| `--surface`, `--blur` | The card's background and blur. `.card` already uses them. |
| `--radius` | Corner radius. `.card` uses it; use `calc(var(--radius) * 0.6)` for things inside. |
| `--ink`, `--halo` | `R G B` triples of the ink and its opposite, for custom alphas: `rgb(var(--ink) / 0.3)`. |
| `--chip` | A small pill's background: a soft dark wash on a bare wallpaper, a plain fill on a surface. |
| `--font`, `--scale` | The font family list, and the scale. Both are applied for you. |

The **surface is `none` by default**: no card, the text sits straight on the wallpaper. That is the
case to design for first. Then `glass` and `solid` have to look just as good.

The classes in `base.css` that do the work:

| Class | Is |
|---|---|
| `.card` | The widget's root box, absolutely filling the page. Paints the surface, border and blur only for `glass` and `solid`. |
| `.panel` | A group inside the card: a quiet fill with a surface, **nothing at all** on a bare wallpaper (spacing and hairlines group things there instead). |
| `.chip` | A pill (label, value, small button). |
| `.field` | A text input; a small frosted pill on a bare wallpaper. |
| `.track` and `.track > i` | A progress track and its fill (`width` or `transform` on the `i`). The track gets a faint outline on a bare wallpaper so it shows on any photo. |
| `.rule` | A hairline divider. |
| `.num`, `.caps`, `.dim`, `.ellipsis` | Tabular numbers, small caps labels, quiet text, truncation. |
| `.gfx` | Gets the same soft drop-shadow as `<svg>` does on a bare wallpaper; put it on canvases and other graphics. |

**Do:**

- Style only with the variables above: `color: var(--t1)`, `background: var(--fill-1)`,
  `stroke: var(--accent)`.
- Keep graphics thin and soft: strokes of 1.5 to 2 px, round caps, no hard edges. On a bare wallpaper
  `<svg>` and `.gfx` get a soft shadow for free; don't add outlines.
- Group with spacing and hairlines (`.rule`, `--fill-2`), not boxes. Use `.panel` for the boxes you
  do want with a surface.
- Use `--radius` for corners, so the user's radius choice reaches your inner elements.
- Make contrast with the *steps* (`--t0` for the one thing that matters, `--t1`, `--t2` for labels),
  not with colors. Use `--accent` once or twice per widget.
- Check every widget at all three surfaces, light and dark text, and over a dark and a light photo
  (the gallery does this in one screenshot), at its smallest and largest size and at `scale` 0.7 and 1.5.

**Don't:**

- Never hard-code a background, a color or a gradient: no `#fff`, `rgb(...)`, `black`, `background:
  rgba(...)` on your own elements. The only colors a widget owns are artwork and user data (a cover
  image, a coin's red or green if you choose).
- Never paint the page: `html` and `body` stay transparent, and `.card` is the only thing that
  paints, and only with a surface. Don't give `.card` a background or border of your own.
- Don't change `color-scheme`: `base.css` pins it to `dark` for every tone, because the shell's
  iframes are `dark` and Chromium paints an opaque backdrop behind a frame whose scheme differs
  (square dark corners behind a rounded glass widget). Don't flip it for `data-text="dark"`.
- Don't add your own text shadows (stacked on the shared one they turn muddy), and don't rely on a
  surface for legibility: with `none` the wallpaper is behind your text.
- Don't scale things yourself for the user's `scale`: lay out at the size you're given
  (`trackSize` from `kit.js` already reports the box divided by the scale) and the page is
  scaled for you.
- Don't read the appearance to pick your own colors. `mypaper.appearance.get()` exists for
  what CSS can't do (say, a canvas), and even then read the CSS variables with `getComputedStyle`.

The theme's widget CSS arrives last in the page, so a theme can override any of the above, or
restyle one widget through `[data-widget="<id>"]`.

### Layout

Your widget is exactly the size of its cells, and it changes while the user drags the handles in
edit mode. The most robust approach is CSS container units; the page itself is the container:

```css
body { container-type: size; }     /* base.css already does this */
.time { font-size: min(34cqh, 17cqw); }
@container (min-aspect-ratio: 3/1) { .card { flex-direction: row; } }
```

Use `size.onChange` (or `trackSize` in the kit) for what CSS can't do: choosing how many items to
show, resizing a canvas. Make sure the widget works at **every** size between its manifest's `min`
and `max`: check the corners in the harness or the gallery.

`trackSize` reports the *page* box (the widget's pixels divided by `scale`), and may report once
with a scale of 1 before the appearance arrives, then again. It knows nothing about your padding or
your inner elements. When you need the real size of an inner element (a list that should show as
many rows as fit), observe it with a `ResizeObserver` instead, as `world-clock` and the stats
widgets do.

### Styles (design system v2)

The user can also pick a **style** (`data-style` on `<html>`): `minimal` (today's flat look, the
default), `depth` (raised cards under a key light, bevels, soft layered shadows), `neon` (glowing
gradient outlines), `paper` (soft matte with a grain) and `aero` (glossy glass with a specular
sheen). The kit does the work: stick to the kit's building blocks and your widget is right in all
five. Concretely:

| Use | For |
|---|---|
| `.card` | the root (the style lights it; with a surface it is inset by `--bleed` so shadows fit, and its padding shrinks by the same amount so your content does not move) |
| `.panel` | a quiet group, visible only with a surface |
| `.tile` | a group that paints in every style except `minimal`, **even on a bare wallpaper** (hourly slots, stat cells); in `minimal` it is a `.panel` |
| `.btn`, `.btn.primary` | a button with a body; primary is filled with the highlight gradient |
| `.chip`, `.field`, `.track` | pills, inputs, progress, each styled per style |
| `.hero` | the big display text or numbers: gradient and embossed in `depth`, glowing in `neon`, letterpress in `paper`, glossy in `aero` |
| `.glow`, `.glow-text` | the style's glow on an element or text (a dot, a ring, a badge) |
| `.flow` | the slowly drifting accent gradient as a background (a bar, an underline) |
| `var(--hi)`, `var(--hi-2)` | the highlight pair for strokes, fills and active states: the accent in `minimal`, the style's colors otherwise (the accent if it has a hue, else the style's signature pair: blue-violet `depth`, cyan-magenta `neon`, terracotta `paper`, sky-aqua `aero`) |
| `var(--elev-1..3)`, `var(--bevel)`, `var(--sheen)`, `var(--rim)`, `var(--glow)` | the raw tokens, for your own raised or lit elements |

Two small opt-outs and one trap: `.flat` on an `<svg>` removes the blanket drop-shadow bare
wallpapers get (and neon's glow) for graphics that carry their own light; `.glow` on an `<svg>`
element is a filter, not a box-shadow; and widget iframes are sandboxed **without `allow-forms`**,
so a `<form>` never submits: handle `keydown` (Enter) and `click` yourself.

Rules that matter more now: the iframe **clips** at its own edge, so outer shadows and glows only
exist inside the box (use inset shadows, or leave room); never put a hard-coded color in a style
rule; per-style tweaks of your own go under `html[data-style="neon"] …`, but try the kit's tokens
first; check every style in the gallery (`gallery.html?widget=<id>&styles=all`) and the whole set
at once in the wall (`wall.html?style=aero&surface=glass&bg=photo-light`).

### The house style

The bundled widgets share one look, and copying `packages/widget-sdk/widget-kit/base.css` and
`kit.js` into your folder gives you the parts:

- Neutral steps (`--t0` for emphasis, then `--t1`, `--t2`, `--t3`) and color only from
  `--hi`/`--hi-2`/`--accent`. `--accent` is white unless the user picks another color (or `auto`,
  the wallpaper's accent), so in `minimal` the widget is monochrome; the other styles put their own
  highlight pair in `--hi` (or the user's accent if it has a hue), without a setting of its own.
- Segoe UI Variable (`--font`), tabular numbers (`.num`) for anything that changes, light weights
  for big numbers, small caps labels (`.caps`).
- Generous padding, soft shapes, one idea per widget.

## 6. Permissions

Anything not listed in `permissions` is denied.

| permission | gives |
|---|---|
| `system.stats` | `mypaper.system.*` |
| `media` | `mypaper.media.*` |
| `launch` | `mypaper.launch` |
| `network:<origin>` | `fetch` to that origin (for example `network:https://api.open-meteo.com`) |
| `theme` | `mypaper.theme.*` |
| `audio` | `mypaper.audio.spectrum` (loopback of the default output, only while you are visible) |
| `user` | `mypaper.user.info` (display name) |
| `storage.shared:<ns>` | `mypaper.storage.shared("<ns>")`, a namespace shared with the other widgets declaring it |
| `clipboard.write` | reserved: no `mypaper` call uses it yet |

`mypaper.pickFile` needs no permission, and `mypaper.appIcon` needs `launch`.

## Placement and instances

`"preset"` says where the widget lives: `desktop` (the default, on the grid below windows),
`overlay` (a small always-on-top widget, click-through except its own pixels; the host shows it in
a separate topmost layer outside edit mode, toggled with **Ctrl+Alt+Shift+O** or the tray; design
it as a self-contained capsule that is fine over any window) or `popup` (opened from a bar module;
see [widgets.md](widgets.md#placement-presets), the bars host owns placing it; render into the
iframe like any widget and take your size from `mypaper.size`). `"instances"` limits how many the
user can add: `single`, `multiple` (default) or `per-monitor`; the edit-mode palette and the
host enforce it.
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
- **Audio is costly for the host, so be frugal.** The host captures only while a *visible* widget
  subscribes to `audio.spectrum`; draw with `frames(fn, 30)` (never faster than the 30 Hz feed), stop
  drawing when every bin is zero (silence), and unsubscribe when the widget has no reason to listen.
  Prefer one canvas over many elements, and pre-compute gradients once.
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
- Appearance: the gallery (`/gallery.html?widget=<id>`): every surface at several sizes over a dark
  and a light photo, and again with `text=dark`/`scale=1.5` and `appearance={"border":false}`.
- Headless screenshots, without opening a window, for a quick look:
  `msedge --headless=new --screenshot=out.png --window-size=248,144 "http://localhost:5173/?ui=0&widget=hello&cols=2&rows=1"`.
