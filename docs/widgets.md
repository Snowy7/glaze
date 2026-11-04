# Native desktop widgets

N4 hosts all 17 built-in widget ids in `mypaper-shell`, using `mypaper_ui`. `crates/widgets` is a
services library. There is no widget executable, HTML entry point, widget WebView environment,
SDK, shell page, CSS plate renderer, backdrop file request or HTML-widget fallback. Web and app
wallpapers retain their own WebViews/capture path (D-W).

## Windows and glass

Each widget is a top-level `Kind::DesktopChild { parent: progman }` surface. The historic enum name
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

Widget geometry is published through `crates/shell/src/glass.rs::publish(monitor, &[Plate])`.
`Plate { id, rect, radius, material: Material { clarity, blur, refraction, highlight, tint } }` uses
monitor-local physical pixels; `id` is the native client HWND encoded as `u64`, for stable identity
and desktop-overlay exclusion. Geometry is compared at rest and sent each active drag frame. This
slice includes the permitted local stub; the shared-memory glass transport is an N2/orchestrator
integration dependency. Actual widget plates and exclusion lists need verification after that
module is wired. Widgets do not capture the desktop themselves.

## Edit mode

Ctrl+Alt+Shift+W toggles edit mode. The existing registered `mypaper.widgets.edit` message to the
message-only `mypaper.widgets.host` window still works for Studio/core callers (wParam: 0 off,
1 on, 2 toggle). The native palette
adds any built-in, selects instances, removes them, and exits. Drag and bottom-right resize snap to
the per-monitor grid; collision checks reject overlapping final placements. The display-frame ticker
is armed only while dragging/resizing or crossfading. Ctrl+Alt+Shift+O and the legacy overlay message
remain accepted by the native host. Per-widget settings forms live in Studio's existing inspector;
no form opens inside a desktop widget.

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
there is no audio capture worker. Playing media updates progress once per second; paused media has
no tick. Legacy photo motion, weather animation and media glow settings remain readable data but
produce no ambient animation. A default clock schedules the next minute boundary, calendars the
next day, running timers the next displayed second, and paused timers nothing. Photo fades last
350 ms. Network workers block on a channel between requests; HTTP never blocks the UI thread.
Cover, display and hold facts stop subscriptions, timers and transient text fields and release
hidden surface bitmaps. Stats subscriptions request only fields actually shown; hidden disks and
connection details incur no query. Waits round up to whole milliseconds, avoiding early-wake spins.
No scheduled redraw occurs when a display list is unchanged.

## Layout and existing data

`%APPDATA%\mypaper\widgets.json` is unchanged: `grid`, `appearance`, and `instances`, with each
instance's `instance`, `widget`, `monitor`, `cell`, `size`, `appearance`, and `settings`. Instance ids
are stable. `now-playing` and `media-pill` are not renamed. The library's `layout.rs` owns grid
conversion, placement, migration and atomic saves. Missing files get six widgets: clock, calendar,
media, weather, monitor and notes. An intentionally empty or invalid existing file is never reseeded.
Invalid live reloads retain the last valid layout.

`widget-data/<instance>.json` stays a JSON object (1 MB limit), with the existing keys: notes `text`,
todo `items`, timer `state`, calendar `events`, and feed/crypto/weather caches. Shared activity lives
under `widget-data/_shared/`. Unknown fields and instances survive layout writes. Picked local URLs
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
arbitrary UI. See [writing-widgets.md](writing-widgets.md) for the JSON-RPC contract and examples.

## Validation

`cargo test -p mypaper-shell` covers 17 golden PNGs, semantic hit targets, scheduling, old-format
storage, drag collision/snap and top-level desktop surface creation. `cargo test -p mypaper-ui`
checks the reusable calendar and RichEdit drawing/selection/undo. `src/widgets/live.rs` is a release
fixture with six actual widgets and a warmed native menu, and can exercise activation, Show Desktop,
Explorer restart, hit-testing and Unicode keyboard entry. `measure.ps1 -Interactions` uses isolated
data and the shared desktop lock, then runs the official three 60-second measurements. See the N4
report under `artifacts/n4/` for exact numbers, limitations and incomplete integration gates.
