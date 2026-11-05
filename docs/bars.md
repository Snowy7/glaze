# Native top bar, dock and taskbar

`mypaper-shell` draws the bars with `mypaper-ui`: software Direct2D/DirectWrite into cropped,
premultiplied layered windows. `crates/bars` (`mypaper-bars`) is a **library of Windows services**,
configuration and presets. It has no executable, browser, graphics device, page or JSON message bridge.
`bars/shell` and the old host, capture, blur and first-paint stand-ins have been removed.

## Processes and recovery

Core waits for Explorer (`wait_for_shell`), stops a leftover shell (`stop_leftover`), then starts
`mypaper-shell` immediately, before starting the wallpaper or setting up its own tray/styling.
The only supervised children are the wallpaper engine and the native shell.

The shell takes `Local\mypaper-shell`, reads `bars.json` and presents each bar as soon as its
window is created. Remaining window creation, provider hooks, icon workers, battery/network/audio,
tray hosting, app-bar negotiation and taskbar changes cannot precede the first present. The bars
use the UI's deferred-accessibility constructor: `WM_GETOBJECT` creates the adapter from the latest
hit snapshot. WARP and the fonts used in the first display list still belong to the first frame.

After painting, the shell takes `Local\mypaper.taskbar.journal` for its lifetime, replays any stale
journal and applies the configured taskbar mode. Core takes this lease only after the old shell
exits. It restores taskbar visibility first, spawns the replacement immediately, then finishes
restoring auto-hide/alignment while the replacement paints. The replacement waits for the lease
before touching Explorer or the journal. Startup failure follows the same serialized restoration.

`WM_CLOSE` to the message-only `mypaper.shell.inbox` asks the shell to exit. On an ordinary exit it
stops its tray host, clears plates, removes reservations and restores the taskbar. It also exits if
its parent process exits. Core replays `%LOCALAPPDATA%\mypaper\taskbar.json` **only after the shell
has exited** (including shutdown/panic), with the replacement held at the journal lease, so both
processes never write it concurrently. Shell crashes restart with zero backoff, overlapping the
remaining replay, with the existing five-crashes-in-a-minute
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
| notifications | Retained Windows toast count when live delivery is qualified; opens the notification center |
| t3, obs, docker, tailscale | Optional shared native integration providers; [settings and qualification gates](integrations.md) |
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

## Flyouts (N3)

Every panel-opening bar click calls exactly:

```rust
flyouts::open(kind: flyouts::Kind, anchor_rect: mypaper_ui::Rect, monitor: &mypaper_desktop::Monitor)
```

