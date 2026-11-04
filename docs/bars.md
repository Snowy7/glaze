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
  "launcher": { "winKey": true, "layout": "win11", "files": true },
  "tray": { "host": false },
  "calendar": { "zones": [{ "zone": "America/New_York" }, { "zone": "Asia/Tokyo", "label": "Tokyo office" }] },
  "audio": { "switchOutput": false },
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
- **`launcher`** (see "Flyouts" → "Launcher"):
  - `winKey` (default `true`): a lone Windows key opens the launcher, only while `taskbar.mode` is
    `hide`.
  - `layout`: `win11` (default), `grid`, `compact` or `dashboard`.
  - `pinned`: the launcher's pins, in the shape of `dock.pinned`; missing means the dock's pins.
  - `files` (default `true`): search recent files too.
- **`tray.host`** (default `false`): run the tray host even with the native taskbar kept. With
  `taskbar.mode: hide` it always runs.
- **`calendar`**: `zones` (at most 4) are extra clocks: `{ "zone": "<IANA name>", "label"? }`.
  `firstDay` is `monday`, `sunday` or `saturday`; missing means the locale's.
- **`audio.switchOutput`** (default `false`): lets the mixer switch the default output device. This
  uses `IPolicyConfig`, an **undocumented** COM interface (the one Sound settings uses). Without it,
  the mixer lists the devices and links to Sound settings.
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
| `start` | a mypaper mark | opens the launcher (see "Flyouts") |
| `workspaces` | virtual desktops: count and current (`VirtualDesktopIDs` / `CurrentVirtualDesktop` under `HKCU\…\Explorer\VirtualDesktops`, watched with `RegNotifyChangeKeyValue`; absent values mean one desktop) | switch desktop: Win+Ctrl+arrow, sent only on the user's click (once per step) |
| `app` | the foreground app's name and icon (the last app window in front: clicking a bar doesn't change it) | — |
| `clock` | time and date (format in `bars.json` → `top.clock`) | opens the calendar |
| `media` | now playing (SMTC): title and artist | play/pause; scroll for next and previous |
| `tray` | the pinned notification-area icons, then a chevron (see below) | clicks go to the icon's app; the chevron opens the overflow grid |
| `network` | wifi signal and name (WLAN API) / ethernet (the interface table: an up, physical, wired interface) / offline; changes from `NotifyIpInterfaceChange` and WLAN notifications | opens the network flyout |
| `volume` | level and mute (Core Audio `IAudioEndpointVolume` on the default output, with its change callback) | opens the mixer; middle click mutes; scroll to change (raising it unmutes) |
| `battery` | percent and charging (`GetSystemPowerStatus`, refreshed on `PBT_APMPOWERSTATUSCHANGE`); hidden on desktops | power settings |
| `quick` | a settings glyph | opens quick settings |
| `power` | a power glyph | menu: lock, sleep, sign out, restart, shut down (restart and shut down ask first) |
| `stats` | CPU / RAM mini gauges (sampled every 2 s) | Task Manager |
| `language` | the keyboard layout of the app in front (`ENG` over `US`), followed on foreground changes and the shell's `HSHELL_LANGUAGE` | opens the layout list |
| `notifications` | a bell with the number of notifications (re-counted every 15 s while shown: change events need package identity) | opens the notifications flyout |

**The tray** comes from the tray host (see "Flyouts" → "Tray host") while the native taskbar is
replaced (`taskbar.mode: hide`, or `tray.host`): the real icons, pinned ones in the bar and the rest
behind the chevron, as the user arranged them in Windows. Otherwise, with Explorer's taskbar kept,
the module reads Explorer's tray through UI Automation (a supported accessibility API), on a worker
thread, refreshed on UIA structure-changed events. What Windows 11 (build 26300, "26H2") exposes to
UI Automation:

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
- A hidden taskbar has no tray in the UIA tree (Explorer removes its whole XAML content; measured:
  no `SystemTray.*` element at all), which is why `hide` uses the tray host instead.
- If UI Automation is unavailable or finds no tray, the module shows a single `overflow` button that
  tries the native overflow flyout (Win+B, Enter). The fallback is logged.

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

## Flyouts

