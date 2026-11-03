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
  - Stats, media and network update only while a module that shows them is on screen.
  - While a fullscreen app is in front, that monitor's bars hide, and the bars elsewhere stop
    their timers except the clock.

## Process

```text
mypaper-core ── supervises (optional, like the widget host) ──► mypaper-bars (crates/bars)
   ├─ per monitor: top bar window and/or dock window, each a WebView2 on https://bars.mypaper/
   ├─ system model (Rust): windows, apps, icons, media (SMTC), volume, network, battery, stats, clock
   ├─ native taskbar control + undo journal
   └─ bridge: page ⇄ host JSON messages (below)
```

- **Single instance.** It uses the named mutex `Local\mypaper-bars`. The message-only control window
  has class `mypaper.bars.host`.
- **Config.** It watches `%APPDATA%\mypaper\bars.json` and `config.json` (hold reasons, same rules
  as the wallpaper), and the active theme for its `bars` skin.
- **Explorer restart.** On `TaskbarCreated` it re-applies the native-taskbar settings and app-bar
  registrations.

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
    "right":  ["media", "tray", "network", "volume", "battery", "quick", "power"]
  },
  "dock": {
    "enabled": true,
    "monitors": "primary",
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

- **`taskbar.mode`**: `keep` (untouched), `autohide`, or `hide` (hidden completely, on every
  monitor; its space goes to our bars).
  - Changing it takes effect live. Leaving `hide` or `autohide` restores the exact previous state from
    the journal.
  - `hide` also makes the taskbar auto-hide first, so the work area is freed even if Explorer re-shows
    it.
- **`taskbar.alignment`**: `center` or `left` (the `TaskbarAl` value Settings writes). Omit it to
  leave Windows' setting alone.
- **`top` / `dock` `monitors`**: `all`, `primary`, or a list of device names (`\\.\DISPLAY2`).
- **`dock.position`**: `bottom`, `left` or `right`.
- **Sizes** are in DIPs.
- **`dock.pinned[].path`** is an exe, a shortcut (`.lnk`), a folder, or an `shell:AppsFolder\<AUMID>`
  path (Store apps).
- **Unknown fields are rejected.** A file that fails to parse is logged and ignored, and the last
  good config keeps running.
- **No file means built-in defaults**: top bar on, dock off, taskbar `keep`. The bars stay out of the
  way until enabled, and the tray offers to enable them.

### Appearance

Bars use the same `Appearance` fields and layering as widgets (see docs/widgets.md, "Appearance"):

1. Built-in defaults: `glass`, tint `#0e0e0e`, opacity 0.6, blur 30, radius 0 for the top bar and 16
   for the dock, text `light`.
2. The theme's `"bars": { "appearance": …, "css": "bars.css" }`.
3. `bars.json` → `appearance`.

The page applies them as the same CSS variables and `data-*` attributes, plus `data-bar="top|dock"`
and `data-position`.

## Modules

These are the top-bar items, in `left` / `center` / `right` order. Each is a small component in
`bars/shell`. Clicks run host commands; the host does the Windows work.

| Module | Shows | Click |
|---|---|---|
| `start` | a mypaper mark | opens the Start menu (`Shell_TrayWnd` `WM_SYSCOMMAND` `SC_TASKLIST`) |
| `workspaces` | virtual desktops: count and current (read from the documented registry values) | switch desktop: Win+Ctrl+arrow, sent only on the user's click |
| `app` | the foreground app's name and icon | — |
| `clock` | time and date (format in `bars.json` → `top.clock: { "seconds": false, "format": "ddd d MMM  HH:mm" }`) | opens the calendar / notification center (`ms-actioncenter:`) |
| `media` | now playing (SMTC): title and artist | play/pause; scroll for next and previous |
| `tray` | the notification-area icons (see below) | left/right click forwarded |
| `network` | wifi signal / ethernet / offline (WLAN API, `INetworkListManager`) | opens quick settings |
| `volume` | level and mute (Core Audio `IAudioEndpointVolume`) | toggle mute; scroll to change |
| `battery` | percent and charging (`GetSystemPowerStatus`); hidden on desktops | power settings |
| `quick` | a settings glyph | opens quick settings (`ms-actioncenter:controlcenter/`) |
| `power` | a power glyph | menu: lock, sleep, sign out, restart, shut down (restart and shut down ask first) |
| `stats` | CPU / RAM mini gauges | Task Manager |

**The tray is best effort.** Notification-area icons belong to Explorer. The host reads them through
UI Automation (a supported accessibility API) from the (possibly hidden) taskbar, shows their
names, and uses each item's icon when one can be obtained. A click is forwarded with UIA
`Invoke`/`ExpandCollapse`. If that isn't possible on a Windows build, the module shows a single
button that opens the native overflow flyout. The fallback is logged.

## Dock

- **Items.** Pinned items come first, in order, then running apps that aren't pinned (if `running`).
  - **Running apps** are top-level windows a taskbar would show: visible, unowned, not tool windows
    (or `WS_EX_APPWINDOW`), not cloaked.
  - **Grouping** is by AppUserModelID (`SHGetPropertyStoreForWindow`, `PKEY_AppUserModel_ID`),
    falling back to the process image.
  - **Indicators**: running apps show a dot, the foreground app a brighter one.
- **Icons.** 64 px PNGs from `IShellItemImageFactory` (exe, `.lnk`, AUMID), cached by key in
  memory and in `%LOCALAPPDATA%\mypaper\icons\`, sent as data URLs.
- **Clicks.**
  - **Left**: activate the app's most recent window, or minimize it if it's already in front. If it
    isn't running, launch it.
  - **Middle**: launch a new instance.
  - **Right**: menu with its windows (activate), Pin/Unpin, New window, Close window(s).
  - **Activation** uses the foreground rights the user's click grants the dock's window (the dock
    takes focus on click; it's not `NOACTIVATE`).
- **Drag** reorders pinned items, and dragging a running app into the pinned area pins it. Both
  write `bars.json`.
- **Magnify** is a macOS-like hover zoom (CSS transform). It's disabled when reduced motion is set.
- **Auto-hide.** The dock reserves no space, slides out after 1 s without the pointer, and slides back
  in when the pointer touches that screen edge. Edge detection uses a 1 px tall transparent window
  on that edge, which receives mouse moves.

## Bridge (page ⇄ host)

All messages go through WebView2 `postMessage` as JSON.

**Page → host requests**: `{ id, method, params }` → `{ id, result }` or `{ id, error: { code, message } }`.

| Method | Params | Result |
|---|---|---|
| `hello` | — | `{ bar: "top"\|"dock", monitor, config, appearance, themeCss, dpi }` |
| `activate` | `{ app }` or `{ hwnd }` | — |
| `launch` | `{ path, newInstance? }` | — |
| `minimize` / `close` | `{ hwnd }` | — |
| `pin` / `unpin` / `reorder` | `{ path, name? }` / `{ path }` / `{ paths }` | — |
| `start` | — | — |
| `actionCenter` | `{ panel: "notifications"\|"quick" }` | — |
| `volume` | `{ set?: 0..1, mute?: bool, delta?: number }` | `{ level, muted }` |
| `media` | `{ action: "toggle"\|"next"\|"previous" }` | — |
| `tray` | `{ id, button: "left"\|"right" }` | — |
| `power` | `{ action: "lock"\|"sleep"\|"signout"\|"restart"\|"shutdown" }` | — |
| `workspace` | `{ direction: "left"\|"right" }` | — |
| `openSettings` | `{ uri }` (an `ms-settings:` URI only) | — |

**Host → page events**: `{ event, data }`.

| Event | Data |
|---|---|
| `apps` | the dock items: `{ key, name, icon?, pinned, windows: [{ hwnd, title, active }] }[]` |
| `foreground` | `{ name, icon? }` |
| `clock` | `{ now }`; the page formats it |
| `media` | `{ title, artist, app, playing }`, or null |
| `volume` | `{ level, muted }` |
| `network` | `{ kind: "wifi"\|"ethernet"\|"none", name?, signal? }` |
| `battery` | `{ percent, charging }`, or null |
| `tray` | `{ id, name, icon? }[]` |
| `workspaces` | `{ count, current }` |
| `stats` | `{ cpu, memory: { used, total } }` |
| `appearance` | `{ appearance, themeCss }` |
| `config` | the parsed `bars.json` |
| `visibility` | bool |

Events for a module are only produced while some bar shows that module.

## Core and tray

- **Core** supervises `mypaper-bars.exe` beside it, as an optional child: restarted with backoff, and
  if it's missing or crash-looping mypaper keeps running without bars.
- **Tray** gains a **Bars** submenu:
  - Top bar ✓ and Dock ✓ toggle `top.enabled` / `dock.enabled`.
  - Taskbar ▸ Keep / Auto-hide / Hide sets `taskbar.mode`.
- **Panic hotkey and Quit**: core stops the bars child, which restores the native taskbar from the
  journal. If the child was killed, core replays the journal itself (the journal format and replay
  live in `crates/bars` as a library used by both).

## Interaction with widgets

App-bar registration changes the work area. The widget host must re-lay out on `WM_SETTINGCHANGE`
with `SPI_SETWORKAREA` (and on display changes), so widgets never sit under a bar.
