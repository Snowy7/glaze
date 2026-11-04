# Native top bar, dock and taskbar

`mypaper-shell` draws the bars with `mypaper-ui`: software Direct2D/DirectWrite into cropped,
premultiplied layered windows. `crates/bars` (`mypaper-bars`) is a **library of Windows services**,
configuration and presets. It has no executable, browser, graphics device, page or JSON message bridge.
`bars/shell` and the old host, capture, blur and first-paint stand-ins have been removed.

## Processes and recovery

Core waits for Explorer (`wait_for_shell`), stops a leftover shell (`stop_leftover`), then starts
`mypaper-shell` immediately, before starting the wallpaper or setting up its own tray/styling.
The only supervised children are the wallpaper engine and the native shell.

The shell takes `Local\mypaper-shell` before touching the taskbar journal. It replays a previous
journal, reads `bars.json`, and hides the native taskbar windows **before loading the UI factories**.
Full auto-hide/alignment changes and app-bar negotiation follow the first paint; those Explorer calls
must not delay showing the bars. Startup failure restores the journal too.

`WM_CLOSE` to the message-only `mypaper.shell.inbox` asks the shell to exit. On an ordinary exit it
stops its tray host, clears plates, removes reservations and restores the taskbar. It also exits if
its parent process exits. Core replays `%LOCALAPPDATA%\mypaper\taskbar.json` **only after the shell
has exited** (including shutdown/panic), so both processes never write the journal concurrently.
Shell crashes restart immediately after journal replay, with the existing five-crashes-in-a-minute
limit. Already-enabled native auto-hide is left under the user's ownership: its flag is journaled
only if mypaper changes it. This avoids restoring a setting we never changed.
Ctrl+Alt+Shift+Backspace remains core's panic hotkey and works independently of the UI thread.

## Configuration and presets

`%APPDATA%\mypaper\bars.json` is watched live. Unknown fields, invalid ranges and duplicate modules
remain errors; an invalid edit leaves the last valid configuration running. Writers use
`mypaper_bars::config::edit`, which validates and writes atomically while preserving unrelated fields.
Missing configuration uses the existing `monochrome` preset and native **liquid** material.

Layers remain: bar design -> preset -> shared `look`/`animation` -> the bar's own fields -> monitor
overrides. Theme `bars.appearance` supplies the appearance under `bars.json`'s appearance. The same
`config::BarsConfig` and `presets` APIs remain available to core's tray and Studio.

```json
{
  "version": 1,
  "preset": "macos",
  "taskbar": { "mode": "hide" },
  "look": { "style": "liquid", "family": "macos" },
  "top": {
    "enabled": true,
    "monitors": "all",
    "layout": "floating",
    "height": 32,
    "margin": 8,
    "left": ["start", "workspaces", "app"],
    "center": ["clock"],
    "right": ["media", "tray", "network", "volume", "battery", "quick", "power"],
    "clock": { "seconds": false, "format": "ddd d MMM  HH:mm" }
  },
  "dock": {
    "enabled": true,
    "monitors": "all",
    "style": "floating",
    "position": "bottom",
    "size": 48,
    "autohide": false,
    "magnify": { "scale": 1.5, "range": 3 },
    "running": "monitor",
    "extras": ["desktop", "trash"]
  },
  "overrides": {
    "secondary": { "top": { "height": 28 } },
    "DISPLAY2": { "dock": { "autohide": true } }
  }
}
```

- `preset`: monochrome, macos, windows11, windows10, chromeos, islands, floating. Existing neon names
  load for compatibility; their decoration is dropped. Top/dock designs still select metrics,
  modules, layout and family independently.
- `taskbar.mode`: keep, autohide or hide. `taskbar.alignment`: left/center, or omitted to leave it alone.
  With both bars disabled the native taskbar is kept.
- `monitors`: all, primary, or a list of device names. Overrides apply first by primary/secondary,
  then by device name. `DISPLAY2` and `\\.\DISPLAY2` are equivalent, case-insensitively.
- `performance`: on, off or auto (on battery). Windows' animation-effects setting and
  `animation.reduceMotion` also control motion.

### Native styles

`look.style` selects liquid (default), mono or solid. `look.family` remains independent (mono,
fluent, macos or material); Fluent groups network/volume/battery clicks into quick settings.
N6 adds the remaining native styles and bundled icon fonts.

Liquid content sits over the engine's plate, with an invisible `Rgba::HIT` shape in the shell.
Mono draws a tint locally; solid draws an opaque local fill. Neither needs a plate or capture.
Font family/size/weight, radius, scheme, tint and solid fills remain data. App icons use their cached
PNGs; module icons use the system icon font or a configured icon pack. Raster and SVG user-pack icons
are decoded once on the CPU, cached across monitors and tinted for mono entries.