What the top-bar modules open: our own Start menu (the launcher), quick settings, network, the
mixer, keyboard layouts, the calendar, notifications, the tray overflow and the power menu. They
replace the taskbar's own menus; nothing goes into Explorer.

- **The page side** (`bars/shell/src/flyouts`) builds each on `flyout.ts`. Each big one is its own
  chunk, loaded the first time it opens; its DOM is built on open and dropped on close.
  `surface.ts` places it: under the top bar's strip, or off the dock toward the middle of the
  screen.
- **Room.** A flyout taller than the bar's `ROOM` asks the host for more first (`room`): the host
  grows the bar's window toward the middle of the screen (clipped to the monitor) and the page waits
  for the resize. The window goes back to its usual size 450 ms after the last flyout closed. The
  region keeps everything outside the flyout click-through.
- **Keyboard.** The launcher asks for the keyboard (`keyboard`): the bar becomes the foreground
  window and the WebView takes focus, and the host stops handing the foreground back while it's
  open. On close (Escape) it hands the foreground back to the app before it, unless a mouse button
  is down (the press that closed it was on another module, whose flyout then opens) or an app
  already took it.
- **Providers** (`crates/bars/src/flyouts`) run on lanes, one thread each (COM in the apartment the
  API wants). A lane starts on the first request and stops after 60 s unused, unless something it
  feeds is watched. Slow requests (DDC/CI, WLAN scans, WinRT reads) answer *later* through the main
  loop, which never blocks on them. **Nothing polls while every flyout is closed**, except the
  notification badge while a `notifications` module is on screen (below).
- **Watching.** An open flyout `watch`es what it shows live: its provider's events (`mixer`,
  `wifi`, ...) or a core topic (`media`, `stats`, ...). A core topic then runs as if a module showed
  it, for that bar only. A bar's page reload drops its watches, its room and its keyboard.
- **Styling.** Every flyout's root is `.layer > .flyout[data-flyout="<name>"]` (`launcher`, `quick`,
  `network`, `mixer`, `language`, `calendar`, `notifications`, `tray`, `menu`). The shared pieces
  are `.fly-panel`, `.fly-header`, `.fly-row`, `.fly-tile[data-on]`, `.fly-slider`, `.fly-switch`,
  `.fly-list`, `.fly-notice` and `.fly-footer`. Each flyout's parts are prefixed with its name
  (`.launcher-tile`, `.qs-tiles`, `.net-item`, `.mix-app`, `.cal-day.is-today`, `.notif-item`,
  `.tray-cell`, `.lang-badge`). They're drawn from the appearance tokens like the bars, and
  monochrome by default: "on" is the text color as a fill. Durations and curves are `--fly-duration`
  and `--ease`, so presets retime them. The flyouts' glyphs are in `flyouts/glyphs.ts`.

### Launcher

The `start` module (and the Windows key, below) opens the launcher.

