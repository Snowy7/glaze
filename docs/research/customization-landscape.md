# Desktop customization landscape

Research for Glaze's next steps: how Seelen UI, YASB, Zebar, Rainmeter, Cairo and others get system data and
style the desktop, and what Glaze can copy under its **no code injection** rule.

- **Written:** 2026-11-03. GitHub sources were read on their default branches (Seelen `master`, YASB `main`,
  Zebar `main`, ManagedShell `master`). Links point at paths, not commits, so line numbers can drift. Where a line
  number is given it is for orientation only.
- **Test machine:** Windows 11 Pro build 10.0.26300 (the OS this research ran on).
- **Rule of evidence:** every claim carries a link to the file, doc page or issue that owns it. A claim marked
  **(unverified)** could not be traced to a primary source. Blog posts are used only as pointers.

## 0. Summary

Ten findings that change what we build:

1. **Seelen UI and YASB both inject into `explorer.exe` for the tray.** Seelen installs a `WH_CALLWNDPROC` hook DLL
   (`sluhk.dll`) on the `Shell_TrayWnd` thread ([`hook_dll/src/lib.rs`][slu-hook],
   [`tray_hook_loader.rs`][slu-loader]). YASB added a hook mode in v1.9.1 (2026-03-21, "for more reliable tray icon
   detection") and its v2.0.0 notes (2026-04-28) call the overhaul "SetWindowsHookEx injection"
   ([release notes][yasb-rel]). Microsoft's own doc says
   [`SetWindowsHookEx` "can be used to inject a DLL into another process"][ms-hook]. Both are off-limits for us.
2. **The no-injection tray method is a `Shell_TrayWnd` host/spy window**, not UI Automation. ManagedShell's
   [`TrayService`][ms-tray] and Zebar's [`tray_spy.rs`][zebar-spy] (and, judging by its imports and docstring,
   YASB's legacy monitor) create their own top-level `Shell_TrayWnd`, stay above Explorer's, broadcast
   `TaskbarCreated`, forward messages to the real tray, and send clicks as `SendNotifyMessage` to each icon's
   callback message. It has a live caveat: Zebar
   [#301][zebar-301] reports it breaks the native taskbar's menus and Explorer restarts on Windows 11 25H2 (a
   user-reported root cause, not confirmed by the maintainers).
3. **Glaze's current tray design (UIA, [`docs/bars.md`](../bars.md)) has no shipping precedent as a primary
   method.** The one open-source UIA user read (`trailist`) keeps UIA only as the fallback behind its Explorer DLL.
   UIA gives names, rectangles and `Invoke`, but not the icon handle or callback message. Keep it as the
   fallback and add the host window plus the `HKCU\Control Panel\NotifyIconSettings` registry for metadata.
4. **Notifications:** `UserNotificationListener` is declared through a manifest capability
   ([Learn][ms-notif-listener], general-use capability list). Seelen declares it in its MSIX manifest
   ([`AppxManifest.xml`][slu-appx]) and falls back to 1 s polling when `NotificationChanged` can't be registered (its
   code comment: "only works on MSIX/APPX/UWP builds"). A sparse package gives an unpackaged app identity
   ([Learn, updated 2026-10-03][ms-sparse]). Whether the capability is honored through a sparse package is not stated
   in the docs: spike it.
5. **Hiding the taskbar is a polling race, not a switch.** Seelen sets `ABM_SETSTATE` autohide and `SW_HIDE` on
   every `Shell_TrayWnd` / `Shell_SecondaryTrayWnd`, retries 10 times at 50 ms, and re-hides on
   `EVENT_OBJECT_SHOW/CREATE` from an out-of-context `SetWinEventHook` ([`shutdown.rs`][slu-hide],
   [`weg/hook.rs`][slu-weghook]). ManagedShell polls every 100 ms ([`ExplorerHelper.cs`][ms-explorerhelper]). No tool
   found prevents the first flash at login; they minimize it.
6. **Autostart:** Seelen uses a **Task Scheduler logon task** (`TASK_TRIGGER_LOGON`, highest run level, priority 2)
   ([`task_scheduler.rs`][slu-task]); YASB uses the **HKCU Run key** ([`cli.py`][yasb-cli]). Microsoft says Explorer
   "may choose to delay" Run-key and Startup-folder programs ([Learn][ms-run]); a Windhawk mod author says the delay
   lives in Explorer's startup processing and does not affect Task Scheduler
   ([mod readme][wh-startup], not a Microsoft source).
7. **Windows 11 24H2 changes we must handle:** (a) Wi-Fi BSSID/scan APIs need precise-location consent and return
   `ERROR_ACCESS_DENIED` without it ([Learn][ms-wifi-loc]); (b) the RetroBar maintainer says Explorer's own network
   tray icon "was removed in Windows 11 24H2" ([RetroBar #902][retrobar-902]); (c) the desktop can be a "raised
   desktop" with a layered `SHELLDLL_DefView` ([Lively source comment][lively-core], which quotes Microsoft but does
   not name the build).
8. **API status.** Documented: per-app volume (`IAudioSessionManager2`), keyboard layout (`GetKeyboardLayout`, TSF),
   WLAN list, Radio on/off, DDC/CI brightness, `FocusSessionManager`. **Undocumented:** default output device
   (`IPolicyConfig`), setting Do Not Disturb, Night Light (CloudStore blob), the `wpndatabase.db` notification store
   (see section 4.6).
9. **WebView2 process sharing** needs the same user data folder **and** the same environment options; a mismatch makes
   the second environment fail ([Learn][ms-wv2-process]). Seelen keeps one data folder per flag set (`gpu`, `no-gpu`,
   ...) ([`webview.rs`][slu-webview]). Microsoft advises not using WebView2 for initial UI
   ([Learn][ms-wv2-perf]). No tool studied pre-renders snapshots.
10. **Competitors moved:** Microsoft's PowerToys Command Palette now ships a **Dock** (AppBar-based, per-monitor
    bands, acrylic/image backgrounds, restores itself after an Explorer restart) ([Learn, 2026-10-14][ms-cmdpal-dock]).
    Seelen's theme system is CSS with tokens and per-widget targets ([`theme-guidelines.md`][slu-theme-doc]). The
    bar to beat is "Seelen-level theming plus Microsoft-level reliability".

**Two repo notes.** `crates/inject` ([lib.rs](../../crates/inject/src/lib.rs)) describes an allowlist that permits
injecting into Windows itself, while the brief for this research says no injection at all. This document follows the
stricter brief; the crate should be reconciled. Also, `docs/bars.md` documents opening Start with `WM_SYSCOMMAND` /
`SC_TASKLIST` posted to `Shell_TrayWnd`; Open-Shell posts the same command to its `g_ProgWin` window (Progman, by
name; unverified) ([`StartMenuDLL.cpp`][os-menu]). Worth testing both on 24H2+.

## 1. Conventions

Feasibility tags used in every table:

| Tag | Meaning |
|---|---|
| `OK` | Documented public API, no code in other processes. |
| `UNDOC` | No injection, but the API, registry value or message layout is undocumented and can break on a Windows update. Wrap in a feature probe and a fallback. |
| `IDENTITY` | Needs package identity (MSIX or sparse package) and usually a signing certificate. |
| `INJECT` | Needs code inside another process. **Excluded.** |

Other notation: **Win11** means the XAML-based taskbar (build 22000+). "Fast path" means what the code does on the
hot path, not what the docs say.

## 2. Windows projects

### 2.1 Seelen UI (v2.8.10 released 2026-10-22; `master` is 2.8.11-dev)

**Architecture.** Rust + Tauri backend; every UI piece is a separate WebView2 widget (Svelte for built-ins, React
for settings) ([`AGENTS.md`][slu-agents]). Three processes matter:

- `slu-service` (admin, started by Task Scheduler) owns taskbar hiding/restoring, global hotkeys and power events
  ([`service/main.rs`][slu-svc-main], [`hotkeys.rs`][slu-hotkeys]).
- The main app (`src/background`) hosts system modules and the widget loader ([`loader.rs`][slu-loader-rs]).
- `sluhk.dll`, the hook library loaded into Explorer (`INJECT`, see below).

**Resource model.** Everything user-facing is a "resource" folder with a `metadata.yml`: `Theme`, `Widget`,
`Plugin`, `IconPack`, `Wallpaper`, with ids like `@user/name` and `!include` / `!extend` for splitting files
([`resource-guidelines.md`][slu-res-doc]).

**System data.**

| Data | How Seelen gets it | Tag |
|---|---|---|
| Tray icons + overlay badges | Hook DLL filters `WM_COPYDATA` to `Shell_TrayWnd` (NIM_ADD/MODIFY/DELETE/SETVERSION) and the undocumented taskband messages `WM_USER+79/85` for overlay icon/description; sends over a named pipe ([`lib.rs`][slu-hook]). Order, pin state and path come from `HKCU\Control Panel\NotifyIconSettings` (`IsPromoted`, `UIOrderList`) ([`system_tray/application/mod.rs`][slu-tray-mod]). Clicks: `SendNotifyMessage` of the icon's callback with `WM_LBUTTONDOWN/UP`, `NIN_SELECT`, `NIN_POPUPOPEN` ([`tray_icon.rs` `send_action`][slu-tray-icon]). After the hook is installed it broadcasts `TaskbarCreated` so apps re-register ([`util.rs`][slu-util]). | `INJECT` |
| Notifications | `UserNotificationListener` + `GetNotificationsAsync(Toast)`; polling thread if `NotificationChanged` fails ([`notifications/application.rs`][slu-notif]). Also reads the SQLite `wpndatabase.db` of the Push Notifications service for toast/badge data ([`wpn_service.rs`][slu-wpn]: "The database is undocumented"). | `IDENTITY`, `UNDOC` |
| Do Not Disturb | Read: `ToastNotificationManagerForUser.NotificationMode` (documented, [get-only][ms-notifmode]); set: quiet-hours profile in `windows_api/undocumented` ("there is no official api to do this") | `OK` read, `UNDOC` write |
| Focus session | `FocusSessionManager.IsFocusActiveChanged` ([`focus_assist/application.rs`][slu-focus]) | `OK` |
| Network / Wi-Fi | `NetworkInformation`, `INetworkListManager`, `GetAdaptersAddresses`, `WlanGetAvailableNetworkList` + `WlanGetNetworkBssList`, WinRT `WiFiAdapter`, tethering manager ([`network/application.rs`][slu-net], [`radios/wifi/adapter.rs`][slu-wifi]) | `OK` (location consent, section 4.6) |
| Audio | `IAudioSessionManager2` sessions ([`media/devices/application.rs`][slu-audio]); default device via hand-written `IPolicyConfig` COM interface ([`audio_policy_config.rs`][slu-policy]) | `OK` / `UNDOC` |
| Keyboard layout / IME | `GetKeyboardLayout(foreground thread)`, TSF `ITfInputProcessorProfileMgr::ActivateProfile`, `ImmGetDefaultIMEWnd` ([`language/application.rs`][slu-lang]) | `OK` |
| Brightness | DDC/CI through `dxva2` physical-monitor handles; "slow (tens of milliseconds, seconds on a misbehaving monitor) and must never run on the UI thread" ([`brightness.rs`][slu-bright]) | `OK` |
| Night Light | Parses the CloudStore Bond blob ([`nightlight/cloudstore.rs`][slu-night]) | `UNDOC` |
| Windows list | `SetWinEventHook` with `EVENT_MIN..EVENT_MAX`, skipped while the session is locked ([`hook.rs`][slu-hookrs]) | `OK` |
| Cursor position for hover | A dedicated "MouseEventHook" thread reads the cursor position and emits `GlobalMouseMove` ([`hook.rs`][slu-hookrs]); the dock webview is click-through and flips hit-testing from that stream ([`layered.ts`][slu-layered]) | `OK` |

