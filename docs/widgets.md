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
publishes each active frame. Widgets never capture the desktop. Frosted glass over this worktree's
rain-city engine was inspected live; the final README screenshot is pending the next desktop window.

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

`src/widgets/check.ps1 [-Bless] [-Studio] [-BlessStudio]` runs widget goldens/click targets and UI checks under the shared
desktop lock. `src/widgets/validate-live.ps1 [-Samples 3]` runs this worktree's release engine
and actual shell with rain-city, production bars/dock and six production widgets, captures
`docs/media/`, and uses the official 60-second measurement tool. A sleeping fixture helper seeds
the layout and holds paused-media, display-awake and visible-desktop guards; it creates no product
surfaces. The measured CPU/RAM row is `mypaper-shell`. The script then launches the shell three
times with those six configured instances and records spawn/first-bar/widget-init markers. Original
processes, arguments and desktop state are restored in `finally`. Raw evidence lives under
`crates/shell/src/widgets/evidence/production/`; `-Interactions -Samples 0` records interaction checks separately
in `evidence/interactions/`. `-Debug -Samples 0` is diagnostic only; measurements require release.
`-HostFixture` runs the component host for counters, not the full-shell CPU/RAM qualification.
No dev server is needed. Production checks require six actual hit-testable widgets before and
after sampling, and both shell and helper must exit successfully. The helper wakes an already-idle
display with a zero-distance mouse move and restores its execution-state requirements on exit;
saved power-plan settings are untouched.
Idle samples start after a full minute of warm-up with all six widgets visible; cold startup is
measured separately. The earlier 18-second warm-up results remain in `evidence/before-full-warmup/`.
The gate fixture prints hit/counter/cost checkpoints before and after external sampling. Its own
periodic CPU/thread/window diagnostics run only in `-Profile`, so they do not pollute idle gates.
GPU sampling uses raw PDH busy-time/timestamp deltas, with owned counter identities preserved across
reordering and process churn. A live comparison against Windows' formatter checks identical values;
the sampler opens no graphics device or vendor driver. Widget file watches use native directory
signals and indefinite waits instead of the generic Windows backend's 100 ms worker tick. The
shared integration configuration watch uses this same helper; bar watches are unchanged.
Cancellation is nonblocking, with the stop handle shared until the worker exits. This also permits
service watches owned by thread-local state: Windows terminates other workers before process-exit
TLS destruction, so joining them from that destructor is invalid. A subprocess regression exercises
that Windows shutdown ordering.
`-Profile -Samples 0` is a separate diagnostic: it compares the six-widget fixture, the same fixture
without system-monitor, and bars alone, including UI/worker CPU and sampling/painting costs. These
different phases are not combined into a gate result.
`-ProductionProfile` separately compares six instances, no instances, and no desktop menu;
`-StatsProfile` compares all metrics, no GPU metric, and no GPU/network metrics. These are
diagnostic reductions, never idle-gate qualifications. `-WithoutDesktopMenu -Samples 1` keeps
the six widgets and isolates the menu. The failed and diagnostic sets remain in their own folders.

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
// A sleeping worker; changed runs on that worker after the watched file's bytes change.
mypaper_ui::FileWatch::new(path: &Path, changed: impl Fn() + Send + 'static)
    -> anyhow::Result<FileWatch>;
```

Existing edit/overlay/layout/data inbox message values and registered Studio/core messages are
unchanged. No engine protocol or engine source changed. The startup seam is in `shell/src/main.rs`,
outside widget directories because the shell owns the order of its hosts. Studio's `stage.rs`
translates/fades the two new rendering operations; its widget duplication action uses owned-file
copying with rollback, and `docs.rs` routes widget saves through the guarded raw JSON API. Ten Studio
goldens and their README copies reflect the completed widget previews. The live runner writes
`docs/media/widgets-native-glass.png` after confirming all six widgets are visible. The last attempt
had T3 covering one monitor, so that screenshot was rejected and is not included as README evidence.
Studio's explicit "Start over" action also retains immutable widget recovery copies; resetting
another malformed layout never replaces an earlier backup.

`shell/build.rs` delay-loads `pdh.dll` and `winhttp.dll` until their first service calls. Startup
evidence checks that PDH is absent at the first bar and records private memory after widgets are
ready. The native shell's application manifest remains executable-only so Studio can link its
view library. Native Studio previews exposed an order-dependent WinRT `Package::Current` factory
crash in notification identity checking; `integrations/src/notifications.rs` now probes the same
identity with `GetCurrentPackageFamilyName`, without retaining a COM factory between test threads.
This also adds `Win32_Storage_Packaging_Appx` to that crate's Windows features.

The shared SMTC snapshot now exposes `position_ms`, `duration_ms` and `position_at_ms`
(`Option<i64>`, Unix milliseconds for the last field), with timeline-change notifications.
The adapter emits seconds plus `_at` for the existing widget view; unsupported/live timelines
remain null. Timeline notifications read only the timeline; they do not reread track metadata.
Decoded album art and its blurred/tinted material are retained until the artwork value changes.
The provider has no sampling timer. Necessary shared-media edits outside widget
ownership are `crates/integrations/src/media.rs` and
`crates/shell/src/integrations/media.rs`, so the shared adapter retains elapsed/duration parity.
Desktop surfaces also emit `Event::Activated`; normal mouse activation permits keyboard editing
while the existing desktop position guard prevents automatic raising over apps.

Sparse updates acquire one WIC read lock for all dirty regions and omit unchanged position/size
from the layered-window update. Each canvas retains at most 64 immutable arc geometries so repeated
displayed values reuse their paths. Retaining only one value rebuilt too much geometry in live
measurements, so that experiment was discarded.
Thirty seconds after widget startup, one heap-cache optimization asks Windows to decommit unused
LFH memory. Live widget data and render caches stay allocated; this adds no periodic idle task.
Its effect on the final memory gate has not been measured. This uses the documented
[HeapOptimizeResources operation](https://learn.microsoft.com/en-us/windows/win32/api/heapapi/nf-heapapi-heapsetinformation).

The additional read-only inbox message `WM_DIAGNOSTICS = WM_APP + 104` (payload zero) logs widget
present counters, visibility/rectangles and `WorkCosts`; the host never schedules it. Existing
`WM_EDIT`/`WM_OVERLAY`/`WM_LAYOUT`/`WM_DATA` remain `WM_APP + 100` through `+103`; edit/overlay
payloads in `lParam` are 0 off, 1 on, 2 toggle. Registered control names remain
`mypaper.widgets.edit` and `mypaper.widgets.overlay` on `mypaper.widgets.host`.

The additive rendering operations have these exact fields (existing `Clip`/`Unclip` remain supported):

```rust
Op::Clipped { rect: Rect, ops: std::sync::Arc<[Op]> }
Op::Emphasis {
    rect: Rect, text: std::sync::Arc<str>, style: TextStyle, color: Rgba,
    italic: bool, strike: bool,
}
Event::Activated
```

## Implementation handoff, 2026-11-05

Source parity is closed for every retained widget/host row in `plans/native/parity.md`: all 17
widgets, edit mode, settings data, scrolling, keyboard controls and error/empty states. Settings
forms remain in Studio. P0 regression coverage includes malformed files at startup and during a
warm session, storage corruption behind a populated cache, immutable recovery copies, owned-photo
URL rebasing, destination refusal and duplicate rollback. Studio retains the shell preview exports
`widgets::{model,time,view,tokens}` and its 27 tests pass.

The final live qualification is **pending**, explicitly deferred by the user. No further desktop
runs were made after that instruction. The last attempt was rejected before measurement because
T3 covered the second monitor (three of six widgets were visible). Earlier display-off attempts
were also rejected. Their partial folders cannot qualify the current source.

The last valid full-shell set before the discarded geometry-cache experiment is retained in
`evidence/production-before-current-geometry/`. Both visibility checks were six of six, the shell
and seed helper exited with code zero, and the release wallpaper engine was built from this
worktree. These are historical measurements, not certification of the final tree:

| Gate | Measured evidence | Result / remaining work |
|---|---|---|
| Shell idle CPU ≤0.1% of one core | 0.05%, 0.16%, 0.08%; median **0.08%** | Historical median passes; final source needs rerun. |
| Shell private memory ≤40 MB | 42, 41, 41 MB; median **41 MB** | Historical result fails; final startup heap release is unqualified. |
| Spawn → first bar ≈73 ms | 263.251, 255.440, 221.090 ms; median **255.440 ms** | Target not met. |
| Widgets start after first bar | All three starts; widget init at 2013.6, 1742.6, 1852.1 ms; PDH absent at first bar | Deferred startup verified on the historical set. |
| Clock one present/minute; zero unscheduled widget presents | Component trace: 202.942 seconds, four minute boundaries, 101 nonclock presents at the visible 2-second sampling cadence | No extra presents in that trace; production scheduling still needs final qualification. |
| Widget goldens and click targets | All 17 widget images and hit maps; shell widget checks **18 passed**, two explicit probes ignored | Pass. |
| Shared UI, Studio and data/service tests | UI **33 + 1**, Studio **27**, widgets **124** (three probes ignored), integrations **10** passed | Pass; strict clippy passes on changed production targets. |
| Real desktop interactions | Twelve app activations, transparent-body hit, Arabic keyboard and committed CJK text, Show Desktop, Explorer recovery, drag/resize persistence and overlay transitions passed | Previous live set; CJK IME composition was not available. |
| Frosted widget glass over worktree rain-city | Inspected live; engine rendered wallpaper/plates on both monitors | Final saved README screenshot pending. |
| Crash → bars ≤300 ms | Not remeasured in this slice | Pending; no recovery-time claim. |

The initial component fixture omitted parts of the production shell. Its low CPU/RAM results
remain in the root evidence folder for diagnosis and must not be used as shell gates. The runner
now defaults to the actual production shell; `-HostFixture` is explicitly diagnostic. Disabling
metrics or removing instances was used only in named profile folders, never to qualify idle.
The discarded single-value arc-cache experiment is archived in
`evidence/production-current-geometry-experiment/`; it measured median 0.18% and 42 MB, so the
final source retains historical geometries in a bounded 64-entry cache instead. Final changes to
that bound and the one-time heap optimization still require the deferred desktop window.

Necessary edits outside widget ownership are limited to these consumers and startup seams:

- `crates/shell/src/main.rs`, `crates/shell/build.rs`, `crates/shell/Cargo.toml`: post-first-present
  initialization, lazy service DLL imports, executable-only manifest and Win32 APIs for safe file
  writes and the one-time unused-heap-cache release.
- `crates/integrations/src/media.rs`, `crates/shell/src/integrations/media.rs`: optional timeline
  fields and event-driven reads/artwork reuse shared with bars.
- `crates/shell/src/integrations/mod.rs`: reuse the signal-driven `mypaper_ui::FileWatch` for
  shared configuration so it adds no 100 ms idle worker.
- `crates/integrations/src/notifications.rs`, `crates/integrations/Cargo.toml`: native package
  identity probing fixes the Studio-test COM factory lifetime crash after the native merge.
- `crates/studio/src/docs.rs`, `crates/studio/src/pages/widgets.rs`,
  `crates/studio/src/stage.rs`: guarded layout writes/reset copies, safe owned-file duplication
  and rendering of the additive UI operations. Ten Studio preview goldens and their copies in
  `docs/media/studio/` were updated for the completed widget views.

No engine, bar, flyout, core or glass-publisher source was changed by this slice. Native changes
through `9cc02b1` are merged into the widget branch; the orchestrator performs the eventual merge
back. Machine-readable handoff details and raw logs are in
`crates/shell/src/widgets/evidence/verification.json`.