- **Index.** Every app the Start menu lists, from `shell:AppsFolder`
  (`SHGetKnownFolderItem(FOLDERID_AppsFolder)` enumerated with `BHID_EnumItems`):
  - packaged apps by AUMID, and the Start Menu folders' shortcuts (user and common);
  - names from the shell; icons from `IShellItemImageFactory` (64 px, in the icon cache);
  - each shortcut's target exe as a search keyword (`msedge` finds Microsoft Edge);
  - uninstallers, readmes and web links left out.

  It's built on its lane at background priority, 8 s after start (or at the first `apps` request).
  It's cached in `%LOCALAPPDATA%\mypaper\apps.json`, and rebuilt when the Start Menu folders change
  (a file watch, settled 3 s) or when the launcher opens more than 10 minutes after the last build
  (packaged installs don't touch those folders).
- **Search** (`flyouts/launcher/fuzzy.ts`) covers apps, about 70 Windows Settings pages
  (`settings-pages.ts`, `ms-settings:` URIs with keywords) and, with `launcher.files`, the recent
  files (`%APPDATA%\Microsoft\Windows\Recent`, read when the launcher opens).
  - Ranking: exact > prefix > a word's prefix > initials (`vsc`) > substring > in-order letters
    (only from 3 typed). Apps come before settings, and settings before files.
  - What the user runs often and lately goes first (`launcher.json`: each run's count and time).
  - Results show as a best match, then apps (8), settings (5) and files (5).
- **Keyboard first.** Typing goes to the search box from anywhere, the arrows walk the list (and
  the pinned grid in two dimensions), Enter opens, Escape closes.
- **Layouts** (`launcher.layout`):

  | Layout | Shows |
  |---|---|
  | `win11` | search, pinned (6 columns, "All apps" A–Z), recommended (recent files, else recent apps) |
  | `grid` | a big grid of every app (Launchpad) |
  | `compact` | a search bar, recent apps until you type (Raycast, Flow) |
  | `dashboard` | search, pinned, a widgets row (time and date, now playing with controls, CPU / memory / battery) and the clipboard history |

  Every layout but `compact` has a footer: the user (`GetUserNameExW(NameDisplay)`), Settings, and
  power (lock, sleep, sign out, restart, shut down; the last two ask first).
- **Pins.** Right-click an app to pin it to the launcher (`launcher.pinned`, which starts as the
  dock's pins), unpin it, or pin it to the dock.
- **Clipboard history** is Windows' own (Win+V, `Clipboard.GetHistoryItemsAsync`). It answers an
  unpackaged process on build 26300 (`examples/winrt-spike.rs`), so there's no history of our own.
  Picking an entry makes it the clipboard's content. With history off, the launcher links to its
  setting.

**The Windows key.** With `launcher.winKey` and `taskbar.mode: hide`, a low-level keyboard hook
(`WH_KEYBOARD_LL`, `flyouts/winkey.rs`) runs in our process, on a thread of its own; nothing is
loaded anywhere else.
- It lets every key through. The one exception is the release of a **lone** press of the Windows
  key: the hook swallows that release and replays it after the unassigned key `vkE8` (AutoHotkey's
  "menu mask key"). Explorer then sees a combination and keeps Start closed, and the launcher opens.
- Combinations (Win+E, Win+Shift+S, ...) pass untouched.
- With a fullscreen app in front (`SHQueryUserNotificationState`, or a window covering its
  monitor), or no bar to show the launcher, the release passes and the native Start opens.
- The keys the bars inject themselves carry `dwExtraInfo` `OURS`, and the hook ignores them. Other
  injected input counts like a keyboard's (on-screen keyboards, remote desktop).
- The launcher opens on the monitor of the app in front, on its top bar if it has one. The host
  makes the bar the foreground window: our replayed input is the last input, so Windows allows it.

### Quick settings

- **Wi-Fi and Bluetooth** on/off: `Windows.Devices.Radios` (`RequestAccessAsync` was `Allowed` for
  the unpackaged process). The Wi-Fi tile's chevron opens the network flyout.
- **Power mode** cycles efficiency → balanced → performance: the Settings slider's overlay schemes,
  `PowerGetEffectiveOverlayScheme` / `PowerSetActiveOverlayScheme` from powrprof.dll (not in the
  SDK headers).
- **Battery saver** (on machines with a battery), **do not disturb**
  (`ToastNotificationManager.GetDefault().NotificationMode`) and **focus**
  (`FocusSessionManager.IsFocusActive`) show their state. Clicking opens their Settings page:
  changing them has no documented API. **Night light** opens its page for the same reason.
- **Brightness:** the built-in panel through WMI (`WmiMonitorBrightness`,
  `WmiMonitorBrightnessMethods.WmiSetBrightness`), else external monitors through DDC/CI
  (`GetMonitorBrightness` / `SetMonitorBrightness`; one slider for all of them). The slider is
  hidden without either.
- **Volume** with mute and a chevron to the mixer, the battery level, and a link to Settings.

### Network

- **Wi-Fi:** the radio's switch and the networks in range (`WlanGetAvailableNetworkList`).
  - One entry per SSID: the connected one, else the saved one, else the strongest. Each shows its
    signal and security.
  - It scans on open (`WlanScan`); the WLAN scan-complete notification updates the list.
- **Connecting:**
  - A saved network connects with its profile (`WlanConnect`).
  - A new WPA/WPA2/WPA3-Personal or open network gets a profile written here
    (`wifi::profile_xml`, the schema `netsh wlan export profile` writes; the SSID also in hex),
    saved with `WlanSetProfile`, then connected.
  - Enterprise and WEP networks link to Wi-Fi settings.
  - The connected network offers Disconnect.
- **Location consent (24H2 and later):** desktop apps see Wi-Fi networks only with location access
  on. `WlanGetAvailableNetworkList` returns `ERROR_ACCESS_DENIED`, and the flyout explains this and
  links to `ms-settings:privacy-location`.
- **Ethernet** shows when wired (the `network` topic). **VPN** links to `ms-settings:network-vpn`.

### Mixer

- **Output:** the device in use and the master volume and mute (the `volume` topic).
- **Devices** (`EnumAudioEndpoints`, friendly names). They're switchable only with
  `audio.switchOutput` (`IPolicyConfig::SetDefaultEndpoint` for every role, an undocumented
  interface); otherwise there's a link to Sound settings.
- **Apps:** every app playing on the default output (`IAudioSessionManager2` sessions; expired
  ones left out), system sounds first, then playing apps.
  - Each has its own slider and mute (`ISimpleAudioVolume`).
  - Names come from the session's display name (indirect strings resolved), else the exe's
    `FileDescription`. Icons come from the exe.
- **Live:** new sessions (`IAudioSessionNotification`), per-session volume and state changes
  (`IAudioSessionEvents`) and default-device changes (`IMMNotificationClient`) refresh the open
  mixer. Rows are kept across refreshes, so a slider being dragged isn't rebuilt under the pointer.

### Language

- The installed layouts come from `GetKeyboardLayoutList`. Each is named by its locale
  (`GetLocaleInfoEx`: "English (United States)", `ENG`, `US`) and its layout's registry name (the
  KLID from the HKL's high word: a substitute layout, an IME, or an `Fxxx` index mapped through
  `Layout Id`; `Layout Display Name` resolved).
- Picking one posts `WM_INPUTLANGCHANGEREQUEST` to the app in front, the documented request (its
  default window procedure activates the layout). Console and some UWP windows ignore it.

### Calendar

Today (weekday, date, time), a month grid you page through (buttons, the wheel, "Today"), and the
extra clocks of `calendar.zones` (local time, day, offset from here). Everything comes from Intl,
and the first weekday is the locale's unless `calendar.firstDay` says otherwise. "Calendar" opens
`outlookcal:`.

### Notifications

The open spike (research 4.2, 7), measured with `cargo run -p mypaper-bars --example winrt-spike`
on build 26300, unpackaged:
- `UserNotificationListener.GetAccessStatus()` and `RequestAccessAsync()` both return `Allowed`.
- `GetNotificationsAsync(Toast)` returns every toast (52 that day), with app names.
- `NotificationChanged` refuses to register: `0x80070490` ("Element not found"), needing package
  identity. So the flyout re-reads the list every 4 s while it's open, and the `notifications`
  module's badge is re-counted every 15 s while the module is on screen (a periodic topic, which
  stops for fullscreen apps).
- Removing one toast (`RemoveNotification`) works unpackaged; `ClearNotifications` may not, so
  "Clear all" falls back to removing them one by one.

No sparse package was needed, so none is shipped. The flyout groups toasts by app, newest first
(the app's logo from `AppDisplayInfo.GetLogo`, the title and body from the toast's text elements),
with dismiss, clear all and a link to the notification center. With access turned off in Settings
it says so and links to `ms-settings:privacy-notifications`.

### Tray host

How the `tray` module gets the real notification-area icons without injection, the ManagedShell /
RetroBar way (research 2.5 and 4.1; `crates/bars/src/tray_host.rs`, wire formats in
`tray_data.rs`).

**When it runs.** Only while a tray module is on screen **and** the native taskbar is replaced
(`taskbar.mode: hide`, or `tray.host: true`), on its own thread:
1. It creates a hidden, topmost window of class `Shell_TrayWnd` (with a `TrayNotifyWnd` child),
   sized like the top bar that shows the tray.
   - `Shell_NotifyIcon` and `SHAppBarMessage` find the first `Shell_TrayWnd` in Z-order, so the host
     keeps its window above Explorer's.
   - It re-checks on Explorer's own window events (out-of-context WinEvents filtered to Explorer's
     process), and with one `FindWindow` every 3 s. Measured: hiding the taskbar makes Explorer
     raise its tray a dozen times in the first second, and the events catch each one.