**Styling and themes.** A theme maps widget ids to CSS/SCSS (`styles`), has an optional `sharedStyles`, and
declares user settings that become CSS variables (`<color>`, `<number>` with slider/dropdown, length, string, URL)
([`theme-guidelines.md`][slu-theme-doc]). System variables expose the Windows accent as seven shades
(`--system-accent-color`, `...-light-`, `...-darkest-`) and design tokens (`--slu-std-*`). Widgets opt into shared
components with `data-skin` attributes. "Extreme performance mode" forces all theme animations off. The bundled
[`bubbles` theme][slu-bubbles] turns the toolbar into floating pills in 12 lines of CSS using
`oklch(from var(--slu-std-bg-color) l c h / 0.95)`. Default theme files cover every flyout (apps menu, bluetooth,
calendar, quick settings, network, media, notifications, power, tray menu, user menu, task switcher, wallpaper
manager...) ([`themes/default/styles`][slu-themes-default]). Icon packs map app user model ids or paths to icons,
file extensions to shared icons, and widget-specific keys ([`icon_pack.rs`][slu-iconpack]).

**Settings schema (what is configurable).** Per-widget structs in
[`libs/core/src/state/settings/mod.rs`][slu-settings]:

| Widget | Options |
|---|---|
| Fancy Toolbar | `itemSize`, `margin`, `padding`, `position` (top/bottom), `hideMode`, `delayToShow`, `delayToHide` |
| Weg dock | `mode`, `hideMode`, `splitWindows`, `temporalItemsVisibility`, `pinnedItemsVisibility`, `position` (4 sides), `showInstanceCounter`, `showWindowTitle`, `size`, `margin`, `padding`, `spaceBetweenItems`, `delayToShow/Hide`, `revealOnAttention`, `showEndTask`, `middleClickAction` |
| Window manager | gaps, `border {enabled,width,offset}`, `floating`, `animations {enabled, durationMs, easeFunction}`, `dragBehavior`, `stackBarVisibility`, layout plugin per monitor/workspace |
| Wallpaper manager | `interval`, `randomize`, `multimonitorBehaviour`, `useAccentColor`, `coveragePauseThreshold`, `showPausedMessage` |
| Global | `activeThemes[]`, `activeIconPacks[]`, `performanceMode {default,onBattery,onEnergySaver}`, `hardwareAcceleration`, `pollingInterval`, `suspendOnGameMode`, per-monitor and per-theme overrides |

**Plugin / widget API.** A widget is an HTML+JS+CSS bundle in a webview with presets (`Desktop`, `Overlay`,
`Popup`), instance modes (`Single`, `Multiple`, `ReplicaByMonitor`), user settings (switch, select, range, color,
font...) and a typed JS API: `Widget.self.init()/ready()`, `invoke(SeelenCommand.*)`, `subscribe(SeelenEvent.*)`
([`widget-guidelines.md`][slu-widget-doc], [`widget-js-api.md`][slu-jsapi-doc]). **Plugins** extend an existing
widget: toolbar items are declared as YAML with `scopes` (Date, Notifications, Media, Network, Keyboard, Bluetooth,
Power, FocusedApp, Workspaces, Disk, Cpu, Gpu, Tray, ...) that inject variables into JS function-body strings
(`template`, `tooltip`, `badge`, `onClick`, `onWheelUp/Down`) ([`toolbar-plugins.md`][slu-tb-plugins]); dock plugins
render to a canvas or name an icon ([`dock-plugins.md`][slu-dock-plugins]).

**Hiding the taskbar.** See section 4.3; the code is in [`service/shutdown.rs`][slu-hide] and
[`weg/hook.rs`][slu-weghook]. Seelen also hides Explorer's notification center / toast previews by `SW_HIDE` on
`ShellExperienceHost.exe` core windows when they uncloak, and the `XamlExplorerHostIslandWindow` content windows
(the code comments mention the volume island).

**Startup and performance.** Task Scheduler logon task launches `slu-service`; the service waits up to 5 s for
`Shell_TrayWnd` then starts the UI ([`wait_for_native_shell`][slu-wait]). WebView2 args are a fixed list
(`--disable-features=...`, `--disable-background-timer-throttling`, `--disk-cache-size=0`, GPU on/off variants) and
the user data directory is chosen by flag set so same-flag widgets share one browser process
([`webview.rs`][slu-webview]). Widgets can be `lazy` (not created until triggered) ([`loader.rs`][slu-loader-rs]).
The dock is a click-through full-area webview, so the page stays loaded and only hit-testing changes
([`layered.ts`][slu-layered]).

**Injection verdict.** `INJECT` for tray and taskbar overlay badges (`sluhk.dll` via `SetWindowsHookExW`
[`tray_hook_loader.rs`][slu-loader]). Changelog shows the tray moved from earlier approaches to "win32 hooks via
dlls" in 2.4.6 ([changelog][slu-changelog]); the earlier mechanism (1.6.0 "System Tray Icons module, incomplete") is
**unverified** from the changelog alone. Everything else in the table above is in-process or documented. Seelen also
fixed an "out of bounds read inside explorer.exe on malformed tray messages" and "a busy app could freeze the native
taskbar and tray icons" in 2.8.8: injection into Explorer has real blast radius.

**Steal.**
- Resource folders with `metadata.yml`, ids, `!include`; themes as per-widget CSS maps plus user-tunable CSS
  variables.
- Windows accent as seven CSS variables; `data-skin` opt-in shared components; "performance mode forces
  animations off".
- Per-monitor and per-workspace overrides in the settings tree; `performanceMode` by power state.
- Toolbar plugins as declarative `scopes` + JS function bodies (cheap, themeable modules).
- Icon packs keyed by AUMID/path/extension; `NotifyIconSettings` for pin/order; "restore taskbar" in a separate
  watchdog process.
- Click-through full-area dock webview with hit-testing from the cursor stream.

### 2.2 YASB (Python + Qt)

- **Architecture.** Python with PyQt6; `config.yaml` + `styles.css`; v2.0.0 "a new CSS engine that enables QSS/CSS
  animations, gradients, and richer styling" ([release v2.0.0][yasb-rel]). Config home
  `C:/Users/{user}/.config/yasb/` ([`Styling.md`][yasb-styling]).
- **Widgets (≈50).** `active_window`, `applications`, `audio_visualizer`, `battery`, `bluetooth`, `brightness`, `cava`,
  `clock`, `control_center`, `cpu`, `custom`, `disk`, `dnd`, `gpu`, `grouper`, `home`, `language`, `launchpad`, `media`,
  `memory`, `microphone`, `notifications`, `power_menu`, `power_plan`, `quick_launch`, `recycle_bin`, `systray`,
  `taskbar`, `traffic`, `volume`, `weather`, `wifi`, `windows_desktops`, plus Komorebi/GlazeWM integrations
  ([`validation/widgets/yasb`][yasb-widgets]).
- **Bar looks.** `dimensions` (px, %, or `auto` to hug content), `alignment`, `blur_effect`, `animation`
  (slide/fade, ms) ([`Configuration.md`][yasb-cfg]). The **adaptive style** draws a rail plus separate "islands" per
  widget group with `-qproperty-islandradius`, `-railheight`, `-grouppadding`, `-edgeradius`, `-borderwidth`
  ([`Styling.md`][yasb-styling]): the best-documented "islands" preset anywhere.
- **Interaction.** Per-widget `callbacks` for left/middle/right (`toggle_window`, `close_app`, `context_menu`,
  `exec <cmd>`), taskbar widget with grouping, drag reorder, hover preview thumbnails (`preview.delay`), animation
  `{enabled, duration}` ([`(Widget)-Taskbar.md`][yasb-taskbar]).
- **System data.**
  - Tray: `use_hook: true` injects `YASBTrayHook.dll` into Explorer with `SetWindowsHookEx(WH_GETMESSAGE)`; the DLL
    subclasses `Shell_TrayWnd`, extracts icon RGBA inside Explorer, and writes to the named pipe
    `yasb_systray_monitor` ([`trayhook.cpp`][yasb-hook], [`systray_hook.py`][yasb-hookpy]). The docs table lists
    `use_hook` default `true`, its description text says "Default is false" ([`(Widget)-Systray.md`][yasb-systray-doc]).
    Legacy mode is the host-window approach by its imports and docstring ("intercepts systray messages",
    `SetWindowPos(HWND_TOPMOST)`, `find_real_tray_hwnd`) ([`systray_monitor.py`][yasb-monitor]). History: hook mode added in
    [v1.9.1][yasb-rel] (2026-03-21); an earlier `CreateRemoteThread` injection was replaced by `SetWindowsHookEx` in
    v2.0.0. `INJECT`.
  - Notifications: `UserNotificationListener.request_access_async()` from an unpackaged Python process; subscribes to
    undocumented WNF state names for change events; a comment notes `ClearNotifications()` "fails with
    ERROR_NOT_FOUND without package identity" ([`windows_notification.py`][yasb-notif],
    [`undocumented.py`][yasb-undoc]). `IDENTITY`/`UNDOC`.
  - Backdrop: `SetWindowCompositionAttribute` accent policy and `DWMWA_MICA_EFFECT` ([`undocumented.py`][yasb-undoc]).
- **Startup.** HKCU `...\CurrentVersion\Run` ([`cli.py`][yasb-cli], [`registry_keys.py`][yasb-reg]). Single-instance
  guard in code.
- **Steal.** The adaptive island geometry (rail + islands + edge radius); `dimensions: auto`; per-widget `callbacks`
  strings; theme manager with a `yasb-themes://` install protocol ([`Styling.md`][yasb-styling]); `hide_icons` list;
  popup grid for overflow tray with blur/corner options.

### 2.3 Zebar and GlazeWM (glzr-io)

- **Zebar** (Rust + Tauri, cross-platform). Widgets are web pages in a "widget pack" with a `zpack.json`
  ([schema][zebar-schema], [starter][zebar-starter]). Per widget: `zOrder`, `transparent`, `focused`, `resizable`,
  `shownInTaskbar`, `caching` rules, `privileges.shellCommands`, and **presets** with `anchor`, `offsetX/Y`, `width`,
  `height`, `monitorSelection`, and `dockToEdge {edge, windowMargin}`. Data comes from "providers" (audio, battery,
  cpu, disk, host, ip, keyboard, komorebi, media, memory, network, systray, weather) declared in the page's JS
  ([`providers/`][zebar-providers]). Marketplace of packs.
- **Zebar tray** (`systray-util`): the `Shell_TrayWnd` spy. Its README states the mechanism and the `WM_COPYDATA`
  `dwData` classes ([README][zebar-spy-readme]); the code sets a 100 ms timer to "regain tray priority" and
  logs "Failed to retrieve initial tray icons. This is expected on W11" ([`tray_spy.rs`][zebar-spy]). The README
  numbers `dwData` as 1 = appbar, 2 = tray, 3 = position; the code (and ManagedShell, Seelen) use 0 = appbar,
  1 = tray, 3 = icon identifier. Trust the code. Known problems: [#301][zebar-301] (breaks native taskbar menu clicks
  and Explorer restart on 25H2 build 26200.7462, reporter's root-cause analysis), [#287][zebar-287] (icons with zero
  window handle and no GUID are dropped).
