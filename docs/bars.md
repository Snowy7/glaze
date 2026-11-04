# Top bar, dock and taskbar: the contract

This is the spec for mypaper's shell bars: a macOS-style **top bar**, a **dock** that can replace
the Windows taskbar, and options for the **native taskbar**. It's the source of truth for
`crates/bars` (the `mypaper-bars` binary), its page `bars/shell`, core's supervision and tray
entries, and the Studio settings. Change it deliberately, in the same commit as the code.

## Principles

- **Our own windows, supported APIs.** Bars are our own windows (one transparent WebView2 per bar per
  monitor). The native taskbar is only shown, hidden or set to auto-hide (`ShowWindow`,
  `SHAppBarMessage`) and aligned (its documented registry value). No code goes into Explorer.
- **Always reversible.** Anything done to the native taskbar is journaled
  (`%LOCALAPPDATA%\mypaper\taskbar.json`) and undone when the bars exit, on the panic hotkey, and at
  the next start after a crash.
- **Reserved space, like a real taskbar.** Docked bars register as app bars (`ABM_NEW` / `ABM_SETPOS`),
  so maximized windows and the widget grid stop at them. An auto-hiding dock reserves nothing and
  slides in at the screen edge.
- **Light.**
  - Window lists come from WinEvent hooks; nothing polls.
  - The clock ticks once a minute, aligned to the minute (or once a second if seconds are shown).
  - Stats, media, network, volume and the tray run only while a module that shows them is on
    screen.
  - While a fullscreen app is in front, that monitor's bars hide, and the bars elsewhere stop
    their periodic work (stats); the clock and event-driven modules keep going.
  - Measured (debug build, two 1440p monitors, two top bars and a dock): 0.03% of the machine's
    CPU at idle; about 520 MB working set for the host and WebView2's processes (one browser, one
    GPU process, one renderer per bar).

## Process

```text
mypaper-core ── supervises (optional, like the widget host) ──► mypaper-bars (crates/bars)
   ├─ per monitor: top bar window and/or dock window, each a WebView2 on https://bars.mypaper/
   ├─ system model (Rust): windows, apps, icons, media (SMTC), volume, network, battery, stats, clock
   ├─ native taskbar control + undo journal (also a library: core replays it)
   └─ bridge: page ⇄ host JSON messages (below)
```

- **Single instance.** It uses the named mutex `Local\mypaper-bars`. The message-only control window
  has class `mypaper.bars.host`. **Posting `WM_CLOSE` to it** makes the bars restore the native
  taskbar, remove their app bars and exit; that's how core stops them.
- **Broadcasts.** A hidden top-level window (class `mypaper.bars.broadcast`) hears what
  message-only windows can't: `TaskbarCreated`, display, power and time changes.
- **Config.** It watches `%APPDATA%\mypaper\bars.json` and `config.json` (hold reasons, same rules
  as the wallpaper), the active theme's folder for its `bars` skin, `palette.json`, and
  `bars/shell/dist` (a rebuilt page reloads every bar).