CSS, gradient/image **surface fills**, noise, glow, neon and shaped corners are ignored with one
warning per process. Image app icons are retained. There is no CSS renderer or DWM blur fallback.

## Geometry and input

Top layouts are strip, floating and islands (`|` separates groups). Width accepts full, auto, DIPs
or a percentage. The bar may sit on the top or bottom edge. Dock layouts are floating (icons) and
taskbar (full edge, left/right modules and optional window titles), on any of the four edges.

The visible bars **float over the wallpaper**. Each non-auto-hiding bar keeps a separate app-bar
reservation (`ABM_NEW`, `ABM_QUERYPOS`, `ABM_SETPOS`); its full band includes margins, so maximized
windows stop outside it. Auto-hide reserves nothing and uses a one-pixel `Rgba::HIT` edge surface.
App-bar position notifications, work-area changes, display/DPI changes and Explorer restart update
the reservations. Work-area notifications are layout changes, never appearance/config changes.

Every visible surface shape explicitly fills `Rgba::HIT` before painting. Transparent space around
floating pills and between islands stays alpha zero and lets clicks through. Dock buffers cover a
bounded magnification envelope rather than a monitor-sized flyout canvas. Flyouts/previews are their
own windows; no bar keeps 360 DIPs of transparent backing memory.

Input uses `mypaper_ui::Event`; hits map directly to typed Rust handlers. The dock remembers the
pressed item so magnification cannot lose its click between press and release. Pointer capture
keeps pin dragging working outside the dock. There is no serialized command envelope.

## Modules and actions

| Module | Shows / default input |
|---|---|
| start | Launcher |
| search | Windows Search |
| workspaces | Current desktop/count; click a desktop to send Win+Ctrl+arrow per step |
| app | Last foreground app's name and icon; clicking the bar does not replace it |
| clock | Minute-aligned date/time; opens notifications (including the calendar) |
| media | SMTC title/artist; click play/pause, wheel previous/next |
| tray | Promoted icons and overflow; callbacks retain the icon's screen anchor |
| network | Wi-Fi / ethernet / offline; opens Wi-Fi, or quick settings for Fluent |
| volume | Level/mute; click mixer, middle mute, wheel +/-4% |
| battery | Percent/charging; absent on desktops; power settings or Fluent quick settings |
| quick | Quick settings |
| power | Windows power menu |
| stats | CPU/RAM, sampled every two seconds while shown; click Task Manager |
| language | Current keyboard language/layout; opens Windows' layout switcher |
| notifications | Count while shown; opens notifications |
| custom:<id> | Bounded command output, with configured click actions |

Clock tokens: `dddd ddd d dd MMMM MMM MM M yyyy yy HH H hh h mm ss tt`; `'literal'` and doubled
quotes work. Windows NLS supplies localized names/periods. A missing top format gets the existing
`ddd d MMM  HH:mm` default; a missing dock format follows Windows' regional short time/date.
Seconds are opt-in and change the deadline to the next second. There is no sub-minute timer for a
minute clock.

`modules.<name>.click` binds left, middle, right, wheelUp or wheelDown to an action. `default` uses
the module's behavior; `none` consumes the input. Supported built-ins remain start, search,
notifications, quick-settings, calendar, network, mixer, native-start, native-notifications,
native-quick-settings, task-manager, settings, show-desktop, media-toggle/next/previous,
volume-up/down/mute, workspace-next/previous, lock and explorer. `{ "exec": program, "args": [] }`
and `{ "open": pathOrUri }` execute the host's own configuration. Wheel bindings are throttled to
120 ms. Module style overrides retain color, flat background, padding, radius, size and weight.

Custom modules keep their command/args/interval, first-line or typed JSON output, ten-second timeout
and error state. Only displayed custom modules run. Stats and custom work stop while a fullscreen
app is in front elsewhere; that monitor's bars hide. Lock/display-off suspend every bar and its
subscriptions. User pause does not remove the user's taskbar.

## Dock behavior

Pins come first; running groups retain first-seen order. App grouping/eligibility is the existing
WinEvent-driven `apps` service (AUMID, otherwise process image). `running` accepts true, false or
monitor; `split` makes one item per window. Pins default to Windows' pinned shortcuts, with Explorer
when none exist. Shortcut identity, app names and icons are resolved on the existing icon worker.

- Left activates the most recent window, minimizes an already-active one and activates the next
  eligible window, or launches an app with no windows. Middle launches another instance.
- Right opens the native command menu with windows, New window, Pin/Unpin and Close.
- Drag reorders pins, pins a running item and unpins an item pulled well off the dock. Split-off
  windows do not edit pins. Edits preserve the rest of `bars.json`.
- Running/active/attention indicators, count badges and optional taskbar titles are drawn natively.
- Magnification uses the former page's cosine falloff and continuous neighbor displacement. Springs
  settle with an exact final frame. The shell's high-resolution ticker is armed only during motion.