- **GlazeWM** (tiling WM). YAML config: `gaps`, `window_effects` (focused/other borders, corner style, transparency,
  hide titlebar), `hide_method: cloak`, `startup_commands: ['shell-exec zebar']`
  ([sample config][glaze-config]). The border/corner effects are `DwmSetWindowAttribute` on other processes' windows
  (`DWMWA_BORDER_COLOR`, `DWMWA_WINDOW_CORNER_PREFERENCE`) ([`native_window.rs`][glaze-dwm]) - documented API,
  `OK` on Win11 build 22000+ ([Learn][ms-dwm]).
- **Steal.** `zpack.json` presets with `anchor`/`dockToEdge`/`monitorSelection`; per-widget `shellCommands`
  allowlist; request caching rules; GlazeWM window effects (accent border, corner style) as a Windows-level toggle
  (Glaze already does this: [`docs/windows-styling.md`](../windows-styling.md)).

### 2.4 Rainmeter

C++/GDI+ skin engine. Skins are `.ini` files with a `[Rainmeter]` section (update interval, default 1000 ms),
**measures** (CPU, Memory, Net, Time, WebParser, Plugin, Script, WiFiStatus...) and **meters** (Bar, Bitmap, Button,
Histogram, Image, Line, Rotator, Roundline, Shape, String) ([measures][rm-measures], [meters][rm-meters],
[skins][rm-skins]). `MeterStyle` inheritance and `DynamicVariables` allow reuse; later meters paint over earlier ones.
Plugins are native DLLs exporting `Initialize`, `Reload`, `Update`, `Finalize` (+ optional `GetString`,
`ExecuteBang`, custom functions) ([C++ plugin API][rm-plugin]). That plugin DLLs run inside `Rainmeter.exe` is
standard but **not stated on that page (unverified here)**.
**Steal.** `UpdateDivider` (per-item update rates), `IfCondition` actions, skin groups, meter style inheritance,
`Substitute` for value-to-icon mapping. Rainmeter's retained, declarative "measure then meter" split maps well onto
Glaze's `bars` modules.

### 2.5 Cairo Desktop and ManagedShell