The anchor is in physical **screen** pixels. `Kind` is Calendar, Quick, Wifi, Mixer, Notifications,
Launcher, Power, TrayOverflow or Language. The request is queued and `Flyouts::process` (called
by the shell's loop after each pass over the input) opens it in the same pass. Every one is native
(`crates/shell/src/flyouts/`); none opens a Windows panel any more.

| Kind | What it shows | Provider |
|---|---|---|
| Calendar | today, the time (a minute tick while open), a month to page through (`mypaper_ui::calendar::MonthGrid`) | Windows NLS names |
| Notifications | Retained Windows toasts with app identity, optional text, dismiss/open-app and clear-all; the month underneath. Clock clicks open this. | One shared integration lease; Windows change events after qualification, otherwise snapshot on open/manual refresh. No polling. |
| Quick | Wi-Fi and Bluetooth tiles (Wi-Fi's chevron opens the Wi-Fi list), power mode (cycles), battery saver, do not disturb, focus, night light (their settings), brightness, volume (mute; chevron opens the mixer), battery | `quick` lane, the endpoint volume |
| Wifi | the radio switch, networks with signal, security and lock; Connect / Disconnect; a network that needs a password goes to **Windows' own prompt** (`ms-availablenetworks:`), enterprise ones to Settings; location-access and no-adapter states | `wifi` lane, refreshed by its scan notification |
| Mixer | output devices (switching the default needs `audio.switchOutput`, else Sound settings), master volume, each app's volume and mute | `mixer` lane, refreshed by session events |
| Language | the installed layouts, the front app's checked; choosing switches it | `flyouts::language` |
| Power | lock, sleep, sign out, restart, shut down; the last three ask first | `system::power` |
| TrayOverflow | the unpinned tray icons; left and right clicks go to the app as from the bar | `Bars::tray`, `Bars::click_tray` |
| Launcher | a windowless RichEdit search field (IME, Arabic, CJK) over the Start menu's apps; Up/Down choose, Enter runs, recent first with no query | `apps.json` index (rebuilt on the `apps` lane when missing or a day old), `launcher.json` history |

- **Win, Win+N, Win+A** open the launcher, notifications and quick settings, through the existing
  `winkey` hook, installed while `launcher.winKey` is on and the taskbar is hidden; a fullscreen app
  in front keeps the key for Windows. Pressing the same shortcut again closes it.
- **One at a time, never overlapping.** One reused popup window (created hidden at start: a new
  layered window costs ~20 ms). Opening another replaces it; the same kind again closes it.
  Escape, a click elsewhere (deactivation) and handing over to Windows close it.
- **Placement:** centred under (or over) the clicked module, a gap from the bar's band, inside the
  work area; notifications at the right edge; the launcher centred high on the monitor.
- **Motion:** position and alpha only, on a spring (~0.35 s), on a ticker armed only while it
  moves. It is visible from its first frame (alpha starts at 45%).
- **Glass:** for the liquid style the body is engine glass (more tinted than the bars' clear
  glass, `look.flyouts.tint`, default 0.5) under an invisible `Rgba::HIT` fill; solid and mono
  draw their own fill. Menus (the dock's and the desktop's) use the same material. See
  [ui.md](ui.md) "Glass under a surface" and engine-notes "Glass overlays".
- **Data never blocks the first frame.** Panels draw at once from what they have; lane replies and
  worker news redraw them. Slow set-up (the endpoint volume, the keyboard layouts, launcher
  icons) runs right after the window is shown (`Panel::opened`). Lanes stop 5 s after a close;
  nothing polls while closed.
- **Panels** implement `flyouts::Panel` (`width`, `view(&mut Kit)`, `activate`, `context`,
  `slide`, `wheel`, `reply`, `changed`, `key`, `tick`, `opened`, `refresh`, `field`,
  `text_changed`, `submit`) and lay out with `flyouts::kit::Kit` (header, rows, tiles, sliders,
  switches, buttons, links, notices). Each control always emits the same ops, transparent at
  rest, so a hover redraws only that control.

**A bar over a window (handed over).** Bars draw plates; while an app window overlaps a bar its
plate is hidden behind that window. The switch is one call on the bar's surface, with no new API:
`surface.set_glass(Some(Glass { body, radius, material }))` while a window overlaps it (and that
bar publishes no plate), `set_glass(None)` when none does. An overlay is one rounded rectangle per
window, so islands need one surface per island or stay on plates. This lives in `bars/**`
(the bars follow-up agent's) and is not wired in this slice.

### Live N3 measurements (2026-11-05)

Release core, shell and engine from this worktree under the desktop lock; fixture `APPDATA` (a still
photo, liquid bars on both 1440p monitors, every module), `%TEMP%\mypaper-tools\n3-flyouts-live.ps1`.
Content time is from the injected button release to the flyout window being visible, measured
from another process; glass is the engine's own open-to-glass log. A cold click comes after 8 s idle
(capture cold) with an 80 ms press; warm clicks follow 0.4 s after a close.

| Check | Measured | Gate |
|---|---|---|
| Content on screen, cold click | 2.7–5.6 ms; launcher 17.7 ms | ≤30 ms |
| Content on screen, warm click | 1.9–4.6 ms | ≤30 ms |
| Win+N / Win+A / Win | 8.9 / 5.8 / 6.1 ms | |
| Glass, cold click | 166–252 ms in 13 of 16 cold clicks (two runs); 326, 351 and 354 ms in the other three | ≤300 ms: **misses 3 of 16** |
| Glass, warm | 42–58 ms | |
| Redraws once settled | 0 for every flyout (1 when the notification list or app index arrives after the motion) | 0 |
| Shell private MB, a flyout open vs closed before it | +0.2 to +2.8 MB (launcher the most) | ≤ +6 MB |
| Shell private MB vs the pristine shell | 29.4 before any flyout; 35.9 with the first; 36.7 after all closed and lanes stopped | freed on close |
| Overlay CPU, an animated window behind quick settings | engine 1.8–2.1% of a core (~25 updates/s), shell 0.2–0.4% | ≤3% |
| Overlay GPU per update | 0.12–0.15 ms (copy, compare, glass; `MYPAPER_OVERLAY_TIMINGS`) | ≤0.5 ms |
| Overlay once closed | engine 0–0.47% (no glass pass); +24 MB while open, freed after the 3 s warmth | 0 |
| Capture border | 0 yellow edge pixels | none |
| Screenshots | a GDI screenshot shows the popup (mean difference 316 in its rectangle) | shown |
| Launcher typing | Latin; Arabic on the Arabic (101) layout (`لاتنم`); CJK as text (`日本語 中文 한국어`) | |
| Dock menu with glass | glass in 174 ms | |

- The pointer-down warm-up was built, measured and **dropped**: with an 80 ms press it brought cold
  glass to 35–75 ms but stalled the click's delivery to the shell by ~30 ms (cold content 22–45 ms).
  Without it the content gate holds, and cold glass is WGC's first frame after `StartCapture`
  (N0.2 measured 170–340 ms): usually under 300 ms, not always. Keeping the glass pipelines across
  cold starts did not change the outliers. The next step is a warm-up that starts **before** the
  press: on a short hover over a module that opens a flyout (bars' hit under the pointer), so the
  capture's start never overlaps a click.
- 0.47% for the engine once closed was the window-cover events of the animated test window; its
  overlay log shows no glass pass then.
- The shell keeps ~7 MB after its first flyouts (DirectWrite fonts, WARP surfaces, Msftedit, the
  heap's high-water); each later open adds ≤2.8 MB and returns it.
- No CJK IME is installed on this machine: CJK went in as committed text (Unicode input), not
  through an IME's composition window. Arabic used the real keyboard layout.

## Plate integration seam

`crates/shell/src/glass.rs` is the only publication boundary:

```rust
struct Material { clarity: f32, blur: f32, refraction: f32, highlight: f32, tint: Rgba }
struct Plate { id: u64, rect: Rect, radius: f32, material: Material }
enum Source { Bars, Widgets }
fn publish(source: Source, monitor: &Monitor, plates: &[Plate]);
```

Plate rects are physical pixels **relative to the monitor**, not screen/DIP coordinates. A call
replaces that source's list on that monitor; `[]` removes it. The shell publishes after each frame whose
geometry/material changed, including every moving dock frame, and publishes nothing at rest.
It clears lists on suspension/removal/shutdown. `Bars::plates(&Monitor) -> Vec<Plate>` exposes the
current bar list. The merger retains both sources on every monitor and owns one thread-local
`mypaper_glass_ipc::Writer`. It converts to physical virtual-desktop pixels and replaces the whole
snapshot atomically. Explicit widget HWND ids are preserved; auto bar ids include the source,
monitor group and shape index, so they never repeat on a second monitor. Identical combined
snapshots do not publish. More than `MAX_PLATES = 96` is rejected without truncating a source.

Bar material comes from resolved style tokens: `look.glass.clarity` (0..1), `blur` (0..32 DIPs),
`refraction`, `highlight` and the appearance tint at `glass.tint`/`look.tint` opacity. Without explicit
clarity, the existing inverse-tint default remains. Blur is scaled to physical pixels and bounded
to the IPC's 32-pixel maximum; geometry and radii are already physical. Mono/solid publish no plates.
There is no shell JSON transport or engine-side spring.

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

### N2 gate follow-up after N4 integration (2026-11-05)

Base: `native` at `33f081a`, merged before continuing. Implementation commits:
`0164067` (deferred startup, journal handoff, scoped bar hooks and glass/material fixes),
`d76b414` (coverage watcher ignores child-window motion), `fb9e3d6` (retain suspension while
presenting bars incrementally), `890ae82` (ignore non-covering overlay notifications), and
`b0d17b3` (ceil deadline waits). Nothing was pushed or merged back.

The repeatable baseline reproduced recovery at 470-569 ms. Core already had **zero restart
backoff**: journal replay cost 47-278 ms, spawn-return gaps about 21-26 ms, and shell startup to
all bars cost 166-460 ms. The cold baseline spent 84 ms in prepare/hide, 26 ms starting services,
56 ms reconciling surfaces and 67 ms laying out/presenting them. The first top layout took
2.45 ms and layout plus present 30.95 ms (the latter includes WARP target initialization,
font rasterization, drawing, copy and showing). Creating a window cost about 14-30 ms; class
registration cost tens of microseconds. Recreating windows during recovery was slower still.

The callback trace recorded 299 location events over roughly 30 seconds, including child HWNDs
and discarded sub-objects. Bar location hooks now follow the foreground process and Explorer;
movement no longer dirties all bar data or re-hides the taskbar on an unrelated data update.
The separate shared widget coverage watcher was also treating child HWND moves as coverage
changes, causing desktop enumeration/restacking. Its hierarchy filter has a real parent/child
HWND test. The later `890ae82` change also excludes non-covering overlay styles, matching the
coverage scan itself. The bars' service objects now cost about 0.2 ms before paint and start their actual
subscriptions afterwards; UI Automation's bar adapters wait for WM_GETOBJECT. No service or
journal/Explorer call is inserted ahead of the first visible frame.

`b0d17b3` fixes a separate scheduler bug: the bar timeout truncated future sub-millisecond
deadlines to zero and could busy-poll until they became due. A focused boundary test first
failed with 0 instead of 1, then passed after rounding up. Widgets already use this conversion.

#### Integrated gates

Release shell/core/engine from this checkout; desktop lock held through setup/interactions and
by the required `measure.ps1 -Seconds 60` for each sample. Two visible 2560x1440, 96-DPI displays,
rain-city at its configured 30 fps / 0.65 scale, liquid top/dock on both screens, six native
widgets (clock, calendar, paused media, weather, system-monitor, notes), minute clocks and no
flyout. No compilation ran during the samples. The latest complete three-sample set is
`n2-bars-gates-ceil`; earlier attempts are retained separately and are not mixed in.
CPU below is percent of one core; memory is private MiB rounded by the measurement tool.

| Gate | Latest measurement | Target | Result |
|---|---|---|---|
| Shell private memory, idle | 32 MB median (32, 32, 32); maximized 27-28 MB | <=40 MB | pass |
| Shell CPU, six widgets visible | 0.18% median (0.21, 0.18, 0.18) | <=0.1% | **fail** |
| Shell CPU over maximized apps | 0.05% median (0.00, 0.05, 0.08) | <=0.05% | pass by plan's median; one sample above |
| Spawn call begins -> first visible bar | 276.2 ms; process age 136.3 ms | about 73 ms | **fail** |
| Spawn call begins -> all four bars painted | 334.7 ms; process age 194.8 ms | about 73 ms | **fail** |
| Kill -> all bars' paint marker | 658.1, 663.3, 487.4 ms; median 658.1 ms (native kill timestamp) | <=300 ms | **fail** |
| Taskbar restored after kill | observed in 3/3; unveiled/visible observation bound 214.4-280.8 ms | at once | **not established** |
| Settled bar redraws | zero dock redraws; two top updates per minute, one per screen | minute tick only | pass for bars |
| Hardware GPU driver in shell / GPU memory | none / 0 MB; software WARP allowed | none | pass |
| Owned steady processes / WebView2 | 3 / 0 | 3 / 0 | pass |
| Core private memory / CPU | 2 MB / 0.00% median | <=8 MB / about 0% | pass |
| Engine private memory | 449 MB median | <=458.1 MiB | pass |
| Whole product private memory | 483 MB median | <=506.1 MiB | pass |
| Rain-city engine CPU | 4.09% median (5.10, 4.09, 3.28) | <=4% | **fail** |
| All plates GPU time | 0.489 ms combined median | <=0.3 ms | **fail**, engine follow-up |
| Rain-city wallpaper GPU time | 4.33 / 3.90 ms median per screen | <=2 ms per screen | **fail**, engine follow-up |
| Goldens and focused click tests | 166 passed across affected crates; 5 intentionally ignored | pass | pass |
| Real Windows alpha hit tests | six of six on both monitors | bodies hit; gaps pass through | pass |
| Panic cleanup | 0 owned processes; 2 native taskbars visible | restore | pass |
| Moving-dock glass | primary publish -> submit p95 5.50 ms; DWM completion bound p95 10.15 ms | <=one frame (about 4.17 ms primary) | **not established** |

**The spawn definition matters.** The previous report used core's *post-spawn* marker. The new
`shell_spawn_begin_ms` marker is immediately before `Command::spawn`; `shell_spawn_ms` remains
after its return. In this run that call cost 166.9 ms. The child began running before the call
returned, so spawn duration and child process age must not simply be added. The first-visible
and all-bars markers are kept separate; an earlier first bar is not counted as all bars back.
The reference first-paint gate remains missed even when using process age alone.

Recovery still has zero intentional backoff. Its replacement process ages at the all-bars
marker, plus the pre-creation remainder (old-process teardown, taskbar reveal and spawn), are
recorded in the raw log and recovery JSON. The lease prevents concurrent journal writes and
moves reveal before the remaining Explorer replay, but does not prove instant restoration or
the <=300 ms gate on this integrated fixture. The final recovery harness timestamps immediately
before native `TerminateProcess`, with the same precise FILETIME clock as the READY markers;
it also excludes an alpha-zero veiled taskbar from its visibility observation. These trials'
shell startup ages to all-bars paint were 204.8, 204.1 and 358.6 ms. The corresponding pre-creation
remainder (old-process teardown, reveal and spawn) was 453.3, 459.2 and 128.8 ms. This isolates
the failed gate from restart backoff; zero backoff alone is insufficient. The earlier
kill-command-bound set is retained (504.2, 586.3, 423.7 ms), not substituted for the final trials.
An additional sequencing comparison completed Explorer replay before respawning: recovery was
916.4, 898.5 and 880.0 ms. That variant was discarded; the committed sequence retains overlapping
renderer startup and journal replay. This comparison used the same native kill clock and six
widgets, with the same first-paint and all-bars markers.

System-monitor redraws at its configured two-second cadence, so the whole six-widget shell
cannot be described as making only minute-clock redraws. In the same final process, removing
the six widget instances through the watched layout, settling and sampling for another 60 s
measured **0.05% shell CPU**. The original layout was then restored before recovery testing.
That compares with the visible-widget median of **0.18%**, and the covered-widget median of
**0.05%** over maximized apps. This supports visible widget work as the remaining idle cost,
consistent with N4's two-second system-monitor sampling/painting profile; it does not isolate
each widget's contribution. No widget service or sampler was changed. An earlier fresh-process
empty-layout control measured 0.21%; it is retained as a separate diagnostic and was not used
to claim attribution or a three-sample gate pass.

The later `n2-bars-gates-overlay` attempt is not an idle baseline: it contains more than 800 dock
animation redraws and a covered screen. Its raw reports remain available; those intervals were
not mixed into the final `gates-ceil` medians. The final settled bar log has no dock redraws
between fixture transitions and only its minute-aligned pair of top-bar updates.

#### Real glass and remaining engine gates

Verified visually: both monitor screenshots from `n2-bars-gates-motion`, its magnified
`dock-moving-0.png`, and the real clock click's Windows notifications/calendar screenshot from
`n2-bars-gates-precise-recovery`. Bar, dock and widget plates coexist without duplicate ids or
source replacement. The engine timing stream's plate sequence is unchanged for **10.50 s**
before motion and **1.95 s** after settling, even while rain-city continues rendering. Motion
produces fresh snapshots; there is no idle publication timer.

This base contains no engine hillclimb lag fix. The primary dock was moved across its icons;
`tools/perf/summarize-gpu.py --warmup-seconds 6` reports 320 geometry-only primary frames:

| Primary dock geometry delivery | Median | p95 | Max |
|---|---:|---:|---:|
| Publish -> queue submission | 2.98 ms | 5.50 ms | 9.21 ms |
| Publish -> DWM completion bound | 7.56 ms | 10.15 ms | 29.33 ms |

The primary display's nominal 240 Hz frame is about 4.17 ms. `DwmFlush` is a conservative
completion bound for the engine's queued work, not a relative shell-versus-glass scanout
measurement; it changes pacing and was enabled only in this separate diagnostic. The <=one-frame
visible-lag gate is **not established**. The secondary dock was visually inspected at rest;
its one geometry-only row is not presented as a secondary motion measurement. No engine source,
spring, quality setting or frame scheduling was changed to improve these figures.

For context, the ten real plates cost **0.489 ms GPU combined median / 0.548 ms p95** in the
timestamp run, above the plan's <=0.3 ms gate. Wallpaper GPU medians were 4.33 / 3.90 ms per screen,
also above <=2 ms. These are engine follow-ups, not shell CPU measurements or claimed passes.

#### Reproduction, checks and handoff

Raw artifacts live under `%TEMP%\mypaper-tools`:

- `n2-gates-ceil-live.ps1 -Measure -Suffix '-gates-ceil'`, its `n2-bars-gates-ceil` directory,
  and `measure-n2-bars-idle-gates-ceil-{1,2,3}.json`,
  `measure-n2-gates-maximized-gates-ceil-{1,2,3}.json`,
  `measure-n2-gates-empty-layout-gates-ceil.json`.
- `n2-gates-native-precise.ps1 -Interaction -Suffix '-gates-precise-recovery'`: native kill
  timestamps, `recovery.json`, `alpha-hits.txt`, `clock-click-0.png`, `panic.txt`, and `live.log`.
- `n2-gates-native-precise.ps1 -Timing -SkipRecovery -Suffix '-gates-motion'`: `timing.jsonl`,
  `summary.json`, both `native-*.png` monitor views and `dock-moving-0.png`.
- `n2-gates-native-precise.ps1 -Suffix '-gates-sequential'`: discarded replay-before-respawn
  comparison, with `recovery.json` and `live.log` in `n2-bars-gates-sequential`.

Targeted check and strict clippy pass for shell, bars, core, UI and desktop. Strict all-target
clippy passes for shell/core/UI/desktop. Including **bars' tests** in all-target strict clippy
still encounters four pre-existing `items_after_test_module` errors in
`bars/src/flyouts/{clipboard,quick}.rs`, `bars/src/icons.rs` and `bars/src/system.rs`; those were
not rearranged across the concurrent service work. Tests: shell 24 passed/1 ignored; UI 19 unit
plus its no-vendor-driver integration test; bars 106/1; core 15/3; desktop's focused real-HWND
coverage test 1/0. Total **166 passed, five ignored**. Goldens were not re-blessed.

The DWM-with/without-shell differential and a native flyout's open/close memory/latency gates
were not remeasured in this follow-up. The clock click opens Windows' own calendar; it is not
reported as a native flyout timing. Those gates remain for the engine/flyout integration runs.

One source path outside ownership: **`crates/desktop/src/cover.rs`**, to filter child HWND and
non-covering-overlay notifications in the shared watcher used by widgets and the engine; its
focused callback test uses real hidden HWNDs. No flyout, widget or wallpaper source was edited.
UI changes preserve existing APIs and add the deferred-accessibility constructor; two safety
comments in the existing ticker test also fix inherited strict-clippy diagnostics.

All fixture processes were stopped and the original user core was restarted in `finally`;
minimized windows and cursor were restored, and the desktop lock released. Native taskbars were
observed restored before restart. As before these tests, the user's existing `taskbar.mode: hide`
configuration hides them again when their original shell starts. Saved configuration was not
rewritten to force a different desktop preference. Thus the literal stock-taskbar-visible exit
condition is not claimed while that original configuration is active.

Exact interfaces for the next slices:

```rust
// shell::glass (existing merger boundary):
pub fn publish(source: Source, monitor: &Monitor, plates: &[Plate]);
// Source::{Bars, Widgets}
// Plate { id: u64, rect: Rect, radius: f32, material: Material }
// Material { clarity: f32, blur: f32, refraction: f32, highlight: f32, tint: Rgba }
// Monitor-local physical pixels; id 0 = auto; explicit HWND ids retained.

// mypaper_ui::Surface:
pub fn new_deferred_accessibility(kind: Kind, name: &str, role: Role) -> anyhow::Result<Self>;
// mypaper_bars::taskbar::JournalLease:
pub fn acquire() -> anyhow::Result<Self>;
// mypaper_bars::taskbar::Taskbar<S>:
pub fn restore_visibility(&mut self) -> anyhow::Result<bool>;
// Lease is thread-owned and not Send. Reveal preserves the remaining undo entries.
// style::GlassStyle adds clarity: Option<f32>, validated in 0..=1.
```

No new messages or flyout service APIs. **Not merge-ready:** visible-widget idle CPU, first
paint, recovery/instant restoration and the engine delivery/GPU gates remain open.