2. It broadcasts `TaskbarCreated`, so every running app adds its icons again, now to the host. That
   is the initial enumeration; the bars ignore their own broadcast.
3. It keeps each icon: owner window, uID or GUID, callback message, `NOTIFYICON_VERSION`, tooltip,
   hidden state, and a **copy** of the `HICON` (apps destroy theirs right after the call) as a PNG
   `data:` URL. Each message is answered at once (`ReplyMessage`).
4. **Everything is passed on to Explorer's real tray**, so Explorer's state stays correct:
   - balloons still become toasts;
   - `NotifyIconSettings` keeps learning about new icons;
   - nothing is lost when the host goes away.

   Icon messages are queued and forwarded one at a time (with the host's copy of the icon). App-bar
   messages are forwarded synchronously, with `ABM_QUERYPOS`/`ABM_SETPOS` re-homed: `SHAppBarMessage`
   shares its data with the receiving process, so the host copies it into a block shared with
   Explorer and copies the answer back. `ABM_GETTASKBARPOS` is answered with the bar's rect, so
   apps open their popups near our tray. `WM_COMMAND`, `SC_TASKLIST` and the `WM_USER` range go to
   Explorer as they came (sent or posted). Registered messages don't: they're broadcasts Explorer
   gets itself.

**Clicks** go to the icon's owner as its callback message (`SendNotifyMessage`), after
`AllowSetForegroundWindow(owner)` (the click on the bar gave us the foreground to pass on).
- Version 4 packs the pointer into `wParam` and the message and uID into `lParam`; older versions
  send the uID and the message.
- Left: mouse move, down, up, then `NIN_SELECT` (v3+). Right: move, down, up, then `WM_CONTEXTMENU`
  (v3+). Middle and double click too.
- `Shell_NotifyIconGetRect` gets the icon's rect from its last click (or the pointer).
- Icons whose owner window is gone, or recycled by another process, are dropped.

**Pins and order** come from `HKCU\Control Panel\NotifyIconSettings`, as the user arranged them in
Windows:
- An icon is matched to its entry by exe and uID or GUID.
- `IsPromoted` icons show in the bar; the rest are in the overflow grid.
- `UIOrderList` sets the order.
- The key is watched (`RegNotifyChangeKeyValue`, re-read at most every 2 s), so a change in
  Settings shows at once.

**Caveats, and what the host does about them:**
- Explorer's restart (Zebar #301): a second `Shell_TrayWnd` during Explorer's start keeps it from
  building its taskbar. So when Explorer's process exits, or its tray window is destroyed, the host
  removes its window at once. It comes back 4 s after Explorer's own `TaskbarCreated`, and never
  before Explorer has run for 4 s.
- Stopping (the tray module off, `taskbar.mode` back to `keep`, quitting) destroys the window and
  broadcasts `TaskbarCreated`, so apps re-register with Explorer.
- A crash leaves nothing behind: the window dies with the process, and Explorer has every icon.
- The bars' own `Shell_TrayWnd` lookups (`taskbar::explorer_tray`, `taskbar_windows`) skip our
  process, so hiding and restoring the taskbar never touches the host's window.
- Apps that don't handle `TaskbarCreated` keep their icon in Explorer only (Seelen and Zebar note
  the same).