[Cairo](https://github.com/cairoshell/cairoshell) is a WPF desktop environment (menu bar, taskbar, its own desktop
icon surface, extensions) built on [ManagedShell](https://github.com/cairoshell/ManagedShell), also used by
[RetroBar](https://github.com/dremin/RetroBar). ManagedShell "supports running as a Windows Explorer replacement, or
running alongside Windows Explorer" ([README][ms-readme]). This is the best open reference for no-injection shell
work. Details:

**TrayService (alongside Explorer).** [`TrayService.cs`][ms-tray]:
1. Registers window classes `Shell_TrayWnd` and child `TrayNotifyWnd` and creates a `WS_EX_TOPMOST | WS_EX_TOOLWINDOW`
   popup (`RegisterTrayWnd`, `RegisterNotifyWnd`).
2. `Run()` broadcasts the registered message `TaskbarCreated` (`SendNotifyMessage(HWND_BROADCAST, ...)`) so every app
   calls `Shell_NotifyIcon(NIM_ADD)` again, now to our window.
3. `WndProc` handles `WM_COPYDATA`: `dwData` 0 = appbar message (`APPBARMSGDATAV3`), 1 = `SHELLTRAYDATA`
   (tray icon), 3 = `WINNOTIFYICONIDENTIFIER` (icon rect query). For `dwData == 0` it can answer or forward.
4. **It forwards** `WM_COPYDATA`, `WM_ACTIVATEAPP`, `WM_COMMAND` and everything `>= WM_USER` to Explorer's real tray
   (`ForwardMsg`: `SendMessage`, or `PostMessage` for `WM_USER+372`), so Explorer keeps working.
5. A 100 ms `trayMonitor` re-raises our window to `HWND_TOPMOST` if `FindWindow("Shell_TrayWnd")` returns another
   one (`FindWindow` walks Z-order, so topmost wins); `SetWindowsTrayBottommost` pushes Explorer's below.
6. `WM_WINDOWPOSCHANGED` with `SWP_SHOWWINDOW` strips `WS_VISIBLE` so our host window never shows.
7. `Dispose()` destroys the windows and, when not running as the shell, sends `TaskbarCreated` again so apps
   re-register with Explorer.

**NotifyIcon** drives clicks: `SendNotifyMessage(hWnd, CallbackMessage, wParam, message | (hiWord << 16))` with
`WM_LBUTTONDOWN/UP`, `WM_RBUTTONUP`, `NIN_POPUPOPEN/CLOSE`, `NIN_SELECT`, `WM_CONTEXTMENU` for version >= 3, after
`AllowSetForegroundWindow(procId)` ([`NotifyIcon.cs`][ms-notifyicon]). `Shell_NotifyIcon` semantics are documented in
[Learn][ms-notifyicon-doc] (including `NOTIFYICON_VERSION_4` event differences).

**Initial enumeration** ([`ExplorerTrayService.cs`][ms-explorertray]): `OpenProcess(PROCESS_ALL_ACCESS)` +
`VirtualAllocEx` into Explorer and `TB_GETBUTTON` on `TrayNotifyWnd`'s toolbar: cross-process memory work that only
makes sense on the Win10 toolbar tray. Zebar copies the same idea and admits it fails on Windows 11
([`tray_spy.rs`][zebar-spy]). Treat as `INJECT`-adjacent: excluded.

**System icons.** `ShellServiceObject` starts the shell system tray objects (volume, network, power) **only when
running as the shell** ([`ShellServiceObject.cs`][ms-sso]). Alongside Explorer you do not get them from tray messages.
RetroBar's maintainer says the Wi-Fi icon was removed in 24H2 ([#902][retrobar-902]); battery/USB icons are flaky
([#1253][retrobar-1253]).

**AppBar and taskbar hiding** ([`ExplorerHelper.cs`][ms-explorerhelper]): remembers `ABM_GETSTATE` at startup, then
`ABM_SETSTATE(ABS_AUTOHIDE)` + `SetWindowPos(HWND_BOTTOM, SWP_HIDEWINDOW)` on the Explorer tray, its Start button
(window class atom `0xC017`) and each `Shell_SecondaryTrayWnd`; a 100 ms timer re-hides; `ShowTaskbar` restores the
saved state. [`AppBarManager`](https://github.com/cairoshell/ManagedShell/blob/master/src/ManagedShell.AppBar/AppBarManager.cs)
implements `ABM_NEW/SETPOS`.

**TasksService.** [`TasksService.cs`][ms-tasks] creates a hook window, calls `SetTaskmanWindow` and
`RegisterShellHookWindow`, registers messages `SHELLHOOK`, `TaskbarCreated`, `TaskbarButtonCreated`, handles
`HSHELL_*` (created, destroyed, activated, flash, monitor changed, fullscreen enter/exit, get-min-rect), and
`SetWinEventHook(EVENT_OBJECT_CLOAKED..UNCLOAKED)`. The `ITaskbarList3` messages (`WM_USER+64` progress,
`+65` state, `+76..78` thumb bar...) only reach this window when it **is** the taskband; alongside Explorer they go to
Explorer's taskband. That is exactly the gap Seelen closes with `TBM_SET_OVERLAY_*` in its hook DLL (my inference from
comparing the two sources: Seelen's DLL comment links to this file). So: **taskbar overlay badges and progress are
not available to a no-injection dock**, except by polling per-window properties we can read ourselves.

**Start menu.** [`ShellHelper.cs`][ms-shellhelper] `ShowStartMenu` = `SendInput` Win key tap; `ShowActionCenter` =
Win+A, `ShowNotificationCenter` = Win+N (Win11 only), Win+S, Win+X, Win+Tab. Native flyouts (action center, network,
battery, clock) via undocumented `ImmersiveShell` COM (`IServiceProvider`, `IShellExperienceManagerFactory`, GUIDs in
[`ImmersiveShellHelper.cs`][ms-immersive]) `UNDOC`.

**Cairo desktop.** Its desktop is a WPF window sent to `HWND_BOTTOM` with its own icon view, and right-click is
`ShellFolderContextMenu` (native `IContextMenu` of the Desktop folder) ([`Desktop.xaml.cs`][cairo-desktop],
[`ShellFolderContextMenu.cs`][ms-ctxmenu]). Extensions come via `IExtensionService`.

**Steal.** The whole `TrayService` pattern including forwarding, bottom-most Explorer tray and `TaskbarCreated`
courtesy on dispose; `ShellContextMenu` helpers for showing native shell menus for a path; Win-key combos for native
UI; `ABM_GETSTATE` journal before hiding.

### 2.6 Open-Shell

C++ Start menu replacement, Win7 to Win11, "originally Classic Shell" ([README][os-readme]). `StartMenuDLL.dll`
is hooked into Explorer: `SetWindowsHookEx(WH_GETMESSAGE, HookProgManThread, ..., progThread)` and
`WH_MOUSE` on the desktop thread ([`StartMenuDLL.cpp`][os-menu] near lines 3078-3082). `INJECT`. Useful as a
reference for what a start-button hijack requires (intercepting the Win key and button press in Explorer) and for
the `SC_TASKLIST` command it posts. Not copyable.
**Steal.** Nothing to copy directly; the lesson is that taking over Start means living inside Explorer.

### 2.7 Taskbar/Start modders: StartAllBack, ExplorerPatcher, Windhawk, TranslucentTB

| Tool | What it hooks or injects | What it achieves | Source |
|---|---|---|---|
| **Windhawk** | Global injection engine; mods are C++ files loaded into target processes (`@include explorer.exe`); engine is `windhawk.dll` | Any in-process patch. Its Taskbar Styler edits the Win11 XAML visual tree by element selector. | [README][wh-readme], [styler mod][wh-styler], [styling guide][wh-guide] |
| **ExplorerPatcher** | In-process DLL inside Explorer (its sources have `dxgi_imp.cpp`, IAT patching via `valinet/hooking/iatpatch.h`, hooks on `twinui.pcshell.dll` and `windowsudk.shellcommon`) plus a DWM component (`ep_dwm`); the installer "will close `explorer.exe` and install the necessary files". The exact load vector is not spelled out in the README. | Win10 taskbar, Start, Alt-Tab on Win11 | [`dllmain.c`][ep-main], [README][ep-readme] |
| **StartAllBack** | Closed source. Its site lists taskbar/Start/Explorer/context-menu/themes and says nothing about mechanism. **Mechanism unverified.** | Labels, edge docking, classic Start, rounded acrylic menus, recolor | [startallback.com][sab] |
| **TranslucentTB** | `SetWindowCompositionAttribute` (undocumented) on `Shell_TrayWnd` and children, `IAppVisibility` sink for Start, `SetWinEventHook`. It installs `ExplorerHooks.dll` via `SetWindowsHookEx` only when the taskbar type is not XAML (the `!= TaskbarType::XAML` branch), so the Win11 XAML taskbar path involves no injection. | Transparent/acrylic/blur taskbar with dynamic states | [`taskbarattributeworker.cpp`][ttb] |
| **Open-Shell** | see 2.6 | | |

**Windhawk styling vocabulary** is the best public catalogue of Windows 11 taskbar looks: themes TranslucentTaskbar,
DockLike, SimplyTransparent, Squircle, Matter, WinXP, Bubbles, RosePine, WinVista, LiquidGlass, FrostedAcrylic;
per-state running indicators (`ActiveRunningIndicator`, `InactiveRunningIndicator`, `RequestingAttentionRunningIndicator`);
button corner radius, label font, indicator fill/height/width, tray icon size/spacing, hide individual system icons,
transforms (`RenderTransform`, `CompositeTransform3D`), gradients, `ImageBrush`, and `WindhawkBlur` with
`BlurAmount`, `TintColor`, `TintOpacity`, `TintLuminosityOpacity`, `TintSaturation`, `NoiseOpacity`, `NoiseDensity`
([guide][wh-guide]). Mica is "not possible" there. Use it as the checklist for what our dock/top bar themes expose.

**Steal.** TranslucentTB's *dynamic states* (different appearance when a window is maximized, Start open, Search
open, Task View open) via `IAppVisibility` + win events, applicable to our own windows. Windhawk's state-keyed
selectors (`Property@State=value`) as a theming idea.

### 2.8 Docks, launchers and wallpaper engines

| Tool | Customization surface worth knowing | Source |
|---|---|---|
| **Nexus Dock** (Winstep) | Mouse-over, launch, attention and delete effects (zoom, swing, bounce, glow, rock, squish, spin, explode); "fluid" liquid effects; live reflections; unlimited nested docks; stacks; blur-behind + colorization (blur only in rectangular areas on Win10+); docks can "reserve screen space"; widgets (clock, weather, CPU/RAM/net meters, calendar, battery); imports skins from other docks | [winstep.net/nexus][nexus] |
| **Winstep Xtreme** (v26.9) | Docks + shelves + NextSTART menus/hotspots + optional taskbar replacement; shared themes across components; auto monitor profiles | [winstep.net/xtreme][xtreme] |
| **RocketDock** | Icon zoom, positioning/layering, auto-hide and popup-on-mouse-over, minimize-to-dock, running indicators, window previews, ObjectDock docklets, imports MobyDock/ObjectDock/RK Launcher/Y'z skins; no version/date on its page (copyright 2023) | [punklabs.com/rocketdock][rocketdock] |
| **PowerToys Command Palette** | Win+Alt+Space launcher with list/detail/form/markdown/grid pages; extensions are packaged .NET apps (`windows.appExtension` `com.microsoft.commandpalette` + out-of-proc COM server, i.e. **package identity**); **Dock**: start/center/end regions, bands, per-monitor layout, position, size (default/compact), Material (transparent/acrylic), color tint/image background with brightness, blur, fit; uses AppBar API, restores after Explorer restart | [overview][ms-cmdpal], [Dock][ms-cmdpal-dock], [extensions][ms-cmdpal-ext] |
| **Flow Launcher** | .NET launcher with C#/Python/JS/executable plugins and XAML themes. **Not verified**: the official docs page returned no body to my fetch. | (no usable source) |
| **Lively Wallpaper** | Video/GIF/web/app/screensaver wallpapers; `LivelyProperties.json` customization callback `livelyPropertyListener(name, val)`; mouse input forwarded with **Raw Input `RIDEV_INPUTSINK`**, keyboard opt-in; attaches via `WorkerW`/`Progman`, handles Win11 "raised desktop" layout | [wiki][lively-wiki], [`WinDesktopCore.cs`][lively-core], [`RawInputMsgWindow`][lively-raw] |
| **Wallpaper Engine** | Web wallpapers get `wallpaperPropertyListener.applyUserProperties`, property types `slider`, `color` ("1.0 0.1 0.25" triples), `bool`, `combo`, `textinput`, `file`, `directory`; audio via `wallpaperRegisterAudioListener` (128 floats: 0-63 left, 64-127 right, ~30 Hz); FPS limiter, RGB devices, media integration page | [properties][we-props], [audio][we-audio] |

**Steal.** Wallpaper Engine's *property schema -> auto-generated UI* (Glaze widgets already follow this idea);
audio spectrum feed at a fixed 128-float contract; Lively's raw-input forwarding; CmdPal Dock's background options
(none/accent/custom tint/image + brightness/blur/fit); Nexus's per-event effect slots (hover, launch, attention,
delete) as a naming scheme for dock animation hooks.

## 3. Linux and macOS (as design references)

| Tool | Model | Reusable idea | Source |
|---|---|---|---|
| **Waybar** | JSONC config + GTK CSS (`style.css`, plus `style-light.css` / `style-dark.css` that follow the system theme). Bar options: `layer`, `position`, `exclusive` (reserve space), `passthrough` (pointer events fall through), `margin-*`, `spacing`, `modules-left/center/right`, `reload_style_on_change`, signal-based show/hide. | Live CSS reload; light/dark stylesheets; `exclusive`/`passthrough` toggles. | [Configuration][waybar-cfg], [Styling][waybar-style] |
| **Polybar** | Only the README was read; its documentation is on the wiki, so no mechanism claims are made here. | (not assessed) | [README][polybar] |
| **eww** | Own Lisp-like `yuck` for structure + SCSS/CSS; windows have `geometry`, `stacking`, `reserve`/struts, `windowtype`, per-monitor; GTK CSS limits (some animations unsupported, no flexbox). | Window/struts declaration separate from content. | [configuration.md][eww-cfg] |
| **AGS / Astal** | Astal is "a collection of libraries written in Vala and C" that "handles the backend logic", one library per system service (`apps`, `battery`, `bluetooth`, `network`, `notifd`, `tray`, `wireplumber`, `mpris`, `hyprland`, `powerprofiles`, `brightness`, ...). AGS (a separate project) was not inspected. | **Service-per-library** layering; one crate per system service in glaze. | [Astal][astal] |
| **KDE Plasma** | Widgets are QML packages with `metadata.json`; user config via a configuration page; Plasma Style is SVG. **Only the doc index was read**, so panel-floating and data-source specifics are unverified. | Package layout; SVG theme parts. | [Plasma widgets][kde-widget] |
| **Kvantum** | SVG-based Qt theme engine; a theme is an SVG + config. | SVG skinning for fixed UI parts. | [README][kvantum] |
| **Hyprland** | Appearance variables and an animation *tree*; curves are cubic Bezier (two control points) or spring (mass/stiffness/damping); `speed` in deciseconds; window styles `slide`, `popin`, `gnomed`; workspace styles `slide`, `slidevert`, `fade`, `slidefade`. Decoration: `rounding` + `rounding_power` (2.0 circle, 4.0 squircle), `gaps_in`/`gaps_out`, blur (`size`, `passes`, `noise`, `contrast`, `brightness`, `vibrancy`, `variant`), shadow (`range`, `render_power`, `color`), glow, motion blur, wobble. | A leaf-based animation tree where children inherit the parent, named curves, and a *squircle* exponent. (Wiki `main` on 2026-11-03: some entries such as blur variants, glow and wobble may be newer than the last stable release; verify before relying on them.) | [animations][hypr-anim], [config options][hypr-opts] |
| **SketchyBar (macOS)** | Items (`left`, `right`, `center`, notch-aware `q`/`e`), `--set` properties (icon, label, background with `corner_radius`, `border`, `blur_radius`, padding, `y_offset`), `--animate <curve> <duration> --set ...` with curves `linear, quadratic, tanh, sin, exp, circ`, duration in 60 Hz frames, keyframes by repeating a property; `update_freq`, event `subscribe`, popups, `--clone`. | Command-style animate API with queued keyframes and cancel-on-set semantics. | [animations][sketchy-anim], [items][sketchy-items] |
| **Ubersicht (macOS)** | Widgets are `.jsx` modules exporting `command`, `refreshFrequency` (default 1000 ms), `render`, `className`; positioned absolutely; GPL-3.0; repo copyright 2019 and old tooling suggest low maintenance (inference). | `command` + `refreshFrequency` + `render` as a tiny widget contract. | [repo][uebersicht] |
| **Raycast** | Extensions are React/TypeScript commands launched in several modes (root search, alias, `launchCommand`, background refresh, deeplink, fallback); the command is unloaded from memory when done and has memory limits. (I could not confirm "Node.js, separate process, native components only" from the page I read.) | Command launch modes and strict unload. | [lifecycle][raycast] |

## 4. Answers to the specific questions

### 4.1 Tray icons without injection

How an app learns about tray icons: apps call `Shell_NotifyIcon`, which sends `WM_COPYDATA` to a window of class
`Shell_TrayWnd` found with `FindWindow` (Zebar's [README][zebar-spy-readme] states this flow; ManagedShell's
[`TrayService.cs`][ms-tray] implements the receiving side). The icon's `hWnd`, `uID`, `uCallbackMessage` and version
come with it, and clicks are returned to that callback.

| Approach | Gets | Clicks | Win11 24H2+ status | Used by | Tag |
|---|---|---|---|---|---|
| Hook DLL in Explorer (`WH_CALLWNDPROC` / `WH_GETMESSAGE`, WndProc subclass) | Every NIM message with raw `HICON`, state flags, plus `ITaskbarList3` overlay messages | Callback message | Works; ships in Seelen and YASB; both fixed crash/security issues inside Explorer | Seelen, YASB v2, trailist | `INJECT` |
| Host/spy `Shell_TrayWnd` window + `TaskbarCreated` broadcast + forwarding | NIM adds/updates/deletes **after** apps re-register; tooltips and an icon handle per message (Zebar's [`IconEventData`][zebar-spy] keeps `icon_handle`; how to copy it safely across processes was not verified) | Callback message via `SendNotifyMessage` | Works in the sources; initial enumeration impossible ("expected on W11"); native taskbar breakage reported on 25H2 ([#301][zebar-301]); misses apps that don't handle `TaskbarCreated` (Seelen: "Some windows fail to re-add their icons ... implementation error on their side", [`util.rs`][slu-util]) | ManagedShell, RetroBar, Cairo, Zebar, YASB legacy | `OK` |
| `TB_GETBUTTON` + `VirtualAllocEx` in Explorer | Existing icons on the Win10 toolbar tray | n/a | Does not work: Win11 tray is XAML ([`tray_spy.rs`][zebar-spy] `TbButtonItem` doc comment) | ManagedShell `ExplorerTrayService`, Zebar | Excluded (cross-process memory writes) |
| UI Automation on the taskbar | Button names (tooltips) and bounding rectangles. A web-search summary of the [trailist][trailist] repo says its `win/uia.rs` finds tray icons as Buttons with `AutomationId=NotifyItemIcon` (file itself not read). trailist's README says its DLL path is primary and "the previous UI Automation path remains" as fallback. | `Invoke` / `ExpandCollapse` executed by Explorer | **Unverified** on a hidden or off-screen taskbar; no `HICON`, no callback, no pin state | Glaze today; trailist (fallback only) | `OK` |
| Read Explorer memory (`NotifyIconItem` structs) | Icon list | Posts callback messages | Offsets "verified on Windows 11 10.0.26300.9711" and must be re-verified per major build ([TrayOrganizer README][trayorg]) | TrayOrganizer | Excluded |
| `HKCU\Control Panel\NotifyIconSettings` | Per-icon key: `ExecutablePath`, `InitialTooltip`, `UID`, `IconGuid`, `IsPromoted`; plus `UIOrderList` | None | Exists on Win11; Win10 has none ([Greenshot source][greenshot]) | Seelen, Greenshot | `UNDOC` |

**Recommendation for Windows 11 24H2+:** run the host-window approach as the primary event source, use
`NotifyIconSettings` for names/paths/pin/order, use UIA as a degraded fallback (and always keep a "open native
overflow" button). Caveats to design around:

- The host window must exist **only while the tray module is enabled**, and be destroyed on exit with a
  `TaskbarCreated` broadcast (ManagedShell does this in `Dispose`).
- Test Explorer restart and native taskbar menus on the current build before shipping (that is what #301 reports).
- Elevated apps: Seelen 2.8.9's changelog lists "tray icons of apps running as another user or elevated (e.g.
  Sunshine) not shown" as fixed ([changelog][slu-changelog]); how it was fixed is not stated. Expect the same gap for
  a non-elevated host window (UIPI blocks messages from lower to higher integrity; inference).
- No overlay badges or progress for the dock without a hook (section 2.5).
- Windows' own system icons (Wi-Fi, volume, battery) are not tray messages any more; build modules from APIs (section 4.6).

### 4.2 Reading notifications

- **Documented API:** `UserNotificationListener` (`RequestAccessAsync`, `GetNotificationsAsync(Toast)`,
  `NotificationChanged`, `RemoveNotification`, `ClearNotifications`) in `Windows.UI.Notifications.Management`
  ([class][ms-unl-class], [guide][ms-notif-listener]). The guide says to "add the User Notification Listener
  capability to your app manifest", the user must grant access in Windows Settings, and revoked access makes calls
  return empty lists. The capability is in the **general-use** table ([capability list][ms-caps]). A background
  trigger (`UserNotificationChangedTrigger`) is documented for app lifecycles, which implies identity.
- **Does it require package identity?** The docs never say "required"; they assume a manifest. Observed behavior:
  Seelen's MSIX manifest declares `uap3:Capability Name="userNotificationListener"` ([manifest][slu-appx]); its code
  comments "this only works on MSIX/APPX/UWP builds" and uses a polling thread when `NotificationChanged` registration
  fails ([`application.rs`][slu-notif] `init`). YASB (unpackaged) calls the same APIs, falls back to WNF
  subscriptions, and notes that `ClearNotifications()` fails without identity ([source][yasb-notif]). **Neither
  source states whether `RequestAccessAsync` returns `Allowed` for an unpackaged exe.** Verdict: reading may work
  unpackaged; reliable change events and clearing want identity. **Verify by spike.**
- **Sparse package:** Microsoft's "packaging with external location" lists "toast and push notifications,
  background tasks..." as unlocked ([Learn, 2026-10-03][ms-sparse]). Requirements from that page: an identity package
  manifest with `uap10:AllowExternalContent`, `runFullTrust`, `TargetDeviceFamily MinVersion 10.0.19041.0`,
  signed with a trusted certificate (self-signed needs the public cert in `Trusted People`; production needs a
  CA/Trusted Signing cert), registered with `Add-AppxPackage -ExternalLocation` or
  `PackageManager.AddPackageByUriAsync`, and a `<msix>` element in the exe's side-by-side manifest. Whether
  `userNotificationListener` is honored in a sparse manifest is **not stated** there.
- **What others do beyond the listener:** Seelen reads `wpndatabase.db` (undocumented SQLite, WAL mode, schema stable
  "since Windows 10 1607") as another source ([`wpn_service.rs`][slu-wpn]); YASB subscribes to WNF state names
  `WNF_SHEL_NOTIFICATION_TOTAL` / `WNF_SHEL_NOTIFICATIONS` for change signals ([`undocumented.py`][yasb-undoc]).
  Both `UNDOC`.
- **Do Not Disturb:** read with `ToastNotificationManagerForUser.NotificationMode`
  ([enum][ms-notifmode], Unrestricted / PriorityOnly / AlarmsOnly) and `FocusSessionManager`
  ([class][ms-focus], `IsFocusActive`, `TryStartFocusSession`, `DeactivateFocus`). Seelen **sets** DND through an
  undocumented quiet-hours profile and falls back to opening `ms-settings:notifications` if it fails
  ([`application.rs`][slu-notif]).

### 4.3 Hiding the native taskbar reliably

Mechanism, from the two codebases that do it:

1. Find all taskbars: enumerate top-level windows with class `Shell_TrayWnd` or `Shell_SecondaryTrayWnd` (and an
   empty title in Seelen's filter) ([`get_taskbars_handles`][slu-hide]).
2. For each, `SHAppBarMessage(ABM_SETSTATE, ABS_AUTOHIDE)` then `ShowWindow(SW_HIDE)`. `ABM_SETSTATE` is documented as
   "Sets the state of the appbar's autohide and always-on-top attributes" ([Learn][ms-abm]); aiming it at Explorer's
   tray window is the common but **undocumented usage**. Glaze already does this ([`docs/bars.md`](../bars.md)).
3. Repeat: Seelen loops 10 times with 50 ms sleep ([`hide_native_taskbar`][slu-hide]); ManagedShell checks
   `IsWindowVisible` every 100 ms and also hides the Start button window ([`ExplorerHelper`][ms-explorerhelper]).
4. Re-hide on events: Seelen's `SetWinEventHook` handler hides again on `EVENT_OBJECT_SHOW` / `EVENT_OBJECT_CREATE`
   when the window or its parent is a taskbar class ([`weg/hook.rs`][slu-weghook]). This catches Explorer restarts
   and monitor hot-plug (new `Shell_SecondaryTrayWnd`), plus `TaskbarCreated`.
5. Restore: Seelen delegates hide/restore to the separate service "so we can safely restore on shutdown even if the main
   app crashes" and only restores if it hid ([`weg/mod.rs`][slu-wegmod], [`shutdown.rs`][slu-hide]). ManagedShell
   remembers the startup `ABM_GETSTATE`. Glaze's journal is the same idea.
6. **Win-key / Start interplay:** the native Start, search, notification center, and volume/brightness flyouts are
   separate Explorer windows that still appear. Seelen hides the notification center/toast window of
   `ShellExperienceHost.exe` when it uncloaks and the `XamlExplorerHostIslandWindow` content windows
   ([`weg/hook.rs`][slu-weghook]) and ships its own apps menu / flyouts. ManagedShell synthesizes Win-key combos
   (`SendInput`) to open native UI ([`ShellHelper`][ms-shellhelper]). Where the native Start menu appears when
   its taskbar is hidden is **unverified** from these sources.
7. **Flash at login:** every tool waits for or reacts to the taskbar appearing (Seelen: `wait_for_native_shell`, up
   to 5 s, then hide within ~50 ms; Glaze: reacts to `TaskbarCreated`). None prevents the initial paint. Untested
   idea: a persisted autohide state in `HKCU\...\Explorer\StuckRects3` (a binary `Settings` value that
   community scripts edit, [example][stuckrects]) before the next sign-in; behavior on 24H2 is unverified, and it is an
   undocumented blob. Treat as an experiment, not a plan.
8. Multi-monitor: loop `Shell_SecondaryTrayWnd` with `FindWindowEx(..., prev, ...)` (ManagedShell comment: "if we have
   3+ monitors there may be multiple secondary TaskBars").

### 4.4 Fastest autostart at login

| Method | Mechanism | Speed | Used by |
|---|---|---|---|
| HKCU `Run` | Explorer reads it after the shell starts. Microsoft: "The system does not provide guarantees about how promptly the programs in the `Run` key are run" and "may choose to delay" them ([Learn][ms-run]). | Delayed by Explorer's startup serialization | YASB ([`cli.py`][yasb-cli]) |
| Startup folder | Same Explorer path as Run | Same delay (the Windows 11 delay threads are about the startup folder, [Q&A][ms-qa-startup]) | typical installers |
| `...\Explorer\Serialize\StartupDelayInMSec` (and `WaitForIdleState`) | Undocumented per-user registry values; setting both to 0 removes Explorer's delay. A Windhawk mod description says Explorer reads them at start and the delay "doesn't change delays configured by other systems, such as Task Scheduler" ([mod][wh-startup]). | Fixes Run/Startup, but it's a **global user tweak**, unsupported, and Windows updates reportedly delete the key | tweak scripts (e.g. [Atlas](https://github.com/Atlas-OS/Atlas/blob/main/playbook/Executables/AtlasModules/Scripts/Tweaks/qol/startup-shutdown/disable-startup-delay.psd1)) |
| Task Scheduler logon trigger | Started by Task Scheduler at logon, not by Explorer's Run/Startup processing (the Windhawk mod author's description; no Microsoft statement found). Seelen registers `\Seelen\Seelen UI Service` with `TASK_TRIGGER_LOGON`, `TASK_RUNLEVEL_HIGHEST`, priority 2, hidden, no battery restrictions ([`task_scheduler.rs`][slu-task]). Triggers also have a documented `Delay` ([Learn][ms-logon-delay]). | Likely the fastest supported option (inference: no timing measurements exist in any source read); Seelen's service waits for the shell itself | Seelen |

**Recommendation:** register a per-user logon task (`TASK_LOGON_INTERACTIVE_TOKEN`, normal priority class raised, no
delay) and wait for `Shell_TrayWnd` as Seelen does. Seelen uses the highest run level, which requires admin at install
time; **whether a limited-privilege logon task for the current user can be created without elevation is unverified
here** (spike it). Keep a Run-key fallback. Do not ship the `Serialize` tweak as default.

### 4.5 Custom desktop right-click menu

| Approach | How | Trade-offs | Tag | Evidence |
|---|---|---|---|---|
| Low-level mouse hook (`WH_MOUSE_LL`) | Callback on our own thread sees every click; check `WindowFromPoint` class (`SysListView32` inside `SHELLDLL_DefView`) and an empty-area hit test | Documented hook type ([`SetWindowsHookEx`][ms-hook]). That LL hooks run in the installing thread and need a message pump is standard but was **not confirmed from the page I read** (that page's pump warning is about 32-bit hooks on 64-bit Windows). Every click passes through us; security tools may flag it | `OK` | [`SetWindowsHookEx`][ms-hook] |
| Raw Input sink (`RIDEV_INPUTSINK`) | `RegisterRawInputDevices` on a message window; get right-button-up system-wide | Cheaper than a hook, cannot swallow the click so the native menu still opens (we'd have to close it or race it) | `OK` | Lively [`RawInputMsgWindow`][lively-raw] |
| Own desktop layer | Replace Explorer's icon surface with our own window at `HWND_BOTTOM`; render icons, show `IContextMenu` of the Desktop folder | Total control, but we own icons, drag/drop, rename, selection, DPI, accessibility. Cairo does it. | `OK` | [`Desktop.xaml.cs`][cairo-desktop], [`ShellFolderContextMenu`][ms-ctxmenu] |
| Show native menu ourselves | `IShellFolder::GetUIObjectOf` -> `IContextMenu` -> `TrackPopupMenu` | Keeps shell extensions (they load in **our** process); needs wrapper for owner-drawn menus | `OK` | ManagedShell `ShellContextMenu` |
| Add items via a shell extension | `IExplorerCommand` / `IContextMenu` handler DLL | Microsoft: "Apps extend the menu with IExplorerCommand + app identity", unpackaged apps can use sparse manifests; the old model "runs in-process in Explorer" ([blog, 2021-07-19][ms-ctx-blog]). The handler runs inside Explorer and other apps: treat as excluded unless the product decision changes. | `IDENTITY` + in-process | same |
| Win11 classic menu toggle | Create an empty default value at `HKCU\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32` (name from `Windows.UI.FileExplorer.ContextMenu`) | Undocumented; restores the old full menu everywhere; user-visible global change | `UNDOC` | [moudey/Shell `Globals.h`][nilesoft], scripts such as [winscript](https://github.com/flick9000/winscript/blob/main/docs/content/misc.md) |

**Recommendation:** Glaze already owns the wallpaper layer, so use Raw Input (to know *when* a right-click hit empty
desktop) or a short-lived LL hook, and show the menu in our own WebView2 menu window. If the menu should include native
entries, build them with `IContextMenu` for the Desktop folder. Never intercept by injection.

### 4.6 APIs

| Capability | API | Status | Source |
|---|---|---|---|
| Per-app volume | `IAudioSessionManager2` (`GetSessionEnumerator`, `ISimpleAudioVolume`) | Documented | [Learn][ms-sdk-asm2]; Seelen [`media/devices`][slu-audio] |
| Master volume / mute | `IAudioEndpointVolume` | Documented | `docs/bars.md` already uses it |
| Output device **switching** | `IPolicyConfig` (CLSID `870af99c-171d-4f9e-af0d-e63df40c2bc9`, IID `f8679f50-850a-41cf-9c72-430f290290c8`) | **Undocumented**: no Learn page exists in `sdk-api` for it; Seelen hand-declares the vtable | [`audio_policy_config.rs`][slu-policy] |
| Keyboard layout list / current | `GetKeyboardLayout(threadOfForegroundWindow)`, TSF `ITfInputProcessorProfileMgr::EnumProfiles` | Documented ([TSF mgr][ms-sdk-tsf]) | [`language/application.rs`][slu-lang] |
| Keyboard layout **switch** | `ITfInputProcessorProfileMgr::ActivateProfile` with `TF_IPPMF_FORPROCESS \| FORSESSION`; Seelen's commented older path used `WM_INPUTLANGCHANGEREQUEST` + `ActivateKeyboardLayout` | Documented APIs; behavior on other processes' threads is the fragile part | same file |
| Wi-Fi list / connect | `WlanGetAvailableNetworkList`, `WlanGetNetworkBssList`, `WlanScan`, `WlanConnect`; or WinRT `WiFiAdapter` (scan, `ConnectAsync`) | Documented. **24H2:** now requires precise-location consent; Wlan calls return `ERROR_ACCESS_DENIED`; `WiFiAdapter.RequestAccessAsync` returns `DeniedBySystem`; use `WlanConnectionProfileDetails.GetConnectedSsid` for the current SSID; `wiFiControl` capability also needs location consent | [Learn, 2025-05-30][ms-wifi-loc] |
| Bluetooth / Wi-Fi radio toggle | `Windows.Devices.Radios.Radio` (`GetRadiosAsync`, `RequestAccessAsync`, `SetStateAsync`) | Documented; "radio control may require explicit user consent"; policies may block | [Learn][ms-radio] |
| Bluetooth devices / battery | Device enumeration + `DEVPKEY` for battery (YASB uses `DEVPKEY_Bluetooth_Battery` `{104EA319-6EE2-4701-BD47-8DDBF425BBE5}, 2`) | Property key is not in public docs I found (**unverified**) | [`undocumented.py`][yasb-undoc] |
| Brightness | Monitor Configuration API (`GetMonitorBrightness`, DDC/CI) for externals; internal panels use IOCTL/WMI | Documented ([`GetMonitorBrightness`][ms-sdk-bright]) | [`brightness.rs`][slu-bright] |
| Night Light | CloudStore blob (Bond-serialized) under the user hive | **Undocumented** | [`nightlight/cloudstore.rs`][slu-night] |
| Focus / DND read | `FocusSessionManager`, `ToastNotificationManagerForUser.NotificationMode` | Documented | [Learn][ms-focus] |
| DND write | Quiet-hours profile (undocumented); `FocusSessionManager.TryStartFocusSession` starts a focus *session* (a related, different feature) | Undocumented / partial | [`application.rs`][slu-notif] |
| Media (now playing) | `GlobalSystemMediaTransportControlsSession` | Documented (Glaze uses it) | `docs/bars.md` |
| Window list, cloak state | `SetWinEventHook`, `DWMWA_CLOAKED` | Documented | [`hook.rs`][slu-hookrs] |

### 4.7 Making the UI appear instantly

What the sources show:

- **Share one browser process.** One `CoreWebView2Environment` per identical configuration; same user data folder
  *and* same options, otherwise creation fails ([process model][ms-wv2-process]). Microsoft: "Multiple apps can share
  a browser process by using the identical user data folder and `CoreWebView2EnvironmentOptions`" and "Share
  WebView2 environments" ([perf][ms-wv2-perf]). Seelen derives four data folders from the flag set (`gpu`,
  `gpu-unstable`, `no-gpu`, `no-gpu-unstable`) and passes the same `additional_browser_args` to all widgets
  ([`webview.rs`][slu-webview]). Caution (Seelen's own comment): `--process-per-site` saves RAM but "if a widget
  crashes it will crash all widgets with the same loader".
- **Avoid WebView2 for first paint.** Microsoft: "To render splash screens or simple dialogs, use lightweight XAML or
  Win32 screens instead of WebView2". Keep the first frame native (a solid/blurred bar background drawn by the host)
  then reveal the page. Prefer "static HTML initially" over a heavy SPA ([perf][ms-wv2-perf]).
- **Reduce startup work.** Lazy creation for flyouts (Seelen `lazy` widgets, `Popup` preset triggered on demand);
  `MemoryUsageTargetLevel = Low` and `TrySuspendAsync()` for hidden webviews (Learn).
- **Pre-rendered snapshots:** none of the studied tools do it (searched Seelen's tree for `snapshot|prerender|preload`;
  only a splash image for settings). A cheap equivalent to build: persist the last rendered module values to disk
  and paint them from the host before the data providers answer.
- **Start early:** Task Scheduler logon trigger (4.4) + `wait_for_native_shell` + early taskbar hide (4.3).
- **No-input dock:** Seelen's click-through full-area webview means the page is always loaded and only toggles hit-testing.

## 5. Prioritized catalogue for Glaze

Priority: **P0** next to ship, **P1** soon, **P2** later. "Src" = where the technique is shown. Everything marked
`OK` or `UNDOC` is possible without injection.

### 5.1 Layout presets and global look (top bar and dock)

| Option | Pri | Feasibility | Src |
|---|---|---|---|
| Presets: **floating** (margins, radius), **islands** (rail + one pill per group), **full-width** Windows-style, **macOS-style** (translucent bar, centered dock) | P0 | `OK` | YASB adaptive style [`Styling.md`][yasb-styling]; Seelen [`bubbles`][slu-bubbles]; Windhawk DockLike/Squircle/Bubbles [guide][wh-guide] |
| Geometry: size, margin, padding, item spacing, position (4 sides for dock; top/bottom for bar), `dimensions: auto` (hug content) | P0 | `OK` | Seelen [settings][slu-settings]; YASB [`Configuration.md`][yasb-cfg] |
| Corner radius + squircle exponent (`rounding_power`) | P1 | `OK` (CSS `corner-shape`/clip-path or SVG mask; to be tested in WebView2) | Hyprland [options][hypr-opts] |
| Per-monitor overrides (enable, position, size, preset) | P0 | `OK` | Seelen `monitorsV3`; CmdPal Dock per-display position |
| Theme variables as user-tunable CSS vars with typed controls (color, number slider/dropdown, length, string, URL) | P0 | `OK` | Seelen [theme guide][slu-theme-doc] |
| Windows accent as CSS variables (7 shades) + light/dark auto | P0 | `OK` | Seelen `--system-accent-*` |
| Wallpaper-derived palette ("dynamic accent") | P1 | `OK` (Glaze owns the wallpaper pixels). Seelen exposes `useAccentColor` but its exact semantics are unverified | [`settings/mod.rs`][slu-settings] |
| Dynamic states: bar/dock look changes when a window is maximized / Start open / fullscreen app | P2 | `OK` via win events and `IAppVisibility` | TranslucentTB [`taskbarattributeworker.cpp`][ttb] |
| Performance mode that disables animation/blur by power state | P0 | `OK` | Seelen `performanceMode` |

### 5.2 Backgrounds, blur, glow, borders

| Option | Pri | Feasibility | Src |
|---|---|---|---|
| Solid, gradient, image background; brightness, blur, fit | P0 | `OK` | CmdPal Dock appearance [Learn][ms-cmdpal-dock]; Windhawk `LinearGradientBrush`/`ImageBrush` |
| Material: transparent / acrylic / mica | P0 | `OK` for system backdrop (`DWMWA_SYSTEMBACKDROP_TYPE`, Win11 22H2+) on our windows; `UNDOC` for `SetWindowCompositionAttribute` accent policy (YASB, TranslucentTB) | [Learn][ms-dwm]; [`undocumented.py`][yasb-undoc] |
| Blur of **our own wallpaper** behind bars (sample the wallpaper texture, tint, noise) | P1 | `OK`, bold idea: only Glaze can do it without hooks, and it is not subject to "blur only in rectangles" limits (Nexus) | Hyprland blur params (`size`, `passes`, `noise`, `vibrancy`, `contrast`, `brightness`) |
| Tint opacity, luminosity, saturation, noise opacity/density | P1 | `OK` | Windhawk `WindhawkBlur` parameters |
| Glow / shadow (range, power, color; separate for hovered/active) | P1 | `OK` (CSS) | Hyprland shadow/glow; SketchyBar shadow |
| Border: width, color, gradient, per-state | P1 | `OK` | YASB `-qproperty-borderwidth`; GlazeWM focused/other borders |
| Edge-curving corners where islands meet the screen edge | P2 | `OK` | YASB `edgeradius` |

### 5.3 Animations and interaction

| Option | Pri | Feasibility | Src |
|---|---|---|---|
| Open/close: slide, fade, pop-in, scale; duration + curve | P0 | `OK` | YASB bar `animation {type: slide/fade, duration}`; Hyprland styles `slide/popin/gnomed` |
| Curve picker: named presets plus custom cubic-bezier and spring (mass/stiffness/damping) | P1 | `OK` (CSS `cubic-bezier()` and `linear()`/JS spring) | Hyprland [animations][hypr-anim]; SketchyBar curves |
| Animation tree with inheritance (global -> bars -> dock -> hover) and per-leaf enable | P1 | `OK` | Hyprland animation tree |
| Dock hover: magnification (zoom), neighbors falloff, bounce on launch, attention pulse | P0 | `OK` | Nexus [effects][nexus]; RocketDock zoom; Seelen `revealOnAttention` |
| Indicator styles for running/active/attention: dot, bar, glow, count badge; per-state colors | P0 | `OK` | Windhawk running-indicator states [guide][wh-guide]; Seelen `showInstanceCounter` |
| Hover preview thumbnails with delay | P1 | `OK` (DWM thumbnails) | YASB `preview.delay`; Seelen `weg-preview` |
| Click actions per module: left / middle / right / wheel -> action or `exec` | P0 | `OK` | YASB `callbacks`; Seelen `onClick/onWheelUp/Down`; Seelen `middleClickAction` |
| Auto-hide with show/hide delays and edge hotspot | P0 | `OK` | Seelen `hideMode`, `delayToShow/Hide`; CmdPal Dock auto-hide |
| Reduce motion toggle bound to Windows setting | P1 | `OK` (`SPI_GETCLIENTAREAANIMATION`, [Learn][ms-spi]) | Seelen performance mode |
| Drag-reorder dock items and bar modules | P1 | `OK` | YASB taskbar; CmdPal Dock edit mode |

### 5.4 Top bar modules

| Module | Pri | Feasibility | Notes / src |
|---|---|---|---|
| Clock/calendar with custom format and extra time zones | P0 | `OK` | CmdPal Dock custom clocks |
| Workspaces / virtual desktops | P0 | `UNDOC` for switching APIs (registry for count; keyboard shortcut for switching, as Glaze does) | Seelen `workspaces`; `docs/bars.md` |
| Focused app name/icon, global menu | P1 | `OK` | Seelen `FocusedApp` scope |
| Tray (pinned + overflow popup grid) | P0 | `OK` via host window (4.1), `UNDOC` for registry | YASB popup options; Seelen pin/order |
| Network, volume, battery, Bluetooth, brightness | P0 | `OK` (Wi-Fi list needs location consent) | section 4.6 |
| Notifications badge + DND state | P1 | `IDENTITY` for reliable events; `UNDOC` for DND write | 4.2 |
| Keyboard layout / IME | P1 | `OK` | Seelen `Keyboard` scope |
| Media now playing + waveform | P1 | `OK` | Seelen `Waveform`; Wallpaper Engine 128-float feed |
| System stats (CPU/GPU/RAM/disk/net) | P1 | `OK` | Seelen scopes; CmdPal perf bands |
| Custom/scripted module (command + interval) | P1 | `OK` | YASB `custom`; Ubersicht `command`+`refreshFrequency` |
| Module plugin format: declarative `scopes` + JS template/tooltip/badge/onClick | P0 | `OK` | Seelen [toolbar plugins][slu-tb-plugins] |

### 5.5 Dock / taskbar

| Option | Pri | Feasibility | Src |
|---|---|---|---|
| Pinned + running items, grouping by AUMID, split windows, instance counter, titles | P0 | `OK` | `docs/bars.md`; Seelen `splitWindows`, `showInstanceCounter`, `showWindowTitle` |
| Unpinned-items visibility: all / on monitor / hidden | P1 | `OK` | Seelen `temporalItemsVisibility`; 2.8.9 changelog |
| Icon size, spacing, magnification, position (4 sides) | P0 | `OK` | Seelen settings; Nexus |
| Overlay badges and progress on icons | P1 | `INJECT` for `ITaskbarList3` messages (section 2.5); partial alternatives: window flashing via `HSHELL_FLASH` shell hook, notification counts from 4.2 | Seelen `TBM_SET_OVERLAY_*` |
| Pin/unpin, recent items, jump-list-like menus | P1 | `OK` (shell `IContextMenu` for the lnk) | ManagedShell |
| Show-desktop, trash, folder stacks | P2 | `OK` | Seelen `weg_show_desktop`, trash module; Nexus stacks |
| Multi-dock (several docks/shelves) | P2 | `OK` | Nexus, Winstep |

### 5.6 Flyouts

| Flyout | Pri | Feasibility | Notes / src |
|---|---|---|---|
| Launcher / Start replacement (search apps, Windows Settings pages) | P0 | `OK` (index via Start-menu folders, AppInfo) | Seelen apps menu (2.8.9 added Windows Settings search); CmdPal; Flow Launcher |
| Quick settings (Wi-Fi, Bluetooth, brightness, volume, DND, night light, power mode) | P0 | `OK`; night light/DND write `UNDOC`; Wi-Fi list needs location consent | Seelen quick-settings; YASB control_center |
| Network (Wi-Fi list, connect, hotspot) | P1 | `OK` (consent) | Seelen network popup |
| Audio mixer (per-app sliders, device switch) | P1 | `OK`; device switch `UNDOC` | Seelen `media-popup` |
| Language/IME picker | P2 | `OK` | Seelen `keyboard-selector` |
| Calendar | P1 | `OK` | Seelen calendar popup; CmdPal clocks |
| Notification center | P1 | `IDENTITY` (events) / `OK` (polling) | 4.2 |
| Power menu (lock/sleep/restart/shutdown) | P0 | `OK` | Seelen, YASB |
| Tray overflow | P0 | `OK` | 4.1 |
| Hiding native duplicates (notification center, volume island) | P1 | `UNDOC` (window class names change per build) | Seelen [`weg/hook.rs`][slu-weghook] |

### 5.7 Widgets and wallpaper

| Option | Pri | Feasibility | Src |
|---|---|---|---|
| Widget presets: `Desktop` (behind windows, remembers position), `Overlay`, `Popup`; instances `Single/Multiple/ReplicaByMonitor` | P0 | `OK` | Seelen [widget guide][slu-widget-doc] |
| Zebar-style placement: `anchor`, offsets, size in px/%/, `monitorSelection`, `dockToEdge` | P0 | `OK` | Zebar [starter][zebar-starter] |
| Property schema -> generated settings UI (slider, color, bool, combo, text, file, directory) | P0 | `OK` | Wallpaper Engine [properties][we-props] |
| Widget capability prompts (shell commands allowlist) | P1 | `OK` | Zebar `privileges.shellCommands`; Seelen 2.8.8 "ask permission" |
| Audio-reactive feed (128 floats, ~30 Hz) | P1 | `OK` (loopback capture) | WE [audio][we-audio]; YASB audio_visualizer |
| Wallpaper properties & multi-monitor behavior, pause on cover (coverage threshold), pause on battery/game | P0 | `OK` | Seelen wall settings `coveragePauseThreshold`; `suspendOnGameMode` |
| Wallpaper input forwarding (mouse via Raw Input) | P1 | `OK` | Lively |
| Desktop right-click custom menu | P1 | `OK` (4.5) | |
| Raised-desktop compatibility (own layered child between `DefView` and `WorkerW`) | P0 | `OK` | [Lively note][lively-core] |

### 5.8 Windows-level tweaks

| Option | Pri | Feasibility | Src |
|---|---|---|---|
| Window border color, caption color, corner preference, backdrop, dark mode | done | `OK` (Win11) | `docs/windows-styling.md`; GlazeWM [`native_window.rs`][glaze-dwm]; [Learn][ms-dwm] |
| Per-state focused/unfocused window border colors | P1 | `OK` (`DWMWA_BORDER_COLOR` cross-process) | GlazeWM `window_effects` |
| Icon packs (AUMID / path / extension -> icon) applied **to our UI** | P0 | `OK` | Seelen [`icon_pack.rs`][slu-iconpack] |
| System-wide icon replacement in Explorer | n/a | `INJECT` | excluded |
| Cursor themes | P1 | `OK`: `SystemParametersInfo(SPI_SETCURSORS)` after writing `HKCU\Control Panel\Cursors`, or `SetSystemCursor` | [Learn][ms-spi], [`SetSystemCursor`][ms-setsyscursor] (Glaze's `crates/tweaks` may already cover; check) |
| Wallpaper set via system | P2 | `OK` (`IDesktopWallpaper`) | [Learn][ms-idesktopwp] |
| Classic Win11 context menu toggle | P2 | `UNDOC`, global side effect, reversible via registry key deletion | [Nilesoft][nilesoft] |
| Taskbar alignment (`TaskbarAl`), hide, autohide | done | `OK`/`UNDOC` | `docs/bars.md` |
| Taskbar transparency/acrylic of the native taskbar | n/a | `UNDOC` (`SetWindowCompositionAttribute` on foreign `Shell_TrayWnd`; TranslucentTB does it without injecting on Win11) but we hide that taskbar | [`taskbarattributeworker.cpp`][ttb] |
| Start-menu / taskbar XAML restyling | n/a | `INJECT` | Windhawk, StartAllBack |
| Startup tweaks (`StartupDelayInMSec`) | P2 | `UNDOC`, optional | 4.4 |

## 6. "What to steal" index (by project)

| Project | Take this |
|---|---|
| Seelen UI | Resource/theme/plugin file format; typed settings tree with per-monitor overrides; `scopes`-based plugin API; accent variables; performance mode; watchdog service for taskbar restore; Task Scheduler logon startup; `NotifyIconSettings` for pin/order; per-flag WebView2 data folders |
| YASB | Adaptive island shape (rail/islands/edge radius); `dimensions: auto`; `callbacks`; theme manager + install protocol; popup tray grid |
| Zebar | `zpack` presets (`anchor`, `dockToEdge`, `monitorSelection`); shell-command allowlist; per-widget caching |
| GlazeWM | `window_effects` accent borders via DWM |
| Rainmeter | `UpdateDivider`, `IfCondition`, `MeterStyle` inheritance, `Substitute` |
| ManagedShell/Cairo/RetroBar | Tray host with forwarding; `ExplorerHelper` hide/restore; native `IContextMenu` helpers; Win-key combos |
| Open-Shell | `SC_TASKLIST` Progman trick (test) and a cautionary tale |
| Windhawk | Styling checklist (states, indicators, blur parameters) |
| TranslucentTB | Dynamic appearance states |
| Nexus/RocketDock/Winstep | Per-event dock effect slots; multi-dock; skin import ideas |
| PowerToys CmdPal | Dock regions (start/center/end), edit mode, band concept, background image options |
| Lively / Wallpaper Engine | Raw-input forwarding; property schema; audio feed contract; raised-desktop handling |
| Waybar | Live CSS reload; light/dark stylesheets; `exclusive` / `passthrough` |
| eww | Window declaration (struts, stacking, monitor) separate from content |
| Astal/AGS | One library per system service; reactive bindings |
| Hyprland | Animation tree, named curves, spring curves, squircle exponent |
| SketchyBar | `--animate` keyframe/cancel semantics; `clone`/`default` styles |
| Ubersicht | `command` + `refreshFrequency` + `render` minimal widget contract |
| Raycast | Strict unload and launch modes for rarely used flyouts |

## 7. Not verified or open

- Whether `UserNotificationListener.RequestAccessAsync` returns `Allowed` for an unpackaged process on current
  builds, and whether `userNotificationListener` works from a sparse-package manifest. Spike both.
- Whether UIA works for tray invocation while the taskbar is hidden (`SW_HIDE`) or off-screen on 24H2+/26300.
- Whether a limited (non-admin) per-user logon task can be created programmatically on a standard install.
- The earlier tray mechanism in Seelen before `sluhk.dll` (changelog wording only).
- StartAllBack's mechanism (closed source, not documented on its site).
- Seelen's `useAccentColor` semantics, Seelen/Windows internals for media (SMTC presumed), power and app icons: not
  inspected.
- Where the native Start menu appears when its taskbar is hidden.
- The format of `IconSnapShot` in `NotifyIconSettings` (Seelen left that code commented out).
- Flow Launcher plugin/theme docs and the KDE panel/data-source details (fetch returned no usable body).
- Whether `StuckRects3` pre-seeding avoids the login flash on 24H2.

<!-- Link references -->

[slu-hook]: https://github.com/eythaann/Seelen-UI/blob/master/src/hook_dll/src/lib.rs
[slu-loader]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/system_tray/application/tray_hook_loader.rs
[slu-tray-mod]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/system_tray/application/mod.rs
[slu-tray-icon]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/system_tray/application/tray_icon.rs
[slu-util]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/system_tray/application/util.rs
[slu-notif]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/notifications/application.rs
[slu-wpn]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/notifications/wpn_service.rs
[slu-focus]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/focus_assist/application.rs
[slu-net]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/network/application.rs
[slu-wifi]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/radios/wifi/adapter.rs
[slu-audio]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/media/devices/application.rs
[slu-policy]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/windows_api/undocumented/audio_policy_config.rs
[slu-lang]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/system_settings/language/application.rs
[slu-bright]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/windows_api/monitor/brightness.rs
[slu-night]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/modules/system_settings/nightlight/cloudstore.rs
[slu-hookrs]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/hook.rs
[slu-layered]: https://github.com/eythaann/Seelen-UI/blob/master/libs/ui/react/utils/layered.ts
[slu-theme-doc]: https://github.com/eythaann/Seelen-UI/blob/master/documentation/theme-guidelines.md
[slu-res-doc]: https://github.com/eythaann/Seelen-UI/blob/master/documentation/resource-guidelines.md
[slu-widget-doc]: https://github.com/eythaann/Seelen-UI/blob/master/documentation/widget-guidelines.md
[slu-jsapi-doc]: https://github.com/eythaann/Seelen-UI/blob/master/documentation/widget-js-api.md
[slu-tb-plugins]: https://github.com/eythaann/Seelen-UI/blob/master/documentation/toolbar-plugins.md
[slu-dock-plugins]: https://github.com/eythaann/Seelen-UI/blob/master/documentation/dock-plugins.md
[slu-bubbles]: https://github.com/eythaann/Seelen-UI/blob/master/src/static/themes/bubbles/styles/toolbar.css
[slu-themes-default]: https://github.com/eythaann/Seelen-UI/tree/master/src/static/themes/default/styles
[slu-iconpack]: https://github.com/eythaann/Seelen-UI/blob/master/libs/core/src/state/icon_pack.rs
[slu-settings]: https://github.com/eythaann/Seelen-UI/blob/master/libs/core/src/state/settings/mod.rs
[slu-agents]: https://github.com/eythaann/Seelen-UI/blob/master/AGENTS.md
[slu-svc-main]: https://github.com/eythaann/Seelen-UI/blob/master/src/service/main.rs
[slu-hotkeys]: https://github.com/eythaann/Seelen-UI/blob/master/src/service/hotkeys.rs
[slu-loader-rs]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/widgets/loader.rs
[slu-hide]: https://github.com/eythaann/Seelen-UI/blob/master/src/service/shutdown.rs
[slu-weghook]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/widgets/weg/hook.rs
[slu-wegmod]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/widgets/weg/mod.rs
[slu-task]: https://github.com/eythaann/Seelen-UI/blob/master/src/service/task_scheduler.rs
[slu-wait]: https://github.com/eythaann/Seelen-UI/blob/master/src/service/windows_api/mod.rs
[slu-webview]: https://github.com/eythaann/Seelen-UI/blob/master/src/background/widgets/webview.rs
[slu-changelog]: https://github.com/eythaann/Seelen-UI/blob/master/changelog.md
[slu-appx]: https://github.com/eythaann/Seelen-UI/blob/master/src/templates/AppxManifest.xml
[yasb-rel]: https://github.com/amnweb/yasb/releases
[yasb-cli]: https://github.com/amnweb/yasb/blob/main/src/cli.py
[yasb-reg]: https://github.com/amnweb/yasb/blob/main/src/core/utils/win32/registry_keys.py
[yasb-styling]: https://github.com/amnweb/yasb/blob/main/docs/Styling.md
[yasb-cfg]: https://github.com/amnweb/yasb/blob/main/docs/Configuration.md
[yasb-taskbar]: https://github.com/amnweb/yasb/blob/main/docs/widgets/(Widget)-Taskbar.md
[yasb-widgets]: https://github.com/amnweb/yasb/tree/main/src/core/validation/widgets/yasb
[yasb-systray-doc]: https://github.com/amnweb/yasb/blob/main/docs/widgets/(Widget)-Systray.md
[yasb-hook]: https://github.com/amnweb/yasb/blob/main/src/core/widgets/services/systray/hook/trayhook.cpp
[yasb-hookpy]: https://github.com/amnweb/yasb/blob/main/src/core/widgets/services/systray/systray_hook.py
[yasb-monitor]: https://github.com/amnweb/yasb/blob/main/src/core/widgets/services/systray/systray_monitor.py
[yasb-notif]: https://github.com/amnweb/yasb/blob/main/src/core/widgets/services/notifications/windows_notification.py
[yasb-undoc]: https://github.com/amnweb/yasb/blob/main/src/core/utils/win32/undocumented.py
[zebar-spy]: https://github.com/glzr-io/zebar/blob/main/crates/systray-util/src/tray_spy.rs
[zebar-spy-readme]: https://github.com/glzr-io/zebar/blob/main/crates/systray-util/README.md
[zebar-301]: https://github.com/glzr-io/zebar/issues/301
[zebar-287]: https://github.com/glzr-io/zebar/issues/287
[zebar-schema]: https://github.com/glzr-io/zebar/blob/main/resources/zpack-schema.json
[zebar-starter]: https://github.com/glzr-io/zebar/blob/main/resources/starter/zpack.json
[zebar-providers]: https://github.com/glzr-io/zebar/tree/main/packages/desktop/src/providers
[glaze-config]: https://github.com/glzr-io/glazewm/blob/main/resources/assets/sample-config.yaml
[glaze-dwm]: https://github.com/glzr-io/glazewm/blob/main/packages/wm-platform/src/platform_impl/windows/native_window.rs
[rm-measures]: https://docs.rainmeter.net/manual/measures/
[rm-meters]: https://docs.rainmeter.net/manual/meters/
[rm-skins]: https://docs.rainmeter.net/manual/skins/
[rm-plugin]: https://docs.rainmeter.net/developers/plugin/cpp/
[ms-readme]: https://github.com/cairoshell/ManagedShell/blob/master/README.md
[ms-tray]: https://github.com/cairoshell/ManagedShell/blob/master/src/ManagedShell.WindowsTray/TrayService.cs
[ms-notifyicon]: https://github.com/cairoshell/ManagedShell/blob/master/src/ManagedShell.WindowsTray/NotifyIcon.cs
[ms-explorertray]: https://github.com/cairoshell/ManagedShell/blob/master/src/ManagedShell.WindowsTray/ExplorerTrayService.cs
[ms-sso]: https://github.com/cairoshell/ManagedShell/blob/master/src/ManagedShell.WindowsTray/ShellServiceObject.cs
[ms-explorerhelper]: https://github.com/cairoshell/ManagedShell/blob/master/src/ManagedShell.AppBar/ExplorerHelper.cs
[ms-tasks]: https://github.com/cairoshell/ManagedShell/blob/master/src/ManagedShell.WindowsTasks/TasksService.cs
[ms-shellhelper]: https://github.com/cairoshell/ManagedShell/blob/master/src/ManagedShell.Common/Helpers/ShellHelper.cs
[ms-immersive]: https://github.com/cairoshell/ManagedShell/blob/master/src/ManagedShell.UWPInterop/ImmersiveShellHelper.cs
[ms-ctxmenu]: https://github.com/cairoshell/ManagedShell/blob/master/src/ManagedShell.ShellFolders/ShellFolderContextMenu.cs
[cairo-desktop]: https://github.com/cairoshell/cairoshell/blob/master/Cairo%20Desktop/CairoDesktop.DynamicDesktop/Desktop.xaml.cs
[retrobar-902]: https://github.com/dremin/RetroBar/issues/902
[retrobar-1253]: https://github.com/dremin/RetroBar/issues/1253
[os-readme]: https://github.com/Open-Shell/Open-Shell-Menu/blob/master/README.md
[os-menu]: https://github.com/Open-Shell/Open-Shell-Menu/blob/master/Src/StartMenu/StartMenuDLL/StartMenuDLL.cpp
[wh-readme]: https://github.com/ramensoftware/windhawk/blob/main/README.md
[wh-styler]: https://github.com/ramensoftware/windhawk-mods/blob/main/mods/windows-11-taskbar-styler.wh.cpp
[wh-guide]: https://github.com/ramensoftware/windows-11-taskbar-styling-guide/blob/main/README.md
[wh-startup]: https://github.com/ramensoftware/windhawk-mods/blob/main/mods/startup-app-delay-fix.wh.cpp
[ep-main]: https://github.com/valinet/ExplorerPatcher/blob/master/ExplorerPatcher/dllmain.c
[ep-readme]: https://github.com/valinet/ExplorerPatcher/blob/master/README.md
[sab]: https://www.startallback.com/
[ttb]: https://github.com/TranslucentTB/TranslucentTB/blob/release/TranslucentTB/taskbar/taskbarattributeworker.cpp
[nexus]: https://www.winstep.net/nexus.asp
[xtreme]: https://www.winstep.net/xtreme.asp
[rocketdock]: https://punklabs.com/rocketdock
[lively-wiki]: https://github.com/rocksdanister/lively/wiki/Wallpaper-Types
[lively-core]: https://github.com/lively-community/lively/blob/HEAD/src/Lively/Lively/Core/WinDesktopCore.cs
[lively-raw]: https://github.com/lively-community/lively/blob/HEAD/src/Lively/Lively/Views/WindowMsg/RawInputMsgWindow.xaml.cs
[we-props]: https://docs.wallpaperengine.io/en/web/customization/properties.html
[we-audio]: https://docs.wallpaperengine.io/en/web/audio/visualizer.html
[waybar-cfg]: https://github.com/Alexays/Waybar/wiki/Configuration
[waybar-style]: https://github.com/Alexays/Waybar/wiki/Styling
[polybar]: https://github.com/polybar/polybar
[eww-cfg]: https://github.com/elkowar/eww/blob/master/docs/src/configuration.md
[astal]: https://github.com/Aylur/astal
[kde-widget]: https://develop.kde.org/docs/plasma/widget/
[kvantum]: https://github.com/tsujan/Kvantum
[hypr-anim]: https://github.com/hyprwm/hyprland-wiki/blob/main/content/configuring/core/animations.md
[hypr-opts]: https://github.com/hyprwm/hyprland-wiki/blob/main/content/configuring/core/config-options.md
[sketchy-anim]: https://felixkratz.github.io/SketchyBar/config/animations
[sketchy-items]: https://felixkratz.github.io/SketchyBar/config/items
[uebersicht]: https://github.com/felixhageloh/uebersicht
[raycast]: https://developers.raycast.com/information/lifecycle
[trailist]: https://github.com/nopnop9090/trailist
[trayorg]: https://github.com/Swirlxx756/TrayOrganizer
[greenshot]: https://github.com/greenshot/greenshot/blob/main/src/Greenshot/SelfService/ViewModels/SetupSectionViewModel.cs
[stuckrects]: https://github.com/awesome-windows11/windows11/blob/main/README.md
[nilesoft]: https://github.com/moudey/Shell/blob/main/src/shared/Globals.h
[ms-hook]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexa
[ms-run]: https://learn.microsoft.com/en-us/windows/win32/setupapi/run-and-runonce-registry-keys
[ms-logon-delay]: https://learn.microsoft.com/en-us/windows/win32/taskschd/logontrigger-delay
[ms-qa-startup]: https://learn.microsoft.com/en-us/answers/questions/4059183/startup-apps-artificially-delayed-on-windows-11
[ms-abm]: https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shappbarmessage
[ms-notifyicon-doc]: https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw
[ms-notif-listener]: https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/notification-listener
[ms-unl-class]: https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.management.usernotificationlistener
[ms-caps]: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations
[ms-sparse]: https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps
[ms-notifmode]: https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotificationmode
[ms-focus]: https://learn.microsoft.com/en-us/uwp/api/windows.ui.shell.focussessionmanager
[ms-wifi-loc]: https://learn.microsoft.com/en-us/windows/win32/nativewifi/wi-fi-access-location-changes
[ms-radio]: https://learn.microsoft.com/en-us/uwp/api/windows.devices.radios.radio
[ms-sdk-asm2]: https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nn-audiopolicy-iaudiosessionmanager2
[ms-sdk-tsf]: https://learn.microsoft.com/en-us/windows/win32/api/msctf/nn-msctf-itfinputprocessorprofilemgr
[ms-sdk-bright]: https://learn.microsoft.com/en-us/windows/win32/api/highlevelmonitorconfigurationapi/nf-highlevelmonitorconfigurationapi-getmonitorbrightness
[ms-dwm]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
[ms-spi]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-systemparametersinfow
[ms-setsyscursor]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setsystemcursor
[ms-idesktopwp]: https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-idesktopwallpaper
[ms-ctx-blog]: https://blogs.windows.com/windowsdeveloper/2021/07/19/extending-the-context-menu-and-share-dialog-in-windows-11/
[ms-wv2-process]: https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model
[ms-wv2-perf]: https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance
[ms-cmdpal]: https://learn.microsoft.com/en-us/windows/powertoys/command-palette/overview
[ms-cmdpal-dock]: https://learn.microsoft.com/en-us/windows/powertoys/command-palette/dock
[ms-cmdpal-ext]: https://learn.microsoft.com/en-us/windows/powertoys/command-palette/extensibility-overview