- Launch and attention motion are bounded (launch ends on the first window or after four seconds).
- Hover previews retain the configured delay, live DWM thumbnails, window activation and close
  actions. Leaving both dock and preview closes it after 280 ms.
- Auto-hide slides out after one second without the pointer; menus, previews and drags keep it open.
  Slide frames move the window without rerasterizing its content. A one-pixel edge reveals it again.
- Desktop and Recycle Bin extras remain; emptying uses Windows' confirmation.

## Tray and Windows providers

With `taskbar.mode: hide` or `tray.host`, the existing native tray host receives real
`Shell_NotifyIcon` messages, preserves Windows' promotion/order settings, forwards Explorer's app-bar
traffic and dispatches each icon's versioned callbacks. Otherwise the existing UI Automation provider
reads Explorer's tray. Icon lists follow native notifications and UI Automation events. The
existing tray host retains its three-second z-order/rejoin check (one window lookup); it runs only
while a tray module is shown. Explorer restart reattaches it and reapplies reservations/taskbar settings after settling.

`mypaper_bars` publicly exposes: appbar, apps, hooks, icons, tray/tray_host/tray_data, media, audio,
network, system (battery/stats/window actions), clock, desktops, custom, trash, accent and taskbar.
`flyouts::{clipboard,index,language,mixer,notifications,quick,wifi,winkey}` retain their plain Rust
provider APIs for N3. Worker results use this typed channel; no browser/JSON bridge remains:

```rust
wake::Msg::{
    Config, Theme, Described(Source, Described), Media(Option<MediaInfo>),
    Volume, Network, Tray(Vec<TrayItem>), Flyout(flyouts::Done),
    Custom(String, custom::Output), WatchError(PathBuf, String),
}
flyouts::Done::{
    WinKey(Instant, winkey::Target), Changed(flyouts::Watched),
    NotificationCount(Option<usize>),
}
flyouts::Watched::{Core(actions::Topic), Mixer, Wifi}
```

`wake::Waker::new(Sender<Msg>, thread_id).send(Msg)` posts `wake::WM_WAKE` (`WM_APP + 10`)
after queueing the result. `flyouts::Lane<S>::spawn(name, init)` creates provider state in its own
COM/WinRT lane; `Lane::run` executes a plain Rust closure there. N3 can reuse the exported providers
without recreating the old request envelopes. Virtual-desktop notifications are manual-reset: the
outer wait leaves them signalled until `Desktops::changed` resets/rearms and reads the values.

## Flyout integration seam (N3)

Every panel-opening bar click calls exactly:

```rust
flyouts::open(kind: flyouts::Kind, anchor_rect: mypaper_ui::Rect, monitor: &mypaper_desktop::Monitor)
```

The anchor is in physical **screen** pixels. `Kind` is Calendar, Quick, Wifi, Mixer, Notifications,
Launcher, Power, TrayOverflow or Language. The tiny `crates/shell/src/flyouts/mod.rs` currently opens
Windows' panel: Win+N/calendar, control center, `ms-availablenetworks:`, app-volume settings, Start,
Win+X/power, native tray overflow or Win+Space. N3 replaces that one boundary.

## Plate integration seam

`crates/shell/src/glass.rs` is the only publication boundary:

```rust
struct Material { clarity: f32, blur: f32, refraction: f32, highlight: f32, tint: Rgba }
struct Plate { rect: Rect, radius: f32, material: Material }
fn publish(monitor: &Monitor, plates: &[Plate]);
```

Plate rects are physical pixels **relative to the monitor**, not screen/DIP coordinates. A call
replaces that monitor's complete list; `[]` removes it. The shell publishes after each frame whose
geometry/material changed, including every moving dock frame, and publishes nothing at rest.
It clears lists on suspension/removal/shutdown. `Bars::plates(&Monitor) -> Vec<Plate>` exposes the
current bar list so N4 can combine bar and widget geometry before the single publication. At that
merge, move publication/clearing from `Bars::paint`/`Drop` into the shell compositor and publish the
combined list once; separate calls would replace each other's plates. There is no engine-side spring
or JSON-file writer in the shell. This base has no `crates/glass-ipc`, so the module is the explicitly requested local
stub (one warning). The orchestrator must connect that module to the transport before measuring
real glass or asserting the integrated glass/fixture gates.

## Validation

`cargo check` / `cargo clippy -- -D warnings` target only bars, shell, core and ui.
`cargo test` runs per crate. Shell tests render the same lists as the screen: six PNGs under
`crates/shell/tests/golden/bars` (top/dock in liquid, mono and solid), alpha/click coverage for every body
and hit, island click-through, minute-clock damage, dock edges/taskbar modules, magnification and
preview clicks. The UI's separate process verifies no vendor GPU driver loads.