- Elevated apps' icons arrive, but our clicks can't reach them unless they let the callback
  message through UIPI.
- Windows' own volume and network icons aren't tray icons on 24H2+; the modules show those. The
  volume icon still arrives as an Explorer icon ("Speakers (USB): 94%").
- Taskbar overlay badges and progress (`ITaskbarList3`) aren't tray messages and stay out of reach
  (research 2.5).

### Power

The `power` module's menu (`menu.ts`): lock, sleep, sign out, restart, shut down; restart and shut
down ask first. The launcher's footer has the same actions.

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
| `tray` | `{ id, button: "left"\|"right"\|"middle"\|"double", rect? }` (`rect`: the icon in DIPs from the window's top-left) | — |
| `power` | `{ action: "lock"\|"sleep"\|"signout"\|"restart"\|"shutdown" }` | — |
| `workspace` | `{ direction: "left"\|"right" }` | — |
| `openSettings` | `{ uri }` (an `ms-settings:` URI only) | — |
| `region` | `{ rects: [x, y, width, height][] }` (DIPs from the window's top-left, at most 64) | — |

`pin` of an item that's already pinned moves it to `index`. `reorder`'s paths are matched
case-insensitively; pins it doesn't list keep their order after the listed ones.

The flyouts' methods (`crates/bars/src/flyouts/mod.rs` → `METHODS`; the page's side is
`bars/shell/src/flyouts/protocol.ts`). Requests marked *later* are answered by a provider's lane
once it has the answer, so the reply may come after other messages:

| Method | Params | Result |
|---|---|---|
| `watch` / `unwatch` | `{ topic }`: `mixer`, `wifi`, `launcherApps`, `notificationList`, or a core topic (`media`, `volume`, `network`, `battery`, `stats`, `clock`, `language`, `notifications`) | — |
| `room` | `{ extent }`: DIPs past the band the window must reach (0: back to `ROOM`) | — |
| `keyboard` | `{ on }`: the bar becomes the foreground window with the page focused, or hands it back | — |
| `apps` | — | `{ apps: [{ id, name, path, icon?, keywords?, packaged }], recent: [{ id, path, count, last }], user: { name } }` |
| `run` | `{ id, path }` | `{ recent }` (opens it and records the run) |
| `recentFiles` | — | *later*: `[{ name, path, at, icon? }]` (newest first, at most 40) |
| `clipboard` / `clipboardPaste` | — / `{ index }` | *later*: `{ enabled, entries: [{ kind: "text"\|"link"\|"image"\|"other", text, image? }] }` / — |
| `quick` | — | *later*: `{ radios: [{ kind, name, on }], brightness?, power?, saver?, dnd?, focus? }` |
| `setRadio` / `setBrightness` / `setPowerMode` | `{ kind: "wifi"\|"bluetooth"\|"cellular", on }` / `{ level: 0..100 }` / `{ mode: "efficiency"\|"balanced"\|"performance" }` | *later*: the quick state / — / the quick state |
| `wifi` | `{ scan? }` | *later*: `{ networks: [{ ssid, signal, security, connected, known }], error?: "noAdapter"\|"location"\|"unavailable" }` |
| `wifiConnect` / `wifiDisconnect` | `{ ssid, password? }` / — | *later*: — |
| `mixer` | — | *later*: `{ devices: [{ id, name, default }], sessions: [{ id, name, icon?, level, muted, system, active }], switchable }` |
| `setSession` / `setOutput` | `{ id, level?, mute? }` / `{ id }` (only with `audio.switchOutput`) | *later*: — / the mixer state |
| `setLayout` | `{ id }` (an HKL in hex) | — |
| `launcherPin` / `launcherUnpin` | `{ path, name?, index? }` / `{ path }` | — (writes `launcher.pinned`) |
| `notifications` / `dismissNotification` / `clearNotifications` | — / `{ id }` / — | *later*: `{ notifications: [{ id, app, icon?, title, body, at }], error?: "denied"\|"unavailable" }` |

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
| `tray` | `{ id, name, icon?, pinned }[]` (`icon`: a PNG `data:` URL from the tray host, a cache URL from UI Automation) |
| `language` | `{ current?, layouts: [{ id, language, layout, short, region }] }` |
| `notifications` | `{ count }` (null when they can't be read) |
| `mixer` / `wifi` / `launcherApps` / `notificationList` | a watched flyout's live data (the shapes above) |
| `launcherPins` | the launcher's pins after an edit |
| `open` | `{ flyout: "launcher" }`: the Windows key (transient) |
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
  the power menu, the dock's context menu) are the `menu` flyout. The launcher, quick settings,
  network, mixer, language, calendar, notifications and tray overflow are further flyouts
  (`bars/shell/src/flyouts`, see "Flyouts").
  - A new one opens through `flyouts/surface.ts` `show()`. That gives it placement, more window
    room when it's taller than `ROOM` (360 DIPs; the `room` request), and the keyboard if it's
    typed into.
  - Its data is a provider on a lane (`crates/bars/src/flyouts/service.rs`): a `Request` variant
    plus its name in `METHODS`, and a `Watched` variant for live data.
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
