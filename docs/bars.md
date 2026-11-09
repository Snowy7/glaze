# Native top bar, dock and taskbar

`glaze-shell` draws the bars with `glaze-ui`: software Direct2D/DirectWrite into cropped,
premultiplied layered windows. `crates/bars` (`glaze-bars`) is a **library of Windows services**,
configuration and presets. It has no executable, browser, graphics device, page or JSON message bridge.
`bars/shell` and the old host, capture, blur and first-paint stand-ins have been removed.

## Processes and recovery

Core waits for Explorer (`wait_for_shell`), stops a leftover shell (`stop_leftover`), then starts
`glaze-shell` immediately, before starting the wallpaper or setting up its own tray/styling.
The only supervised children are the wallpaper engine and the native shell.

The shell takes `Local\glaze-shell`, reads `bars.json` and presents each bar as soon as its
window is created. Remaining window creation, provider hooks, icon workers, battery/network/audio,
tray hosting, app-bar negotiation and taskbar changes cannot precede the first present. The bars
use the UI's deferred-accessibility constructor: `WM_GETOBJECT` creates the adapter from the latest
hit snapshot. WARP and the fonts used in the first display list still belong to the first frame.

After painting, the shell takes `Local\glaze.taskbar.journal` for its lifetime, replays any stale
journal and applies the configured taskbar mode. Core takes this lease only after the old shell
exits. It restores taskbar visibility first, spawns the replacement immediately, then finishes
restoring auto-hide/alignment while the replacement paints. The replacement waits for the lease
before touching Explorer or the journal. Startup failure follows the same serialized restoration.

`WM_CLOSE` to the message-only `glaze.shell.inbox` asks the shell to exit. On an ordinary exit it
stops its tray host, clears plates, removes reservations and restores the taskbar. It also exits if
its parent process exits. Core replays `%LOCALAPPDATA%\glaze\taskbar.json` **only after the shell
has exited** (including shutdown/panic), with the replacement held at the journal lease, so both
processes never write it concurrently. Shell crashes restart with zero backoff, overlapping the
remaining replay, with the existing five-crashes-in-a-minute
limit. Already-enabled native auto-hide is left under the user's ownership: its flag is journaled
only if Glaze changes it. This avoids restoring a setting we never changed.
Ctrl+Alt+Shift+Backspace remains core's panic hotkey and works independently of the UI thread.

## Configuration and presets

`%APPDATA%\glaze\bars.json` is watched live. Unknown fields, invalid ranges and duplicate modules
remain errors; an invalid edit leaves the last valid configuration running. Writers use
`glaze_bars::config::edit`, which validates and writes atomically while preserving unrelated fields.
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

Dock right-click menus read the app's native Windows jump list on a worker when opened.
Pinned destinations, Recent/Frequent, custom categories and Tasks are flat sections with muted,
non-interactive headers above the app's
launch, Pin/Unpin and Close commands. Shell links retain their complete serialized target,
arguments, working directory and shell properties; Windows executes them. Missing lists show
only the basic commands. AutomaticDestinations are read as memory-backed OLE storage, including
DestList versions 1–4 and 6; CustomDestinations use bounded shell-link framing. Destination paths
are never logged. Frequent/Recent show up to ten unpinned entries; pinned destinations are not truncated.
Custom lists show Recent/Frequent only when the app explicitly requests them.

Rows load their shell icons on the same worker after the first list delivery, at the menu's
physical DPI size. Serialized links honor `IconLocation` before falling back to their target;
shell items use `IShellItemImageFactory`. A bounded session cache keys icons by destination/link
and size. The second delivery updates open rows without replaying opening motion. Jump lists use a fixed
280 design-pixel width and ellipsize long labels. They omit per-window rows and redundant New
window commands (the app launch row and native Tasks provide those actions).