### Live N2 measurements (2026-11-05)

Release `mypaper-shell` and `mypaper-core`, built in this worktree, under the desktop lock.
Two visible 2560x1440 screens at 96 DPI, liquid bars on both, minute clock, Spotify paused, no
flyout open during sampling. The fixture used a still photo and the already-installed wallpaper
engine: the engine shader/transport and N4 widgets are not in this base. These numbers measure the
bar slice, not the integrated rain-city + six-widget fixture.

| Check | Measured | Gate | Status |
|---|---|---|---|
| Shell private memory | clean idle median 14 MB; observed peak 21 MB | <=40 MB | passes for this fixture |
| Shell idle CPU, percent of one core | median 0.05% (0.05, 0.08, 0.03) | <=0.1% | passes for this fixture |
| Bars over maximized apps | median 0.08% (0.08, 0.08, 0.13); one later recheck 0% | <=0.05% | **fails**; recheck is not a replacement three-sample median |
| Shell GPU engine use / GPU memory | 0% / 0 MB in the samples; no vendor GPU DLL | software renderer | passes |
| mypaper WebView2 processes | 0 | 0 | passes |
| Idle redraws | no dock redraws; two top-bar updates per minute, one per screen | minute tick only | passes between external/config changes |
| Bars first paint, final startup | 143.9 ms after core's post-spawn marker; process age 164.4 ms | about 73 ms after spawn | **fails** |
| Kill shell to restored bars | 571.7, 473.3, 369.7 ms; median 473.3 ms | <=300 ms | **fails** |
| Taskbar after shell kill | observed restored in 3/3 trials, followed by bars in 3/3 | restored | passes |
| Panic hotkey | zero owned processes remaining; both native taskbars visible | restored | passes |
| Alpha hit tests | six real Windows hit tests pass on the two monitors | bodies hit, gaps pass through | passes |
| Style goldens/click tests | six PNGs; all focused tests pass | pass | passes |

CPU/RAM samples use the required `measure.ps1 -Seconds 60`. The clean idle median combines
`n2-bars-idle-1`, `n2-bars-idle-3` and `n2-bars-idle-replacement-1`. The original second sample
(0.60%) contained an external Explorer restart at 21:21 UTC and is retained as an interrupted
sample, not counted as idle. A separate widget fixture process also appeared during part of that
run; no widget cost is included in the shell row. The final replacement run had only this core,
shell and image engine. The three maximized samples above remain a failed/provisional bar result;
the final single maximized recheck measured 0% and 21 MB. The last journal fix affects recovery,
not the idle drawing path.

Raw artifacts are under `%TEMP%\mypaper-tools`: `n2-bars/{idle,maximized}-{1,2,3}.txt` and
`n2-bars/live.log`, plus `n2-bars-replacement/{idle-1,maximized-1}.txt`, `live.log`,
`recovery.json`, `panic.txt`, `alpha-hits.txt` and `modules.txt`. The required measurement tool's
JSON files are alongside those directories (its repeated maximized-1 label was overwritten by
the final one-off recheck; the original three text reports are preserved). `native-0.png` and
`native--2560.png` in each directory show the native bars; `clock-click-0.png` in the first run
confirms a real clock click opened Windows' notifications/calendar. The harness closes that
panel, removes its maximized test windows, restores minimized windows/cursor, stops its own
processes, restarts the user's installed core and releases the lock in `finally`.

Validation: shell 14 passed; bars 103 passed and one machine-index test ignored; UI 16 unit tests
and its separate no-vendor-GPU test passed; core 15 passed with three Task Scheduler tests ignored.
Total: **149 passed, four ignored**. Focused check/clippy (`-D warnings`) pass. Release builds were
limited to shell/core and their dependencies.

**Not merge-ready:** first paint, maximized CPU and crash recovery still miss their gates.
Real glass appearance, plate latency while moving, and the full rain-city/six-widget CPU/RAM gates
are unmeasured until the shared-memory transport, engine and N4 are integrated. `tools/perf` and
`scripts/deploy-local.ps1` still refer to the removed bars binary/page in this base; these files are
outside this slice's ownership and need updating during orchestration.

The final cleanup exposed a separate integration issue: the main checkout's rain-city manifest
now has the new shader `images` field, while its installed wallpaper binary still uses the old
schema and exits on that field. The installed core was restored with a **run-only** absolute theme
override to this worktree's compatible `themes/rain-city` assets; the saved configuration and the
main checkout's theme were left untouched. Install the matching engine and relaunch normally before
removing this worktree.

Small changes outside the source ownership list: `crates/shell/Cargo.toml` and `Cargo.lock` add the
services/native SVG dependencies; `crates/shell/tests/golden/bars/*.png` hold the six required style
fixtures. No widget or engine source was edited.
