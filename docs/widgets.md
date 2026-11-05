# Native desktop widgets

N4 hosts all 17 built-in widget ids in `mypaper-shell`, using `mypaper_ui`. `crates/widgets` is a
services library. There is no widget executable, HTML entry point, widget WebView environment,
SDK, shell page, CSS plate renderer, backdrop file request or HTML-widget fallback. Web and app
wallpapers retain their own WebViews/capture path (D-W).

## Windows and glass

Each desktop widget is a top-level `Kind::DesktopChild { parent: progman }` surface. The historic enum name
means desktop layer; it is **not** a `WS_CHILD`. A Windows 8+ / PerMonitorV2 shell manifest is embedded.
N0.2 measured that ULW/GDI children of raised Progman are invisible and WGC cannot exclude children.
The selected order is ordinary app > widget > engine glass > Progman. Top-level widgets can be
excluded from the web/app wallpaper desktop overlay. See [ui.md](ui.md#desktop-windows-n4) for the
surface contract and [engine-notes.md](engine-notes.md#glass-overlays) for the measured engine result.

Activation cannot raise a widget through app windows. Window-event reconciliation repairs the
contiguous desktop band, restores widgets after Show Desktop, and recreates windows after Explorer
restarts. No idle z-order timer runs. Every rounded surface body is filled at `Rgba::HIT` so transparent
glass content receives pointer input. Corners remain click-through. Monitor work areas and grid
cells are stored logically, then converted to physical pixels with each monitor's DPI; a monitor
change rebuilds text, images and surface geometry.

Widget geometry is published through `crate::glass::publish(Source::Widgets, &Monitor, &[Plate])`.
`Plate { id, rect, radius, material: Material { clarity, blur, refraction, highlight, tint } }`
uses monitor-local physical pixels; `id` is the client HWND as `u64`. The merger keeps bar and widget
plates independent. The engine draws the wallpaper and frosted plates; widgets draw transparent
content. Default blur is **12 px at 96 DPI**. Geometry/material changes publish once; dragging
publishes each active frame. Widgets never capture the desktop. A live worktree-engine screenshot is
[widgets-native-glass.png](media/widgets-native-glass.png).

The overlay preset (including media-pill) uses an activating topmost `Kind::Popup` surface and follows `overlay`. While arranging it moves into the desktop band, then returns to its configured layer on exit.
Desktop presets stay in the desktop band below ordinary apps. Live glass over an app behind a topmost
widget still needs the engine's overlay consumer; the wallpaper plate seam does not capture apps.

## Edit mode

Ctrl+Alt+Shift+W toggles edit mode. The existing registered `mypaper.widgets.edit` message to the
message-only `mypaper.widgets.host` window still works for Studio/core callers (wParam: 0 off,
1 on, 2 toggle). The native palette
adds any built-in, selects instances, removes them, and exits. Drag and all eight resize edges/corners snap to
the per-monitor grid. Moves first try to swap a single neighbor into the original cell; otherwise
overlapping neighbors shift down, then right in reading order. If they cannot fit, the entire edit
is refused with feedback. Arrows move the focused widget; Shift+arrows resize it. The opposite edge
stays fixed when resizing from the top/left. Instance limits also apply to duplication. The display-frame ticker
is armed only during dragging/resizing, crossfades, flip transitions or finite launch feedback. Ctrl+Alt+Shift+O and the legacy overlay message
remain accepted by the native host. Per-widget settings forms live in Studio's existing inspector; Calendar event entry remains an on-widget content interaction: title,
validated date/time, six color swatches, Save/Enter and Cancel/Escape. Edit mode is refused during
a hold and exits when a hold begins.

## Views and scheduling

| Widget ids | Native behavior |
|---|---|
| `clock` | digital, dial, flip, greeting and world clocks; minute default, optional seconds |
| `calendar` | reusable `mypaper_ui::calendar::MonthGrid`, month navigation, day/agenda and local event entry |
| `now-playing`, `media-pill` | one media view, session metadata/artwork and transport controls; ids retained |
| `quick-toggles` | native action grid with pages |
| `launcher` | real app icons, launches, editable web search |
| `weather` | linked weather service, cached Open-Meteo data, WinHTTP worker |
| `system-monitor` | CPU, memory, GPU and network snapshots; visible-only 2 s samples |
| `timer` | focus/break/custom modes, start/pause/reset/skip and shared activity counts |
| `countdown` | wall-clock remaining digits at the displayed resolution |
| `photo` | bounded WIC decode, cover/contain and finite crossfade |
| `notes` | native reading view and windowless RichEdit editing |
| `todo` | check, edit, remove and add with the same native text component |
| `rss` | parsed feeds, cached items and native link actions |
| `crypto` | cached prices/changes fetched on a worker |
| `heatmap` | shared todo/timer activity counts |
| `quote` | deterministic daily/hourly selection or click to change |

There is no ambient animation. Media spectrum capture is opt-in (`variant = "viz"`); otherwise
there is no audio capture worker. Media uses the [shared integration provider](integrations.md);
the provider changes on SMTC events. Visible playing widgets interpolate their displayed timeline
once per second; paused media has no progress tick. Legacy photo motion, weather animation and
media glow settings remain readable data but
produce no ambient animation. A default clock schedules the next minute boundary, calendars the
next day, running timers the next displayed second, and paused timers nothing. Photo fades last
350 ms. Network workers block on a channel between requests; HTTP never blocks the UI thread.
Cover, display and hold facts stop subscriptions, timers and transient text fields and release
hidden surface bitmaps. Stats subscriptions request only fields actually shown; hidden disks and
connection details incur no query. Waits round up to whole milliseconds, avoiding early-wake spins.
No scheduled redraw occurs when a display list is unchanged. CPU/RAM/GPU percentages and meter
fills use the same rounded precision; undisplayed raw counter changes do not dirty the view.
Network counters cache physical interface identities and invalidate through Windows interface-change
notifications; traffic and link state are still sampled each interval. Zero-valued GPU engine rows
skip name parsing/hashing. Stats request only visible fields; `interval` retains the manifest's
string values `"1000"`, `"2000"` (default) and `"5000"` milliseconds. Histories use stable metric ids,
including network and each drive, so disabling a preceding metric never swaps traces.

Scrollable notes, tasks, feeds, event lists, world clocks, forecasts and app lists expose all content.
Wheel, arrows, Page Up/Down and Home/End move the viewport; Tab/Shift+Tab traverse visible actions,
and Enter/Space invoke them. Task rows also accept F2 and Delete. Escape cancels a task edit; blur
commits it. A launcher query's first Escape clears it and its second Escape exits the field.
Focus has a visible outline. Hover reveals full media metadata and heatmap date/count descriptions.
Launcher hover magnification and finite launch feedback run only in response to input.

`Op::Clipped { rect, ops: Arc<[Op]> }` clips retained rendering; widget viewports separately clip their hit geometry. Studio's existing `Op::Clip { rect }` / `Op::Unclip` markers remain supported. Retained damage remains local to
the changed children. Widgets use `Surface::present_sparse` to draw disjoint changed meter bounds
with one layered-window present. Text bounds follow measured glyphs rather than their layout cells.
`Op::Emphasis { rect, text, style, color, italic, strike }` supports the notes Markdown subset without
changing the shared `TextStyle` contract. RichEdit remains responsible for editing, selection,
clipboard, IME and undo; wheel coordinates are converted to client pixels for its hit testing.

`mypaper-shell` initializes `Widgets` after `Bars::new` has presented and after a message-loop turn.
The initialization cannot run ahead of the first visible bar. Facts arriving before it are applied
before the first widget paint or provider request.
`widgets_init_ms` and `widgets_ready_ms` are separate startup markers; they can be compared with the
bar's `first_paint_ms`. Widget surfaces use the deferred-accessibility constructor. Desktop-band
repair follows window events, coalesced by a one-shot 50 ms deadline, with a final repair 250 ms
after foreground changes to settle Explorer's animation. Two ordered passes converge paired glass
windows. There is no idle z-order timer. Coverage location hooks follow only the foreground process
and Explorer; a cached top-level set distinguishes destroyed apps from child-window churn.

## Layout and existing data

`%APPDATA%\mypaper\widgets.json` is unchanged: `grid`, `appearance`, and `instances`, with each
instance's `instance`, `widget`, `monitor`, `cell`, `size`, `appearance`, and `settings`. Instance ids
are stable. `now-playing` and `media-pill` are not renamed. The library's `layout.rs` owns grid
conversion, placement, migration and atomic saves. Missing files get six widgets: clock, calendar,
media, weather, monitor and notes. An intentionally empty or invalid existing file is never reseeded.
Invalid live reloads retain the last valid layout. Every layout save also parses the current file
at the write boundary, including edits arriving before a watcher notification. Unreadable or invalid
files refuse the write; invalid bytes are preserved in a non-overwritten `widgets.json.bak` (then
`.bak.1`, etc. for different contents). `widgets.json.previous` holds the prior valid version.
The edit palette displays the error; writes resume only after a valid reload. Layout changes are
committed in memory only after their save succeeds. Removal cleans data/picked files only after that
commit. Duplication rebases owned URLs recursively in settings and stored data, copies private files,
and refuses conflicting destination data instead of replacing it. A failed copy/save rolls back only
newly created duplicate data. Storage writes likewise refuse malformed existing JSON.

`widget-data/<instance>.json` stays a JSON object (1 MB limit), with the existing keys: notes `text`,
todo `items`, timer `state`, calendar `events`, and feed/crypto/weather caches. Shared activity lives
under `widget-data/_shared/`. Unknown settings keys and unrecognized widget instances survive layout writes; unknown structural
fields are rejected by the existing layout validator. Picked local URLs
and existing crypto cache shapes are read without rewriting the user's data. Notes/todo changes
save after 400 ms or on blur, hide and exit. Goldens and migration tests use old-format data.

The 17 original settings schemas are compiled under `crates/widgets/src/catalog/`. The library's
`registry::builtins() -> Registry` returns them. Studio reads these same files for its existing
settings forms; its layout stage uses name placeholders until N7 provides native previews. CSS
appearance paths are retained as data but are not loaded. Native views use theme tokens/materials.

## Services API

The `mypaper-widgets` library exports `layout`, `storage`, `registry`, `manifest`, `look`, `media`,
`stats`, `audio`, `appicon`, `plugins`, `rpc`, `wake`, `display`, `files`, `pick`, `userinfo` and feed
parsing. `bridge` contains typed service replies/errors, not a web message bridge.
`Waker::for_window(Sender<Msg>, HWND, message: u32)` posts service completions to the UI inbox;
thread-message wakeups remain available for other hosts. All UI and RichEdit objects stay on one
thread. External JSON-RPC plugins remain separate trusted processes.

## Plugins

A plugin is data/service code, not a widget renderer. Native views must explicitly call its declared
methods or subscribe to its declared events. Installing a plugin does not load HTML, JavaScript or
arbitrary UI. The supported consumers are Studio's declared setting-search calls and explicit Rust host/view
integrations through `Plugins::call`/`subscribe`; there is no generic desktop provider renderer or
user-configurable arbitrary plugin view. Built-in weather/RSS/crypto use their linked native worker.
Retaining plugin manifests does not promise an HTML or automatic provider UI. See
[writing-widgets.md](writing-widgets.md) for the JSON-RPC contract and examples.

## Validation

`src/widgets/check.ps1` runs 17 golden PNGs, semantic hit targets, scheduling, old-format
storage, drag collision/snap and desktop activation checks under the desktop lock. UI checks
include exact sparse/full meter pixels, calendar and RichEdit drawing/selection/undo. `src/widgets/live.rs` is a release
fixture with six actual widgets and a warmed native menu, and can exercise activation, Show Desktop,
Explorer restart, hit-testing and Unicode keyboard entry. The follow-up validation commands below use isolated data and the shared desktop lock.
`artifacts/n4/` retains the original N4 measurements; current follow-up evidence lives under
`src/widgets/evidence/`. The performance gate is separate from source parity status.

## Studio settings and persisted content

The catalog is the authoritative typed data model: defaults, ranges, allowed selections, list item
fields, groups, file types, size/instance limits and replacement aliases remain compiled unchanged.
Studio owns settings forms; native widgets consume these keys. Unknown saved settings survive layout
writes. Empty file selections are retained, and private image URLs keep their existing wire format.

| Widget | Settings (complete key inventory) | Catalog |
|---|---|---|
| `calendar` | `variant` (select), `weekStart` (select), `weekNumbers` (boolean) | [schema](../crates/widgets/src/catalog/calendar.json) |
| `clock` | `variant` (select), `format24` (boolean), `showSeconds` (boolean), `showDate` (boolean), `zones` (list), `name` (string), `font` (font) | [schema](../crates/widgets/src/catalog/clock.json) |
| `countdown` | `style` (select), `title` (string), `date` (string), `time` (string), `afterwards` (select), `showSeconds` (boolean), `startDate` (string) | [schema](../crates/widgets/src/catalog/countdown.json) |
| `crypto` | `coins` (list), `currency` (select), `showChange` (boolean), `refreshMinutes` (number) | [schema](../crates/widgets/src/catalog/crypto.json) |
| `heatmap` | `variant` (select), `source` (select), `weekStart` (select), `labels` (boolean), `legend` (boolean) | [schema](../crates/widgets/src/catalog/heatmap.json) |
| `launcher` | `variant` (select), `shortcuts` (list), `labels` (boolean), `iconSize` (slider), `search` (boolean), `engine` (select), `customUrl` (string) | [schema](../crates/widgets/src/catalog/launcher.json) |
| `media-pill` | `variant` (select), `progress` (boolean) | [schema](../crates/widgets/src/catalog/media-pill.json) |
| `notes` | `fontScale` (slider) | [schema](../crates/widgets/src/catalog/notes.json) |
| `now-playing` | `variant` (select), `tint` (boolean), `glow` (slider), `viz` (select), `sensitivity` (slider) | [schema](../crates/widgets/src/catalog/now-playing.json) |
| `photo` | `variant` (select), `image1` (image), `image2` (image), `image3` (image), `image4` (image), `image5` (image), `image6` (image), `interval` (slider), `motion` (slider), `shuffle` (boolean), `fit` (select), `caption` (string) | [schema](../crates/widgets/src/catalog/photo.json) |
| `quick-toggles` | `variant` (select), `wifi` (boolean), `bluetooth` (boolean), `focus` (boolean), `airplane` (boolean), `nightlight` (boolean), `sound` (boolean), `display` (boolean), `battery` (boolean) | [schema](../crates/widgets/src/catalog/quick-toggles.json) |
| `quote` | `mode` (select), `source` (select), `custom` (list) | [schema](../crates/widgets/src/catalog/quote.json) |
| `rss` | `feeds` (list), `variant` (select), `maxItems` (number), `showSource` (boolean), `showTime` (boolean), `refreshMinutes` (number) | [schema](../crates/widgets/src/catalog/rss.json) |
| `system-monitor` | `variant` (select), `cpu` (boolean), `ram` (boolean), `gpu` (boolean), `net` (boolean), `disk` (boolean), `battery` (boolean), `drives` (string), `interval` (select), `vary` (boolean) | [schema](../crates/widgets/src/catalog/system-monitor.json) |
| `timer` | `focusMinutes` (number), `shortMinutes` (number), `longMinutes` (number), `customMinutes` (number), `sessionsBeforeLong` (number), `autoAdvance` (boolean) | [schema](../crates/widgets/src/catalog/timer.json) |
| `todo` | `title` (string), `addPosition` (select), `hideDone` (boolean) | [schema](../crates/widgets/src/catalog/todo.json) |
| `weather` | `variant` (select), `city` (place), `units` (select), `animate` (boolean) | [schema](../crates/widgets/src/catalog/weather.json) |

Per-instance files remain JSON objects at `widget-data/<instance>.json`. Notes store `text`; tasks
store `items: [{ text, done }]` (300 maximum); calendars store `events: [{ id, date, title, time,
color }]` (500 maximum, `YYYY-MM-DD`, optional `HH:MM`, color index 0–5). Timer `state` retains
`mode`, `total`, `remaining`, `endsAt`, `finishedAt`, `sessions` in milliseconds. Exiting a long break
resets its session cycle; delayed focus completion credits its actual local finish date. Session task
credits move with list insertions/deletions so unchecking reverses the right day's completion.

RSS/crypto `cache` keeps source identity, `at` (epoch ms), items and per-feed errors or quotes. Old
coin-id keyed crypto caches are read without rewriting on load. Current-price precision follows the
currency/value. RSS merges newest first with global deduplication and keeps cached content on failure.
Weather's ten-minute cache remains owned by the linked service under `plugin-data/weather`. Photo
settings own `image1`–`image6`; broken files advance/retry on bounded deadlines and keep a good image
visible. Heatmap history remains in `widget-data/_shared/activity.json` under `todo` and `timer`.

Notes render headings, both bullet kinds, indented task boxes, quotes, rules, bold, italics, code and
strike as text operations; no HTML is evaluated. Material/text settings retain layering and resets,
active-palette auto accent, opaque solid fill, optional text shadow and distinct minimal/depth/paper/
aero/liquid native treatments. Deliberately removed CSS, noise, gradients, glow and ambient motion
remain readable data and do not regain renderers.

## Widget parity follow-up validation

`src/widgets/check.ps1 [-Bless] [-Studio]` runs widget goldens/click targets and UI checks under the shared
desktop lock. `src/widgets/validate-live.ps1 [-Interactions] [-Samples 3]` runs this worktree's release
engine with rain-city, production bars/dock and six production widgets, captures `docs/media/`, and
uses the official 60-second measurement tool. It then launches the actual production shell three
times with those six configured instances and records spawn/first-bar/widget-init markers. Original
processes, arguments and desktop state are restored in `finally`. Raw evidence lives under
`crates/shell/src/widgets/evidence/`; `-Interactions -Samples 0` records interaction checks separately
in `evidence/interactions/`. `-Debug -Samples 0` is diagnostic only; measurements require release.
No dev server is needed.
`-Profile -Samples 0` is a separate diagnostic: it compares the six-widget fixture, the same fixture
without system-monitor, and bars alone, including UI/worker CPU and sampling/painting costs. These
different phases are not combined into a gate result.

Additional public services for Studio/native callers:

```rust
// Existing guarded API (signature unchanged):
Layout::save(&self, path: &Path) -> anyhow::Result<()>;
// Studio's raw JSON follows the same validation, backup and refusal at the write boundary:
Layout::save_value(value: &serde_json::Value, path: &Path) -> anyhow::Result<()>;
// New duplicate/storage helpers:
Storage::duplicate(&mut self, from: &str, to: &str) -> anyhow::Result<()>;
Storage::remove(&mut self, instance: &str) -> anyhow::Result<()>;
pick::rebase_value(value: &mut serde_json::Value, from: &str, to: &str);
pick::copy_instance_checked(root: &Path, from: &str, to: &str) -> anyhow::Result<()>;
// Shared sampler fields are all bool: gpu, disks, net, battery, network.
Stats::subscribe_fields(&mut self, instance: &str, sub: &str,
    interval: Duration, now: Instant, fields: SampleFields);
Stats::refresh_devices(&mut self);
// Deferred host startup applies buffered facts before its first widget frame:
Widgets::new(ui: &mut Ui, window: HWND, initial_facts: &[(usize, isize)])
    -> anyhow::Result<Widgets>;
// Additive UI APIs:
DisplayList::damage_regions(&self, old: &DisplayList) -> Vec<Rect>;
Surface::present_sparse(&mut self, ui: &mut Ui, origin: (i32, i32),
    size: (u32, u32), list: DisplayList) -> anyhow::Result<bool>;
```

Existing edit/overlay/layout/data inbox message values and registered Studio/core messages are
unchanged. No engine protocol or engine source changed. The startup seam is in `shell/src/main.rs`,
outside widget directories because the shell owns the order of its hosts. Studio's `stage.rs`
translates/fades the two new rendering operations; its widget duplication action uses owned-file
copying with rollback, and `docs.rs` routes widget saves through the guarded raw JSON API. Ten Studio
goldens and their README copies reflect the completed widget previews. The requested README
screenshot is saved in `docs/media/widgets-native-glass.png`.

The shared SMTC snapshot now exposes `position_ms`, `duration_ms` and `position_at_ms`
(`Option<i64>`, Unix milliseconds for the last field), with timeline-change notifications.
The adapter emits seconds plus `_at` for the existing widget view; unsupported/live timelines
remain null. The provider has no sampling timer. The two necessary changes outside widget
ownership are `crates/integrations/src/media.rs` and
`crates/shell/src/integrations/media.rs`, so the shared adapter retains elapsed/duration parity.
Desktop surfaces also emit `Event::Activated`; normal mouse activation permits keyboard editing
while the existing desktop position guard prevents automatic raising over apps.