- **Explorer restart.** On `TaskbarCreated` (after a second's settle) it re-applies the
  native-taskbar settings and registers its app bars again.
- **Displays.** On `WM_DISPLAYCHANGE` / `WM_DPICHANGED` it re-reads the monitors, adds, moves and
  removes bars, and re-applies the taskbar mode (a monitor that comes back has a fresh secondary
  taskbar).
- **The page** is `bars/shell/dist`, found beside the exe or any parent, else under the current
  folder or any parent. Without it no bars are created and **the taskbar is kept**, whatever
  `taskbar.mode` says, so nothing is left without a taskbar.
- **Files.** WebView2 profile: `%LOCALAPPDATA%\mypaper\webview2-bars\`. Icon cache:
  `%LOCALAPPDATA%\mypaper\icons\`. `MYPAPER_BARS_DEVTOOLS=1` enables devtools and context menus.

## Config: `bars.json`

```json
{
  "version": 1,
  "taskbar": { "mode": "keep", "alignment": "center" },
  "top": {
    "enabled": true,
    "monitors": "all",
    "height": 32,
    "left":   ["start", "workspaces", "app"],
    "center": ["clock"],
    "right":  ["media", "tray", "network", "volume", "battery", "quick", "power"],
    "clock":  { "seconds": false, "format": "ddd d MMM  HH:mm" }
  },
  "dock": {
    "enabled": true,
    "monitors": "primary",
    "style": "floating",
    "position": "bottom",
    "size": 48,
    "autohide": false,
    "magnify": true,
    "running": true,
    "pinned": [
      { "path": "C:\\Windows\\explorer.exe", "name": "File Explorer" },
      { "path": "shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", "name": "Terminal" }
    ]
  },
  "appearance": { }
}
```

- **Every field is optional**; a missing one takes the default shown above, except
  `dock.enabled` (default `false`) and `dock.pinned` (below). `{}` is the same as no file.
- **`taskbar.mode`**: `keep` (untouched), `autohide`, or `hide` (hidden completely, on every
  monitor; its space goes to our bars).
  - Changing it takes effect live. Leaving `hide` or `autohide` restores the exact previous state from
    the journal.
  - `hide` also makes the taskbar auto-hide first, so the work area is freed even if Explorer re-shows
    it.
  - **In `hide` the taskbar never stays visible.** Explorer shows it again on some events (Start
    and the Windows key, notifications, display and resolution changes, `TaskbarCreated`). The bars
    watch the taskbar windows with out-of-context WinEvent hooks (`EVENT_OBJECT_SHOW`, and
    `EVENT_OBJECT_LOCATIONCHANGE` for an auto-hidden taskbar sliding in) and hide them again as the
    event arrives. They never give up; re-hides are only spaced 50 ms apart, so a fight with Explorer
    stays cheap. Each re-hide also checks that auto-hide is still set, and sets it again if not.
- **`taskbar.alignment`**: `center` or `left` (the `TaskbarAl` value Settings writes, announced with
  `WM_SETTINGCHANGE` `"TraySettings"`). Omit it to leave Windows' setting alone; removing it
  restores the original value.
- **`top` / `dock` `monitors`**: `all`, `primary`, or a list of device names (`\\.\DISPLAY2`).
- **`dock.style`**: the dock's layout. `floating` (the only one so far) is a macOS-like pill of
  icons centered on its edge. See "Extending" for adding one.
- **`dock.position`**: `bottom`, `left` or `right`.
- **Sizes** are in DIPs. `top.height`: 20–80. `dock.size` is the icon size, 24–128; a `floating`
  dock's band (what it reserves) is `size + 16`: a 6 DIP gap from the screen edge, then a pill with
  5 DIP of padding.
- **`top.clock.format`** tokens: `dddd` `ddd` `d` `dd` `MMMM` `MMM` `MM` `M` `yyyy` `yy` `HH` `H`
  `hh` `h` `mm` `ss` `tt`; `'quoted text'` and anything else is literal. Default
  `ddd d MMM  HH:mm`, or `ddd d MMM  HH:mm:ss` with `seconds`. At most 64 characters.
- **`dock.pinned[].path`** is an exe, a shortcut (`.lnk`), a folder, or an `shell:AppsFolder\<AUMID>`
  path (Store apps). **Missing `pinned`** means the native taskbar's pinned shortcuts
  (`%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\*.lnk`, in name order;
  Store apps pinned there aren't shortcuts and don't come along), or File Explorer when there are
  none. An empty list is a choice and stays empty.
- **Modules** are listed at most once across `left`, `center` and `right`.
- **Unknown fields are rejected.** A file that fails to parse is logged and ignored, and the last
  good config keeps running.
- **Writers change only what they set.** The tray, Studio and the dock's pin edits read the file as
  JSON, set their field(s), check that the result is still valid, and write it atomically. They
  refuse to touch a file that doesn't parse, so a typo is never overwritten, and a file never gains
  the defaults it left out.
- **No file means built-in defaults**: top bar on, dock off, taskbar `keep`. The bars stay out of the
  way until enabled, and the tray offers to enable them.

### Appearance

Bars use the same `Appearance` fields and layering as widgets (see docs/widgets.md, "Appearance"):

1. Built-in defaults: `glass`, tint `#0e0e0e`, opacity 0.6, blur 30, radius 0 for the top bar and 16
   for the dock, text `light`, `border` on, accent `#ffffff`, font Segoe UI Variable, scale 1,
   `shadow` off.
2. The theme's `"bars": { "appearance": …, "css": "bars.css" }`.
3. `bars.json` → `appearance`.

The host resolves the layers (`accent: auto` takes the palette's accent, else white) and sends every
field, per bar. The page applies them as the same CSS variables and `data-*` attributes, plus
`data-bar="top|dock"` and `data-position`, and inserts the theme CSS last.

A WebView can't see what's behind its window, so `blur` only blurs the page's own content under
popovers; behind a bar there's the wallpaper (bars reserve their band) or, for an auto-hiding dock,
whatever window is there, seen through the tint.

## Modules

These are the top-bar items, in `left` / `center` / `right` order. Each is a small component in
`bars/shell`. Clicks run host commands; the host does the Windows work.

| Module | Shows | Click |
|---|---|---|
| `start` | a mypaper mark | opens the Start menu (`Shell_TrayWnd` `WM_SYSCOMMAND` `SC_TASKLIST`; the Windows key without a taskbar) |
| `workspaces` | virtual desktops: count and current (`VirtualDesktopIDs` / `CurrentVirtualDesktop` under `HKCU\…\Explorer\VirtualDesktops`, watched with `RegNotifyChangeKeyValue`; absent values mean one desktop) | switch desktop: Win+Ctrl+arrow, sent only on the user's click (once per step) |
| `app` | the foreground app's name and icon (the last app window in front: clicking a bar doesn't change it) | — |
| `clock` | time and date (format in `bars.json` → `top.clock`) | opens the calendar / notification center (`ms-actioncenter:`) |
| `media` | now playing (SMTC): title and artist | play/pause; scroll for next and previous |
| `tray` | the notification-area icons (see below) | left/right click forwarded |
| `network` | wifi signal and name (WLAN API) / ethernet (the interface table: an up, physical, wired interface) / offline; changes from `NotifyIpInterfaceChange` and WLAN notifications | opens quick settings |
| `volume` | level and mute (Core Audio `IAudioEndpointVolume` on the default output, with its change callback) | toggle mute; scroll to change (raising it unmutes) |
| `battery` | percent and charging (`GetSystemPowerStatus`, refreshed on `PBT_APMPOWERSTATUSCHANGE`); hidden on desktops | power settings |
| `quick` | a settings glyph | opens quick settings (`ms-actioncenter:controlcenter/`) |
| `power` | a power glyph | menu: lock, sleep, sign out, restart, shut down (restart and shut down ask first) |
| `stats` | CPU / RAM mini gauges (sampled every 2 s) | Task Manager |

**The tray is best effort.** Notification-area icons belong to Explorer. The host reads them through
UI Automation (a supported accessibility API) from the (possibly hidden) taskbar, on a worker
thread, and refreshes on UIA structure-changed events. What Windows 11 (build 26300, "26H2")
exposes:

- Each *promoted* icon is a `SystemTray.NormalButton` with AutomationId `NotifyItemIcon`; its Name is
  the tooltip. It supports `Invoke`.
- The overflow chevron is the `SystemTray.NormalButton` with AutomationId `SystemTrayIcon` left of
  them. Hidden icons only exist in the tree while the overflow flyout is open, so they're reached
  through it: it's always listed last, with id `overflow`.
- UIA gives no icons. The host uses the PNG snapshot the shell keeps under
  `HKCU\Control Panel\NotifyIconSettings\<id>\IconSnapshot`, from the entry whose `InitialTooltip`
  starts the button's name, else whose exe's `FileDescription` or file name appears in it. Items
  without a match show a monogram.
- Left click is UIA `Invoke`. Right click focuses the button (UIA `SetFocus`) and sends the
  context-menu key (Shift+F10), which the tray turns into the app's context menu.
- **With `taskbar.mode: hide` there are no tray icons**: Explorer removes a hidden taskbar's whole
  XAML content from the UIA tree (measured: no `SystemTray.*` element at all), so there's nothing
  to read or invoke. The icons are there in `keep` and `autohide` (an auto-hidden taskbar is only
  slid off-screen), so use `autohide` to keep the tray module working.
- If UI Automation is unavailable or finds no tray (as in `hide`), the module shows a single
  `overflow` button that tries the native overflow flyout (Win+B, Enter). The fallback is logged.

## Dock

- **Items.** Pinned items come first, in order, then running apps that aren't pinned (if `running`),
  in the order they started (focusing a window never reshuffles the dock).
  - **Running apps** are top-level windows a taskbar would show: visible, not cloaked, and either
    `WS_EX_APPWINDOW` or unowned, not a tool window and not `WS_EX_NOACTIVATE`. The desktop, the
    taskbars and UWP's inner `Windows.UI.Core.CoreWindow`s never count.
  - **Grouping** is by AppUserModelID (`SHGetPropertyStoreForWindow`, `PKEY_AppUserModel_ID`),
    falling back to the process image. A pin claims the running apps whose AUMID or exe matches it
    (a shortcut's target and AUMID are read from the `.lnk`).
  - **Names**: a pin's `name`, else the shell's display name (a shortcut's or AppsFolder entry's
    name), else the exe's `FileDescription`. A running app with an AUMID takes its Start menu
    entry's name and icon (`shell:AppsFolder\<AUMID>`), which is also what launches it again.
  - **Indicators**: running apps show a dot, the foreground app a brighter one.
- **Icons.** 64 px PNGs from `IShellItemImageFactory` (exe, `.lnk`, AUMID), made on a worker
  thread, cached by key in memory and in `%LOCALAPPDATA%\mypaper\icons\`. That folder is mapped as a
  second virtual host, so events carry URLs (`https://icons.bars.mypaper/<hash>.png`) and the
  WebView caches the images. (Data URLs would resend every icon with every window event.)
- **Clicks.**
  - **Left**: activate the app's most recent window, or minimize it if it's already in front (and
    bring the next window forward, like the taskbar). If it isn't running, launch it.
  - **Middle**: launch a new instance.
  - **Right**: menu with its windows (activate), Pin/Unpin, New window, Close window(s).
  - **Activation** uses the foreground rights the user's click grants the dock's window (the dock
    takes focus on click; it's not `NOACTIVATE`). "In front" means the last app window that had the
    foreground before the click. A minimized window is restored with `WM_SYSCOMMAND` `SC_RESTORE`
    (Chromium-based windows ignore a cross-process `ShowWindow(SW_RESTORE)`).
  - **The bars never keep the foreground unless clicked.** When a bar is activated without a click
    (WebView2 taking focus as it starts, or Windows picking a topmost bar after the active app
    minimized or closed), the host hands the foreground back to the last app window, or the top
    one, or the desktop.
- **Drag** reorders pinned items, and dragging a running app into the pinned area pins it. Dragging
  a pinned item well off the dock unpins it. All write `bars.json`.
- **Magnify** is a macOS-like hover zoom (CSS transform). It's disabled when reduced motion is set.
- **Auto-hide.** The dock reserves no space and sits at the work area's edge (the screen edge
  unless the native taskbar is kept there). A 1 px transparent, non-activating window on that edge
  receives mouse moves; touching it sends the page `reveal`, and the page slides the dock in. It
  slides out after 1 s without the pointer (not while a menu or a drag is open) and then sets an
  empty region.

### Windows and regions

A bar's window is its band plus 360 DIPs toward the middle of the screen (clipped to the monitor),
for popovers, menus and magnified icons. The page tells the host which parts are visible and take
the pointer (`region`); the host sets that as the window region (`SetWindowRgn`), so the rest
clicks through to the windows behind. The top bar's band is its strip at the top of the window;
the dock's pill is centered along its edge, 6 DIPs from it.

## Bridge (page ⇄ host)

All messages go through WebView2 `postMessage` as JSON. The page's side is
`bars/shell/src/protocol.ts`. The host accepts messages from `https://bars.mypaper/` only, and sends
events to a bar only after it said `hello`.

**Page → host requests**: `{ id, method, params }` → `{ id, result }` or `{ id, error: { code, message } }`
(`code`: `InvalidParams`, `NotFound`, `Unavailable`, `Internal`).

| Method | Params | Result |
|---|---|---|
| `hello` | — | `{ bar: "top"\|"dock", monitor: { device, primary, width, height }, config, appearance, themeCss, dpi }` (sizes in DIPs) |
| `activate` | `{ app }` (an item's `key`) or `{ hwnd }` | — |
| `launch` | `{ path, newInstance? }` | — |
| `minimize` / `close` | `{ hwnd }` | — |
| `pin` / `unpin` / `reorder` | `{ path, name?, index? }` / `{ path }` / `{ paths }` | — |
| `start` | — | — |
| `actionCenter` | `{ panel: "notifications"\|"quick" }` | — |
| `volume` | `{ set?: 0..1, mute?: bool, delta?: -1..1 }` | `{ level, muted }` |
| `media` | `{ action: "toggle"\|"next"\|"previous" }` | — |
| `tray` | `{ id, button: "left"\|"right" }` | — |
| `power` | `{ action: "lock"\|"sleep"\|"signout"\|"restart"\|"shutdown" }` | — |
| `workspace` | `{ direction: "left"\|"right" }` | — |
| `openSettings` | `{ uri }` (an `ms-settings:` URI only) | — |
| `region` | `{ rects: [x, y, width, height][] }` (DIPs from the window's top-left, at most 64) | — |

`pin` of an item that's already pinned moves it to `index`. `reorder`'s paths are matched
case-insensitively; pins it doesn't list keep their order after the listed ones.

**Host → page events**: `{ event, data }`.

| Event | Data |
|---|---|
| `apps` | the dock items: `{ key, name, path, icon?, pinned, windows: [{ hwnd, title, active, minimized }] }[]` |
| `foreground` | `{ name, icon? }`, or null |
| `clock` | `{ now }` (ms since the epoch); the page formats it |
| `media` | `{ title, artist, app, playing }`, or null |
| `volume` | `{ level, muted }`, or null without an output device |
| `network` | `{ kind: "wifi"\|"ethernet"\|"none", name?, signal? }` |
| `battery` | `{ percent, charging }`, or null |
| `tray` | `{ id, name, icon? }[]` |
| `workspaces` | `{ count, current }` (`current` is 0-based) |
| `stats` | `{ cpu, memory: { used, total } }` |
| `appearance` | `{ appearance, themeCss }` |
| `config` | the parsed `bars.json`, defaults filled in |
| `visibility` | bool |
| `reveal` | null: an auto-hiding dock's edge was touched |

Events for a module are only produced while some bar shows that module, and a value is only resent
to a bar when it changed. After `hello`, and when a hidden bar comes back, the host sends the
current value of everything the bar shows.

## Hold

- **Fullscreen app** (with `pause_on_fullscreen`): the bars on its monitor hide (window hidden,
  WebView suspended, `visibility: false` first); the other bars stay up but stop periodic work.
  Detected on foreground and move events (and `ABN_FULLSCREENAPP`), never on a timer.
- **Display off or session locked**: every bar is suspended and every source stops.
- **Paused by the user, or on battery**: no effect. The bars are the user's taskbar.

## Core and tray

- **Core** supervises `mypaper-bars.exe` beside it, as an optional child: restarted with backoff, and
  if it's missing or crash-looping mypaper keeps running without bars.
- **At startup**, before starting the bars, core stops a bars process left by an earlier run and
  replays a leftover taskbar journal (a crash's), so the native taskbar is always back first.
- **Tray** gains a **Bars** submenu:
  - Top bar ✓ and Dock ✓ toggle `top.enabled` / `dock.enabled`.
  - Taskbar ▸ Keep / Auto-hide / Hide sets `taskbar.mode`.
- **Panic hotkey and Quit**: core stops the bars child first (`WM_CLOSE` to `mypaper.bars.host`,
  then up to 3 s), which restores the native taskbar from the journal. If the child had to be
  killed, core replays the journal itself (the journal format and replay live in `crates/bars` as a
  library used by both). Core also replays it when the bars stop for good (crash loop, missing
  page).

### The journal

`%LOCALAPPDATA%\mypaper\taskbar.json` holds each thing's original, written before its first change
and kept through later changes, so `keep` always returns to how the user had it:

```json
{ "appbar": { "was": 0 }, "hidden": true, "alignment": { "was": 1 } }
```

`appbar` is the `ABM_GETSTATE` before the bars made the taskbar auto-hide, `hidden` says they hid the
taskbar windows, and `alignment` is `TaskbarAl` before they changed it (`null`: absent). Restoring
puts each back and deletes the file. A corrupt journal is set aside as `taskbar.json.bad`.

## Extending

The page and the host are built so new bars, flyouts and looks are additions, not rewrites.

- **Modules.** `bars/shell/src/top/modules.ts` maps every `Module` to a factory
  (`Record<Module, Factory>`), so a module added to `protocol.ts` (and `config::Module` in Rust)
  fails the build until it has one. Its data is a `Topic` in `crates/bars/src/bridge.rs`
  (`module_topic`): events flow only while a bar shows it.
- **Dock styles.** `dock.style` picks a layout: `bars/shell/src/dock/styles.ts` maps each
  `DockStyle` to a page factory, and `DockStyle::band` (`crates/bars/src/config.rs`) gives its band.
  A full-width, Windows-style taskbar is a new style (a factory drawing Start, search, task buttons,
  tray and clock in one strip, plus its band); the window, app-bar, auto-hide, region, item and
  activation machinery is shared. The page sets `data-style` on `<html>`.
- **Flyouts.** `bars/shell/src/flyout.ts` is the one mechanism for anything that opens over the
  screen. `defineFlyout(id, render)` registers one; modules open it with `toggle(props, { anchor,
  place })`. One is open at a time; it closes on Escape, an outside press and window blur; it's part
  of the window region while open; its `scope` ends its subscriptions on close. Menus (`menu.ts`:
  the power menu, the dock's context menu) are the `menu` flyout. A launcher, quick settings, a
  network list, a volume mixer, a language switcher, a calendar and the tray overflow are further
  flyouts. A flyout must fit in its bar's window: the band plus `ROOM` (360 DIPs,
  `crates/bars/src/bar.rs`; raise it, or make it per bar, for bigger ones).
- **Styling.** Everything is drawn from CSS variables and attributes, and the theme's CSS is
  inserted last (`<style id="theme-css">`), so presets restyle without code:
  - on `<html>`: `data-bar="top|dock"`, `data-position`, `data-style`, `data-surface`,
    `data-text`, `data-shadow`, and the class `paused` while hidden;
  - variables: `--surface`, `--raised`, `--blur`, `--radius`, `--line`, `--edge`, `--t0`…`--t3`,
    `--fill-1`…`--fill-3`, `--accent`, `--font`, `--scale`, `--ease`, `--drop`, `--shade`; the top
    bar's `--bar-h`, the dock's `--size`;
  - top bar: `.top`, `.zone[data-zone="left|center|right"]`, `.mod[data-module="<name>"]` (one per
    module; e.g. `.ws-dot.is-current`, `.tray-item`, `.gauge`, `.clock`);
  - dock: `.dock`, `.pill`, `.items`, `.slot[data-state="none|running|active"][data-pinned]
    [data-name="<app>"]`, `.icon`, `.dot`, `.divider`, `.dock-label`;
  - flyouts: `.layer > .flyout[data-flyout="<id>"]` (`--origin` is the side toward the anchor),
    `.menu`, `.menu-item`, `.menu-sep`, `.menu-header`, `.hud`.

  Glyphs are inline SVGs from one map (`bars/shell/src/icons.ts`): the place for an icon mapping.

## Interaction with widgets

App-bar registration changes the work area. The widget host must re-lay out on `WM_SETTINGCHANGE`
with `SPI_SETWORKAREA` (and on display changes), so widgets never sit under a bar.

## How to run

```sh
pnpm -C bars/shell install
pnpm -C bars/shell build           # → bars/shell/dist
pnpm -C bars/shell test            # vitest: layout, dock ordering and drag, clock format, magnify
cargo build -p mypaper-bars
target\debug\mypaper-bars.exe      # or let mypaper-core start it
```

- Without a WebView2 host, `pnpm -C bars/shell dev` (or the built page in a browser) runs against a
  mock host: `?bar=top|dock&pos=bottom|left|right&autohide=1`.
- To try it without touching your real setup, point `APPDATA` and `LOCALAPPDATA` at a temporary
  folder holding `mypaper\bars.json`.