Bar menus attach to the rendered icon and stay outside the current visible bar bounds, including
magnified icons and reveal transforms. They open away from each dock edge with a six-pixel scaled
gap, clamp to the monitor work area, and retain that edge as asynchronous rows arrive. Tray and
bar flyout menus use the same placement. Empty categories are omitted, matching [Windows taskbar
guidance](https://learn.microsoft.com/en-us/windows/win32/shell/taskbar-extensions).

Windows taskbar pins seed the dock only when `dock.pinned` is absent. An explicit list is owned
by the user and does not continuously import newly pinned taskbar apps. Pins always render,
including while stopped. Two explicit AppUserModelIDs must match before windows can merge into
a pin; a shared Chromium executable alone cannot merge a browser and its PWAs.

The bars are drawn with the UI kit ([ui.md › The kit](ui.md#the-kit)): `bars/look.rs` makes one
kit `Theme` per bar, and `bars/view.rs` draws the top bar, islands, dock, tray, workspace dots,
media, clock, status text and tooltips with `kit::Frame` from it. `look.style` and `look.family`
pick the kit style:

| `look.style` | `look.family` | kit style |
|---|---|---|
| liquid (default) | macos, mono | Liquid |
| liquid | fluent | Fluent |
| liquid | material | One UI |
| solid | any | Solid |
| mono | any | Mono |

`look.family` also decides the menus and flyouts; Fluent groups network/volume/battery clicks
into quick settings. `look.scheme` (or the theme's text tone) picks dark or light, and the accent
is the theme's or Windows'. `look.glyphs: fluent` draws module icons from Segoe Fluent Icons.

- **Body.** Over glass (liquid, fluent, oneui) the shell fills only `Rgba::HIT`; the engine's plate
  draws the glass and its one edge. Mono and solid draw the theme's surface (solid adds its one
  hairline); `look.background` as a solid colour replaces it; performance mode drops the glass and
  draws the theme's surface. No outline is drawn on glass.
- **Items** are kit washes (capsules in liquid and One UI, the theme's radius elsewhere), always in
  the list and transparent at rest, so a hover redraws one item. Keyboard focus draws the kit's
  focus ring; a click never does. Text is the theme's body role (`look.font` and module styles
  override family, size and weight); colours are the theme's label steps. The minimal design
  (no body, straight on the wallpaper) puts a soft shadow copy under its text.
- **Icons.** Module glyphs are the kit's icons (Lucide vectors, or the Fluent font). A user icon
  pack in `%APPDATA%\glaze\icons` still replaces them by key; the bundled `mono` pack draws app
  icons only. Every bitmap (tray icons, app icons, pack SVGs) is fetched at the exact device size
  it is drawn (`Images::sized`: SVGs rasterized at that size, larger bitmaps area-averaged down)
  and placed on whole pixels, so icons are crisp at every DPI and tray icons are evenly spaced.
- **Dock.** Icons at `dock.size`; magnification as configured; a pressed icon sinks 3% and
  darkens (the kit's press state); running dots in the padding below the icon (label for the
  active app, secondary for the others, warning for attention, or `look.indicator` colours);
  hairline dividers between kept and other apps and before the trash, as macOS draws them;
  tooltips with the app's name. The taskbar style keeps Windows' washes.
- **Menus** the bars open (dock, tray) and the desktop menu use the bar theme's `theme.tokens()`.

The appearance layer is resolved when configuration changes: `surface: none` removes the bar's
body, `solid` draws the tint at `opacity`, and `glass` uses the style's engine material. An explicit
`look.background` or `look.transparent` wins over that surface choice. `appearance.font` supplies
the font under `look.font`; `appearance.scale` multiplies monitor DPI for content, geometry and
reservations. Radius, text tone, accent and blur reach bars and popup themes; popup fonts and
content scale also follow them. `shadow: false` removes bare-bar text shadows and menu shadows.
An explicit solid background draws its own colour without a glass plate. Borders apply to drawn
surfaces; glass keeps the engine's single edge.

Every `system-accent-light-1..3` and `system-accent-dark-1..3` colour token uses its actual Windows
palette entry (separate from a theme's `accent`). `wallpaper` backgrounds keep the wallpaper plate
even when a window overlaps the bar; `blur` backgrounds always use the live overlay. Background
brightness and `look.saturation` tune the engine's backdrop; background blur adds to the material's
frost. Glass `style: none` disables the material, `frosted` removes lensing, and `intensity` scales
refraction, highlight, frost and explicitly set sheen/depth. Theme
`bars.glass` is a partial layer under the bar's glass controls; removing fields restores defaults.

### Native state and motion controls (2026-11-06)

Appearance values override design defaults; explicit shared, per-bar and monitor `look` values
still win. Font, scale, radius, tone and glass resets therefore work with a preset selected too.
`look.items` resolves rest, hover and active fill, text colour, scale, border and radius. Hover and
active inherit rest fields. Flat surfaces also consume per-state `look.border` and `look.shadow`;
glass draws neither of those UI decorations. Built-in `var(--fill-1..3)`, `var(--edge)` and
`var(--t0..3)` use the native scheme's palette, and named colour choices are cached on reload.

Motion resolves structural defaults (`animation`, `bar`, `top`/`dock`), then the named leaf at each
level, in that order. A disabled ancestor wins. `reduceMotion: never` overrides Windows' animation
preference; performance `on`, or `auto` while on battery, still disables motion and glass. The same
policy, duration and curve reach flyouts and menus. Hover/press transitions, dock magnification,
launch bounce and attention have separate resolved leaves; show/hide does not take `open` timing.
Named springs fit an explicitly configured duration; a physical `{mass, stiffness, damping}`
spring uses its natural bounded settling time. Every transition ends at the exact final value.

Flyout and menu `open` styles support fade, slide (without glass), pop, scale, morph and none.
Pop/scale/morph keep a bitmap only during opening, transform its hit regions and engine glass
body together, then restore the original display list and free the bitmap. Bar show/hide consumes
slide/fade/pop/scale/morph/none separately from popup opening; pointer coordinates follow the
same inverse transform as the pixels. Glass popup slides retain the stationary fade to avoid
repeated overlay capture flashes. The engine fade now takes the resolved duration (up to the
wire's 2,000 ms limit), or zero for reduced motion, disabled motion and presentation `none`.
The engine still uses its cubic ease-out for that separate glass fade; the shell's content follows
the configured curve. Motion accepts durations up to 5,000 ms, so the complete C14 contract remains
partial. Continuous corners and the finer glass controls are described below. Windows style
synchronization is implemented (C25).

Flyout kit controls and menu rows now consume the hover/press leaves too. Their sampled washes
keep state only for entered controls, remove resting entries, and stop the frame source when
settled. `glaze_ui::interact::motion::InteractionMotion`, `Frame::with_motion` and the menu's
optional interaction timings add this behavior without changing existing kit callers. Configured
flat-surface shadows reserve a bounded outset, including for cropped islands and docks; their
screen anchors remain unchanged.

### Finer glass controls (2026-11-06)

The shell resolves optics with each surface's look, caches the IPC plate style and popup tokens,
and copies them when publishing or opening. It uses `Writer::publish_styled` for wallpaper plates
and sends `Request::Style` after every overlay `Open`, including a capture warm-up. Style-only
changes invalidate a plate snapshot. No timer or frame work was added at rest.

- `background.brightness` (wallpaper/live blur) and `look.saturation` reach bars, dock, previews,
  flyouts and menus; removing them restores inherited design defaults (neutral 1/1 when unset).
- Blur stays in design pixels until each surface's DPI/content-scale projection; both transport
  paths accept up to 256 physical px and clamp larger projections to that wire limit. The background's
  extra blur is added to the material's frost, including flyouts (unless `flyouts.blur: false`).
- `look.corners: squircle` sends smoothing 0.6 and draws the same superellipse extent/exponent as
  liquid-rust in the kit body and its invisible hit fill. Circular/default surfaces retain the
  existing renderer path. Software contour geometry is cached, bounded to 64 entries per canvas.
  The engine overlay's **DirectComposition clip is still circular**; at nominal radius 24 and
  smoothing 0.6 it can trim about 0.14 px from the shader contour. C10 remains partial until the
  engine owner makes that clip agree with the smoothed shader (plates already agree).
- `glass.sheen` drives the light inside the top rim. `glass.depth` is sent too, but the engine
  draws its broad shade **under the lit top edge**. The legacy bar contract specified an inner
  shadow toward the bottom. That semantic gap remains C12; it is not claimed as parity.
- Widgets do not inherit `bars.json` optics; they retain their own look/material controls and
  publish a neutral style in the same merged snapshot. Their appearance remains unchanged.

The kit additions are opt-in: `glass::GlassStyle`, `Backdrop::open_styled`/`style`,
`Surface::set_glass_style`, menu `Tokens::glass_style`, `Op::Continuous`,
`geom::continuous::surface` and `Frame::fill_continuous`. Existing backdrops inherit the default
trait methods and existing surfaces keep neutral optics and circular corners.

Round-two verification: `cargo check --workspace` and the release workspace build passed; the
final fractional-DPI radius correction also passed the shell tests and release shell build.
Library tests passed: UI 62, shell 143 (10 ignored), bars 118 (one ignored). Clippy for those
libraries passed with existing warnings. New checks cover cached optics/reset inheritance,
motion-policy/2,000 ms wire bounds, the continuous contour's engine coefficients, alpha hit
geometry, translation and unchanged-list damage. The only Studio edit was an explicitly
authorized one-line compile fix: translate `Op::Continuous` like the other rectangle ops.

Verified with DPI-aware captures on both 2560x1440 screens at content scale 1.2: dark/desaturated
squircle glass and bright/vivid circular glass, a reduced-motion launcher, and a 1,200 ms fade
dock command menu (opening and settled). No menu command was invoked. The bar fixtures explicitly
set `look.transparent: false` so the macOS design's default bare top bar does not hide the body
under test. Read-only shared-memory inspection confirmed matching plate/style sequence stamps,
blur 76.8 physical px, brightness 0.55/1.6, saturation 0/3, smoothing 0.6/0 and sheen/depth 0/1.
These are the transmitted controls; the engine's documented 64-point frost ceiling still applies.
The reduced-motion launcher drew twice, with no content transition. Its cold capture took
615 ms to supply glass, which `fade_ms: 0` does not remove. The timed menu's glass arrived in 99 ms.

Artifacts and reproduction scripts are in `%TEMP%\glaze-tools\bars-parity-2`: inspect
`desaturated-squircle-{0,1}-{top,dock}.png`, `vivid-round-{0,1}-{top,dock}.png`,
`{desaturated-squircle,vivid-round}-plates.json`, `launcher-reduced-motion-{opening,settled}.png`,
and `menu-timed-glass-{opening,settled}.png`. Every experiment restored the original `bars.json`
bytes; SHA-256 remained `88032FB713BEDDF3D3B8386859655E7A0E2C05DCBDB2F8E9718C5607C0C61BA0`.
The current shell is deployed under core supervision; the user's Studio remained running.

The prescribed `measure.ps1` samples are three settled 30-second runs per state, with the user's
default config and no popup. CPU is percent of **one core**. Other agents built, deployed and
reconciled the index during this live-desktop run; these numbers establish no idle regression,
not a performance improvement or the native plan's quieter fixed-fixture CPU gate.

| Shell / sample | Before CPU % | After CPU % | Before private MB | After private MB |
|---|---:|---:|---:|---:|
| 1 | 0.52 | 0.10 | 37 | 36 |
| 2 | 0.36 | 0.21 | 37 | 35 |
| 3 | 0.16 | 0.21 | 36 | 35 |
| Median | **0.36** | **0.21** | **37** | **35** |

Raw artifacts are `%TEMP%\glaze-tools\measure-bars2-{before,after}-{1,2,3}.json` and
`bars-parity-2\settled-samples.json`. The sampling shell was PID 67424 after deployment.
The final radius-only fix was deployed as PID 44212. A later agent replaced the executable at
06:28 (PID 79660, a different hash); two additional post-interaction samples on that build were
1.09% then 0.42%, both 36 MB. They are retained as `measure-bars2-after-interactions{,-repeat}.json`
and excluded from the six-sample comparison. No sustained regression was reproduced, and no
diagnostic switch, keeper process or timer was left enabled.

### Night verification (2026-11-06)

The release workspace build passed, followed by normal deployment. Focused library tests passed:
bars 116 (one ignored), shell 140 (ten ignored), UI 60, config 20, theme 15, and widget layering 7.
Clippy for the shell/UI/bars libraries passed with existing warnings. Resolver tests cover preset
versus explicit appearance, monitor precedence, independent Windows accent shades, theme-glass
resets, state inheritance, motion policy and curves. Reveal geometry also checks inverse pointer
mapping; the real-window bar test covers multiple dock plates without island windows.

Verified: DPI-aware captures on both 2560x1440 screens show solid/light Georgia text at 1.1x,
frosted/dark glass and the flat performance-mode surface. Guarded input opened the native launcher
with scale and the dock's commands menu with morph; opening and settled captures were inspected.
No menu action was invoked. Other bar reveal styles have source/math coverage, not a separate
live capture. Artifacts and the reproduction scripts are in `%TEMP%\glaze-tools\bars-parity`:
`solid-light-0-top.png`, `frosted-dark-1-dock.png`, `performance-0-top.png`,
`launcher-scale-{opening,settled}.png`, and `menu-morph-{opening,settled}.png`.

Each configuration experiment restored the user's original `bars.json` bytes. Its SHA-256 is
`88032FB713BEDDF3D3B8386859655E7A0E2C05DCBDB2F8E9718C5607C0C61BA0`.
The user's Studio window stayed open. Engine-owned crates were not edited.

The prescribed 30-second live sampler produced the following shell results (CPU is percent of
one core, not the whole 16-core machine):

| State / artifact in `%TEMP%\glaze-tools` | CPU % one core | Private MB |
|---|---:|---:|
| Before work, `measure-bars-before.json` | 0.42 | 34 |
| After variants and launcher/menu interaction, `measure-bars-after.json` | 0.31 | 37 |
| Restarted, settled default config, `measure-bars-after-settled.json` | 1.98 | 34 |

The default-state memory increase is zero, but the exercised process exceeded the +1 MB
allowance by retaining 3 MB above the cold baseline. These samples do **not** close the memory
gate after interaction or establish ~0 idle CPU. Other live work changed window/service activity
during this run, so the higher CPU sample is not attributed to this patch. A diagnostic minute
reported 356 bar ticker wakes, 187 ms in bar paint and 96 ms in bar services; the frame source was
not continuously busy, but a controlled quiescent comparison remains needed. The immediate
startup sample (2.24%, 33 MB) is retained separately as `measure-bars-after-startup.json` and is
not called idle. No performance improvement is claimed from these unlike desktop states.

Studio follow-ups include exposing the now-consumed appearance
and state controls, separate motion leaves and show/open styles, and inheritance resets. At this
first-round handoff, renderer knobs and Windows style synchronization remained pending; the
round-two section above and C25 record their later implementations and remaining contracts.

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

Input uses `glaze_ui::Event`; hits map directly to typed Rust handlers. The dock remembers the
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

`glaze_bars` publicly exposes: appbar, apps, hooks, icons, tray/tray_host/tray_data, media, audio,
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
flyouts::open(kind: flyouts::Kind, anchor_rect: glaze_ui::Rect, monitor: &glaze_desktop::Monitor)
```

The anchor is in physical **screen** pixels. `Kind` is Calendar, Quick, Wifi, Mixer, Notifications,
Launcher, Power, TrayOverflow or Language. The request is queued and `Flyouts::process` (called
by the shell's loop after each pass over the input) opens it in the same pass. Every one is native
(`crates/shell/src/flyouts/`); none opens a Windows panel any more.

| Kind | What it shows | Provider |
|---|---|---|
| Calendar | today/time, localized month/weekdays, paging/Today, up to four labeled extra clocks with day/offset, Outlook Calendar link | Windows NLS and Globalization IANA zones, including DST |
| Notifications | Retained Windows toasts grouped into expandable app stacks, real logos, age/text, swipe/card/group/all dismiss, DND/Settings and month underneath. Clock clicks open this. | One shared integration lease; qualified Windows events plus four-second reads while this panel is open. No closed-panel polling. |
| Quick | Wi-Fi and Bluetooth tiles (Wi-Fi's chevron opens the Wi-Fi list), power mode (cycles), battery saver, do not disturb, focus, night light (their settings), brightness, volume (mute; chevron opens the mixer), battery | `quick` lane, the endpoint volume |
| Wifi | the radio switch, networks with signal, security and lock; Connect / Disconnect; a network that needs a password goes to **Windows' own prompt** (`ms-availablenetworks:`), enterprise ones to Settings; location-access and no-adapter states | `wifi` lane, refreshed by its scan notification |
| Mixer | output devices (switching the default needs `audio.switchOutput`, else Sound settings), master volume, each app's volume and mute | `mixer` lane, refreshed by session events |
| Language | the installed layouts, the front app's checked; choosing switches it | `flyouts::language` |
| Power | lock, sleep, sign out, restart, shut down; the last three ask first | `system::power` |
| TrayOverflow | the unpinned tray icons; left and right clicks go to the app as from the bar | `Bars::tray`, `Bars::click_tray` |
| Launcher | Win11 pin grid or Compact list; RichEdit search/IME, Apps/Settings/Files ranking, all apps, pin menus, clipboard history, user/Settings/Power footer | `apps.json`, eight-second initial build, Start-folder watcher/ten-minute stale check; `launcher.json` usage count/recency |

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
  icons and the launcher's RichEdit DLL initialization) runs right after the window is shown (`Panel::opened`). Lanes stop 5 s after a close;
  nothing polls while closed.
- **Panels** implement `flyouts::Panel` (`width`, `view(&mut Kit)`, `activate`, `context`,
  `slide`, `wheel`, `reply`, `changed`, `key`, `tick`, `opened`, `refresh`, `field`,
  `text_changed`, `submit`) and lay out with `flyouts::kit::Kit` (header, rows, tiles, sliders,
  switches, buttons, links, notices). Each control always emits the same ops, transparent at
  rest, so a hover redraws only that control.

**A bar over a window.** Every top-bar island has its own cropped HWND, hits and local focus.
Window/location events update cached external window rectangles (DWM frame bounds exclude shadows).
An overlapped island calls `surface.set_glass(Some(Glass { body, radius, material }))` and drops
only its own wallpaper plate; uncovering calls `set_glass(None)` and restores the plate. The dock
keeps one envelope surface for magnification. Hidden bars clear their overlays. Plate ids use the
actual surface HWND. Unchanged geometry/content does not call `Surface::present` again.

**Hover preparation.** A 60 ms dwell over a module that actually opens a flyout starts capture on
a worker through a hidden, click-through client; it closes that client immediately and uses the
engine's existing three-second warm session. Pointer-down does no capture work. Overrides to
`none` or an explicit Windows panel do not warm. Moving away cancels a pending dwell. This shares
the existing engine request protocol; no new capture mode or wallpaper quality change is involved.

**Glaze's tray.** Core sends a copied typed menu model to the shell's message-only tray inbox.
The shell acknowledges within a bounded 20 ms exchange, then shows `glaze_ui::Menu<u32>` using
the same material/tokens as flyouts and desktop/dock menus. A selection is posted back with its
request nonce; core retains and executes the command/theme mapping. Missing or unresponsive
shells use `TrackPopupMenu`; core and its panic hotkey have no UI-renderer dependency. Configured
tray hosting remains alive while bars are hidden/fullscreen. Long menus scroll and keyboard
selection stays in view; menu motion stops when settled.

### Live N3 measurements (2026-11-05)

Release core, shell and engine from this worktree under the desktop lock; fixture `APPDATA` (a still
photo, liquid bars on both 1440p monitors, every module), `%TEMP%\glaze-tools\n3-flyouts-live.ps1`.
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
| Overlay GPU per update | 0.12–0.15 ms (copy, compare, glass; `GLAZE_OVERLAY_TIMINGS`) | ≤0.5 ms |
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

### Bars/flyouts parity follow-up (2026-11-05)

Implementation commits: `8ad9968` (shared menu tokens, scrolling/motion and UI helpers),
`b1128c0` (bars/dock/flyout/tray closure), `389fe66` (merge native `9c35d30`),
`fb46b82` (shared notification-provider integration and deferred launcher setup), `4ee2fcc`
(panic inside the fallback menu), `eaa4406` (acknowledged tray-request commit), and `993f0ba`
(command-delivery diagnostics). Integration registry hooks remain intact. The old notification
panel wrapper now names the grouped center, which consumes the existing shared lease rather than
starting another toast listener. Its four-second refresh action does not reconnect the provider.
Disabling/changing integration settings clears the retained panel data.

The top/dock follow-up also restores visible tooltips and keyboard focus/invoke, explicit Windows
bypass actions, network/volume/custom status details, SVG masks and bundled glyph assets, titled
button width, Close all, distinct running/attention indicators, taskbar desktop sliver and fitted,
paged previews. The launcher retains Win11 and Compact, maps Grid/Dashboard explicitly, and restores
Settings/file search, usage ranking, pins/context actions, all apps and clipboard history. Calendar
zones use real IANA conversion; focused tests cover DST, fractional offsets and date rollover.
The owned parity rows are updated; R03's remaining overlay-widget outcome belongs to H10's owner.
Broader N6 appearance/motion customization remains listed separately in C06–C15/C25.

**These commits do not establish a performance-gate pass.** Production core/shell/engine were built
in this worktree's existing `target` and run under the desktop lock with isolated config, a still
photo, six default widgets and liquid island bars/docks on both monitors. CPU is percent of one
core. The latency harness uses a zero-duration press, checks that the previous popup is hidden,
and measures release-to-visible externally. Hover trials dwell 160 ms before the click; the
warm-up itself is armed after 60 ms. No pointer-down capture runs.

| Check | Measured | Gate / result |
|---|---|---|
| Quick content, no hover, four cold trials | 3.05–5.87 ms; median 3.74 ms | ≤30 ms: pass |
| Quick content after hover, four trials | 4.36–19.28 ms; median 5.40 ms | ≤30 ms: pass; no former ~30 ms click stall |
| Launcher first content after hover | 19.16 ms | ≤30 ms: pass |
| Glass after hover, Quick / launcher | 43.7–57.4 / 49.0 ms after opening | ≤300 ms: pass in these trials |
| Glass without hover, four cold trials | 178.5–208.2 ms | ≤300 ms in these four; earlier test-binary cold probe was 347.8 ms |
| Hover capture setup on its worker | 28.7–37.5 ms | off the click thread |
| Own tray glass menu / Pause reply | 28.45 ms initially; 32.33 / 39.22 ms in final cold runs; Pause delivery verified | ≤30 ms content: **missed in final runs** |
| One covered island / uncovered again | one engine overlay / zero | independent island switch: pass |
| Settled bars / flyouts | zero redraws in a three-second bar interval; all nine panel closes report zero after motion | zero: pass |
| Shell idle, three 60 s samples | 0.13, 0.10, 0.21%; median 0.13% | ≤0.1%: **fail** |
| Shell over actual maximized fixtures | 0.23% in one 60 s sample; all six widgets confirmed hidden | ≤0.05%: **fail**, not a three-sample qualification |
| Shell private memory, visible idle / maximized | 47 / 39 MiB | ≤40 MiB: visible fixture **fails** |
| Shell before any popup / after the interaction sequence | 44.07 / 45.10 MiB | closed delta +1.03 MiB; per-open peak was not sampled |
| Shell-unresponsive tray fallback | stock menu visible with shell suspended | pass |
| Panic with suspended shell and fallback menu open | supervisor handles panic; desktop restored within 4.79–5.10 s | functional pass; includes existing child grace, not instant restoration |
| First paint / all bars (process READY markers) | 282.6 / 515.4 ms | earlier first-paint budget remains missed; no recovery requalification |
| Goldens/click/unit/integration tests | 200 passed; seven deliberately ignored | pass |

The tray renderer itself logged 2.7–3.0 ms to show; core constructs the menu (including its bounded
Explorer desktop-state read) before asking the shell. That cold-open path remains above budget.
The first 300 ms config observation also missed a delivered Pause; the later check waits for the
write and the trace confirms the same command id/nonce in shell and core.

The earlier `production-final` rectangular-cover intervals (0.31/0.18/0.34% shell CPU) were not
maximized and did not hide widgets, so they are retained as diagnostics and excluded from the
maximized gate. The corrected fixture asserts zero visible widgets before sampling. No steady
sampling/visual quality was reduced to obtain the figures above. The bar log still shows only
minute-clock updates once settled; the remaining CPU cost needs profiling in the integrated shell.
Animated-background overlay GPU/CPU, crash recovery, per-open peak memory, actual CJK composition
live focus handback and screen-sharing were not requalified here; the earlier N3 evidence above is historical.

The first tray harness mistakenly selected a hidden pooled submenu; it now searches visible
windows and asserts closure before CPU sampling. A real island input test also exposed an invalid
UI Automation focus id after cropping: crops retain only their own focus, and Surface validates
focus before publication. A later panic test exposed Windows' stock modal menu swallowing the
thread hotkey. Core now installs a scoped `WH_MSGFILTER` during that fallback, calls `EndMenu`, and
reposts the hotkey to its ordinary supervisor pump; the live suspended-shell test passed afterward.

Reproduce with `%TEMP%\glaze-tools\native-bars-closure.ps1`: `-Production -Measure -Suffix
production-final` for the interaction/idle sequence; `-Production -MaxOnly -Measure -Panic -Suffix
maximized-qualified` for the corrected maximized diagnostic; `-Production -MaxOnly -TrayCheck -Panic -Suffix
tray-final` for the short tray/panic check (`tray-trace` retains the clean command-delivery trace). Pass the built shell/core paths using `-ShellTest` and
`-CoreTest`. Logs, screenshots and results JSON are in the matching `native-bars-closure-*`
directories; `measure-bars-closure-*.json` contains the raw 60 s samples. Each script restores config
environment/minimized windows, stops its fixtures and releases the lock in `finally`. After cache
cleanup removed the old executable, the user requested restoration from this worktree's native
`target/release/glaze-core.exe`; no old main-checkout binary is used.

Targeted `cargo check` and all-target clippy pass; clippy retains four inherited
`items_after_test_module` warnings in bars. Tests run per crate: shell 44/2 ignored, UI 23/0,
core 16/4, bars 107/1, integrations 10/0. Goldens for restored bar glyphs and the richer notification
center were reviewed and updated; the initial stale notification golden failure passed after its
WinRT fixture timestamps were corrected. Live release-test fixtures were used only during diagnosis;
the reported final timing, CPU and panic results use production executables.

Small changes outside the listed source ownership: `crates/shell/Cargo.toml` enables the DWM frame
bounds API; `crates/shell/src/overlay_client.rs` adds the hover worker; the notification wrapper and
the goldens/fixture timestamps in `crates/shell/src/integrations/` select the grouped presentation;
`crates/integrations/src/{lib,notifications}.rs` add the visible-center refresh action and attempt
every dismissal. No widget, Studio or engine source was changed by these commits.

Exact additive interfaces:

```rust
// glaze_bars::tray_menu; WM_COPYDATA to HWND_MESSAGE class "glaze.shell.tray-menu".
// dwData = MAGIC (0x4D50544D), wParam = core callback HWND; reply 1 means copied/accepted.
pub enum Item {
    Separator,
    Action { label: String, id: u32, checked: bool, enabled: bool },
    Submenu { label: String, items: Vec<Item> },
}
pub struct Request { pub nonce: usize, pub point: (i32, i32), pub items: Vec<Item> }
// CHOSEN = WM_APP + 18: wParam = command id (0 = closed), lParam = request nonce.
// SHOW = WM_APP + 19 to the shell inbox: wParam = core HWND, lParam = acknowledged nonce.
// A copied request cannot open until SHOW. Late requests after a timeout stay uncommitted.
// Core rejects stale nonces. The request acknowledgement is bounded to 20 ms.

// glaze_integrations::Action adds RefreshNotifications, valid only for Notifications.
// shell::overlay_client
pub fn warm(client: HWND, shape: glaze_ui::glass::Shape);
// glaze_ui
pub fn set_style(tokens: Option<Tokens>); // glaze_ui::menu
// DisplayList::translated(dx, dy); MonthGrid::view_with_weekdays(..., &[&str; 7]);
// Surface::animation_frames(Duration) -> AnimationFrames (cancel on drop); Event::Frame.
```

#### Native Studio merge and field audit

`93400fb` merges native `6a5037e`. `shell::preview` and `lib.rs` stay unchanged from that baseline;
Studio continues to call the actual bar/widget/menu views. The merge keeps both Surface's focus
validation and its new application-window presentation path. Display-list translation now handles
`Clip`/`Unclip`, and the obsolete shell test-binary desktop fixture is removed because the startup
loop belongs to the binary, while its views now belong to the library.

| Studio-written field | Native consumer / remaining row |
|---|---|
| Bar look style, scheme, family, radius and direct font size | token resolver and shared preview/native view |
| Module weight, text/fill/edge colors, padding/radius/size | module view; a width-only edge now follows the bar rim, and zero width hides it |
| Custom command arguments | typed argv in the custom provider, without shell re-parsing |
| Taskbar button length, including titled buttons | explicit length wins over the 168 DIP default |
| Root animation enabled/duration/curve and dock magnify timing | bar motion resolver |
| Bar-wide border/shadow and item rest/hover/active styling | consumed; C10 remains for saturation and continuous corners |
| Appearing style, hover/press, bounce/attention durations and popup opening timing | consumed; C14 remains for the engine glass fade/slide path, C15 closed |
| Appearance font/content scale/shadow and dedicated OneUI/Fluent rendering | consumed; C06 remains for physical blur range, C25 for Windows-wide token sync |

The focused shared-preview test checks module weight/color/size, the width-only edge and a titled
212 DIP taskbar button through `preview::bars`, rather than a duplicate Studio renderer. C06, C10
and C13–C15/C25 now name the specific unsupported consumers. Native Studio's own rows and notes
from N7 are preserved, and parity counts are recomputed.

Post-merge checks: shell **45 passed / 1 ignored**, UI **25/0**, core **16/4**, bars **107/1**,
integrations **10/0**, Studio **24/0**: **227 passed, six deliberately ignored**. Strict all-target
clippy passes for shell/core. Studio's 14 affected light/dark golden images and their documentation
copies were refreshed for the restored shared bar glyphs and reviewed; no Studio Rust source or
preview substitute was introduced. These image/doc updates are the additional out-of-ownership
edits needed to keep `cargo test -p glaze-studio` passing.

The main timing/CPU/memory table above predates the Studio merge. A short current-build probe
(`studio-merged`) measured content at **4.59 ms**, with no-hover cold glass at **484.2 ms**. The
separate `studio-idle-1` run measured hover-prepared content/glass at **8.21 / 58.2 ms**, then a
60-second visible-widget idle sample at **0.29% shell CPU / 45 MiB private memory**: both idle
gates still fail. Its closed post-popup reading was 46.17 MiB. The first probe's 33.1 MiB startup
reading was not representative of settled memory, and is not used to claim a pass. These are
single diagnostics, not three-sample current-build qualifications; maximized CPU was not rerun
after this merge. The unresolved gates remain explicit. Release binaries for core, wallpaper, shell and
the newly required native Studio are rebuilt in this worktree's existing target. Windows refused
to replace the three running executables during publication; their linked artifacts are copied
into `target/release` only after the old processes stop under the desktop lock. The normal core is
then restarted there, using the user's saved configuration.

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
`glaze_glass_ipc::Writer`. It converts to physical virtual-desktop pixels and replaces the whole
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
`crates/shell/tests/golden/bars` (top/dock in all five kit styles dark, and liquid light), alpha/click coverage for every body
and hit, island click-through, minute-clock damage, dock edges/taskbar modules, magnification and
preview clicks. The UI's separate process verifies no vendor GPU driver loads.

### Live N2 measurements (2026-11-05)

Release `glaze-shell` and `glaze-core`, built in this worktree, under the desktop lock.
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
| Glaze WebView2 processes | 0 | 0 | passes |
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

Raw artifacts are under `%TEMP%\glaze-tools`: `n2-bars/{idle,maximized}-{1,2,3}.txt` and
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
(Since 6 November the hooks run on a `win-events` thread of their own; a data change marks only
the bars showing that topic. See performance.md, "Shell idle hill-climb".)
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

Raw artifacts live under `%TEMP%\glaze-tools`:

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

// glaze_ui::Surface:
pub fn new_deferred_accessibility(kind: Kind, name: &str, role: Role) -> anyhow::Result<Self>;
// glaze_bars::taskbar::JournalLease:
pub fn acquire() -> anyhow::Result<Self>;
// glaze_bars::taskbar::Taskbar<S>:
pub fn restore_visibility(&mut self) -> anyhow::Result<bool>;
// Lease is thread-owned and not Send. Reveal preserves the remaining undo entries.
// style::GlassStyle adds clarity: Option<f32>, validated in 0..=1.
```

No new messages or flyout service APIs. **Not merge-ready:** visible-widget idle CPU, first
paint, recovery/instant restoration and the engine delivery/GPU gates remain open.
