# Windows tiling WMs and Win32 edge cases: how they do it, from primary sources

Research for mypaper's scrolling ("infinite canvas", niri/PaperWM-style) tiling WM.
Everything was read on **2026-11-08**. Repos are shallow clones in `C:\mypaper-targets\tiling-research\src\` at these commits:

| Repo | Commit (date) | Lang |
|---|---|---|
| LGUG2Z/komorebi | `72aa4c9` (2026-10-30) | Rust |
| LGUG2Z/komorebi-application-specific-configuration | `7bc0e8b` (2026-10-18) | YAML |
| glzr-io/glazewm | `5709ad0` (2026-04-09) | Rust |
| dalyIsaac/Whim | `1e86b57` (2025-12-14) | C# |
| workspacer/workspacer | `8ab569e` (2024-10-27) | C# |
| eythaann/Seelen-UI | `bc53be8` (2026-11-05) | Rust |
| microsoft/PowerToys (FancyZones only, sparse) | `ba946c9` (2026-11-06) | C++ |
| jcardama/LeopardWM (scrolling) | `36574ea` (2026-11-04) | Rust |
| subie/swm (scrolling) | `4d6b035` (2026-06-26) | C# |

Paths below are relative to `src\`. "Learn" means learn.microsoft.com. Anything I could not trace to a source is marked **(unverified)**.

---

## 0. The findings that matter most for mypaper

1. **Nobody can cloak another process's window through a public API.** `DWMWA_CLOAK` only works on the caller's own windows and returns `E_ACCESSDENIED` for foreign HWNDs. Two independent codebases say so in comments: `LeopardWM/crates/platform_win32/src/placement.rs:41-47` and `Seelen-UI/libs/positioning/src/shell_view.rs:1-2`. Learn's DWMWA_CLOAK entry doesn't state the restriction either way, so treat it as empirical but consistent. Every WM that cloaks (komorebi's default, GlazeWM's default, Seelen during animations) uses the **undocumented** `IApplicationViewCollection::GetViewForHwnd` → `IApplicationView::SetCloak(1, 2|0)`. They reach it through `CoCreateInstance(CLSID_ImmersiveShell)` + `IServiceProvider::QueryService`. That is out-of-process COM into explorer, not code injection, but it is undocumented (vtable taken from Ciantic/AltTabAccessor; `komorebi/komorebi/src/com/interfaces.rs:1-3`). It depends on Explorer running, which matters for mypaper: Explorer still runs but its taskbar is hidden. **(unverified)** that it works with the taskbar hidden, since none of the sources tested that.
2. **komorebi's Scrolling layout parks windows instead of hiding them.** A column that scrolls out of view is moved to the work-area edge with **5 px left on screen** and keeps its size (`komorebi/komorebi/src/scrolling.rs:46-69`). If another monitor lies past that edge, the window is **cloaked** instead, and a partly visible window is clamped to its own work area (`scrolling.rs:115-145`; `docs/usage/scrolling-layout.md` "Windows that are out of view"). LeopardWM parks off-screen windows at a sentinel `(-32768, -32768)` (`i16::MIN`, `platform_win32/src/lib.rs:123`; `visibility.rs:34-80`). swm lets windows straddle monitor bezels and moves them off the virtual desktop. This is the main design choice mypaper has to make.
3. **Animate DWM thumbnails, not the windows.** komorebi keeps the real window cloaked and still, and moves a DWM thumbnail of it on one full-virtual-screen host window. That costs one `DwmUpdateThumbnailProperties` per frame (`komorebi/komorebi/src/animation/ghost.rs:1-14`). Learn confirms a cloaked window "is still composed by DWM", so its thumbnail stays live (DWMWA_CLOAK, [DWMWINDOWATTRIBUTE](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)). This is the cheapest way to get smooth scroll animation with near-zero cost to the apps.
4. **Windows that refuse a width have to be learned from what they do.** `WM_GETMINMAXINFO` limits are private to the app. komorebi sets the width, reads the result back via `DWMWA_EXTENDED_FRAME_BOUNDS`, and if the window ended up more than 16 px wider, it raises that column's minimum width, only ever upward (`scrolling.rs:37-38,171-243`). LeopardWM records a `WidthViolation`/`HeightViolation` the same way (`placement.rs:474-490`).
5. **Crash recovery needs an on-disk journal.** A cloaked window looks exactly like one on another virtual desktop or a suspended UWP app. So komorebi writes `%LOCALAPPDATA%\komorebi\komorebi.cloaked.json` on every cloak/uncloak and uncloaks those HWNDs on the next start (`komorebi/komorebi/src/cloaked.rs`). GlazeWM has a separate `wm-watcher` process that restores windows if the WM dies (`glazewm/packages/wm-watcher/src/main.rs`). LeopardWM has a `watchdog` crate and panic-hook recovery (`crates/watchdog/src/main.rs:1`; `visibility.rs:731-770`). GlazeWM issue [#1358](https://github.com/glzr-io/glazewm/issues/1358) (open, 2026) shows that windows can be left **permanently cloaked**. Plain `ShowWindow` cannot undo that; only `SetCloak(1,0)` can.
6. **Foreground stealing.** All the Rust WMs use the same allowed trick. They send an empty `SendInput` mouse event to their own process, so that "the calling process received the last input event", then call `SetForegroundWindow` (`komorebi/.../windows_api.rs:722-748`; `glazewm/.../native_window.rs:285-307`). Whim and workspacer use `keybd_event(0,0,0,0)` (`Whim/src/Whim/Native/NativeManager.cs:40-48`). komorebi also sets `SPI_SETFOREGROUNDLOCKTIMEOUT` to 0 at startup (`windows_api.rs:1186-1222`, `main.rs:254`). That is a **user-wide setting change**, which mypaper's net-negative and no-surprises rules should avoid.
7. **No WM gets around UIPI.** A non-elevated WM cannot move, resize or restyle elevated windows. LeopardWM checks integrity levels up front and skips such windows (`platform_win32/src/elevation.rs:1-12`). FancyZones warns the user (`FancyZonesLib/WindowDrag.cpp:41-44`). `SetWindowLongPtr` documents the failure ([Learn](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongptrw)). mypaper must float or ignore elevated windows unless it runs elevated or as UIAccess.
8. **None of the WMs inject code**, with one exception. Seelen-UI installs a `WH_CALLWNDPROC` hook DLL into Explorer's `Shell_TrayWnd` thread to capture tray and taskbar calls (`Seelen-UI/src/background/system_modules/system_tray/application/windows/tray_hook_loader.rs:44-60`), which is injection and banned for mypaper. komorebi, GlazeWM, Whim, workspacer, FancyZones, LeopardWM and swm use only `WINEVENT_OUTOFCONTEXT` WinEvent hooks and `WH_KEYBOARD_LL`/`WH_MOUSE_LL`. All of those run in the caller's own process ([SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook), [LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)).

---

## 1. komorebi (closest to us)

### 1.1 Event intake
- **One hook for every event:** `SetWinEventHook(EVENT_MIN, EVENT_MAX, None, cb, 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS)` on a dedicated thread. That thread runs a `GetMessageW` loop that **sleeps 10 ms after every message** (`komorebi/src/winevent_listener.rs:24-56`). Events go into a bounded(20) crossbeam channel. A full-range hook is wasteful: the callback is called for every caret, cursor and control event system-wide. mypaper should register narrow ranges, as GlazeWM, Whim and FancyZones do (below).
- **Filtering in the callback:** it returns early unless `idObject == OBJID_WINDOW` (`windows_callbacks.rs:146-148`). LOCATIONCHANGE is the costliest event. The code comment says "far more of these are sent than of anything else, by every window that is moved, and what a window is takes a call into it for each of the exe, the title and the class". For LOCATIONCHANGE it only checks a hash set plus `IsZoomed`, to synthesise Maximize/Unmaximize (`windows_callbacks.rs:121-140, 211-224`).
- **WinEvent to WM event mapping** (`window_manager_event.rs:162-232`):
  - DESTROY → Destroy, HIDE → Hide, CLOAKED → Cloak
  - MINIMIZESTART → Minimize; SHOW or MINIMIZEEND → Show; UNCLOAKED → Uncloak
  - FOCUS or FOREGROUND → FocusChange; MOVESIZESTART/END → move-resize
  - CAPTURESTART/END → MouseCapture
  - NAMECHANGE → Show, but only for apps on the `object_name_change` list. Firefox and IntelliJ "don't send ObjectCreate or ObjectShow on launch", only NAMECHANGE plus LOCATIONCHANGE. It is skipped while the window is minimized ([#941](https://github.com/LGUG2Z/komorebi/issues/941)).
- **Reaper thread:** polls **every 20 ms** for every managed HWND (`reaper.rs:174-206`). It drops the HWND if `!IsWindow` or `!IsWindowVisible`, the latter as a hack for invisible WS_EX_LAYERED Word windows. That polling is an idle CPU cost mypaper should avoid; use DESTROY plus lazy validation instead.
- **Hide handling** (`process_event.rs:352-397`): many apps send HIDE when they minimize, close, or close to tray. komorebi only drops a window on HIDE if `!IsWindow`, or if the app matches `tray_and_multi_window` and komorebi didn't hide it itself (tracked in `HIDDEN_HWNDS`). Built-in tray list (`lib.rs:94-126`): explorer, firefox, chrome, idea64, ApplicationFrameHost, steam.
- **Display-change grace:** for 10 s after a display connect, resume or unlock, minimize events are ignored, "to avoid treating OS-initiated minimizes (caused by transient monitor disconnects) as user-initiated" (`monitor_reconciliator/mod.rs:48-101`; `process_event.rs:330-336`).

### 1.2 Which windows get managed (`window.rs:812-1074`)
The checks run in this order:
1. `IsWindow`.
2. Rect at least the minimum width and height.
3. Has a title.
4. Not cloaked, unless the event is Hide or Cloak.
5. Class not in `PERMAIGNORE_CLASSES` (`Chrome_RenderWidgetHostHWND`).
6. Ignore rules (built in: `OPContainerClass` and `IHWindowClass` from mstsc/WSL [#74](https://github.com/LGUG2Z/komorebi/issues/74), plus komorebi-bar).
7. Manage-override rules.
8. Then: `(WSL2 X server exe || titlebar-removed rule || (WS_CAPTION && WS_EX_WINDOWEDGE)) && !WS_EX_DLGMODALFRAME && (layered-whitelisted || !WS_EX_LAYERED)`, or a force-manage rule.

`WS_EX_LAYERED` is rejected by default because it produces "a lot of dupe events". Steam is whitelisted (`lib.rs:87-93`). Firefox is a "slow application": the WM sleeps 20 ms before reading its styles (`lib.rs:185-191, 261`).

**Separate filter used for borders:** `WS_CHILD || WS_EX_TOOLWINDOW || WS_EX_NOACTIVATE` (`windows_callbacks.rs:91-98`).

**Startup enumeration** (`windows_callbacks.rs:46-71`): visible, not minimized, `should_manage`. Windows that are maximized get restored (`SW_SHOWNOACTIVATE`) so they can be tiled.

### 1.3 Positioning and invisible borders (`windows_api.rs:501-552, 789-821`)
- The visual rect is read from `DWMWA_EXTENDED_FRAME_BOUNDS`, with a fallback to `GetWindowRect`.
- `shadow_rect = GetWindowRect - ExtendedFrameBounds`. It is added to the target rect, so the **painted** area matches the layout exactly. That is the Win10/11 invisible resize border of about 7 px.
- Flags: `SWP_NOACTIVATE | SWP_NOSENDCHANGING | SWP_NOCOPYBITS | SWP_FRAMECHANGED`. Under the `Async` handling mode it also adds `SWP_ASYNCWINDOWPOS`, so a hung target can't block the WM. The default mode is `Sync` (`lib.rs:263-264`). Sync is required to read back the true size for minimum-width learning (`scrolling.rs:40-45`).
- **Never `HWND_TOPMOST` for tiled windows.** The code comment explains that TOPMOST is "sticky" and "viral" to owned windows; komorebi uses `HWND_TOP` instead. It cites the [SetWindowPos remarks](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos): making a window topmost also makes its owned windows topmost.
- **DPI:** the process sets `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2` (`windows_api.rs:1169-1172`; `main.rs:223`). A `TODO(raggi)` notes that rects are not scaled per window.

### 1.4 Hiding (workspaces, stacks)
`HidingBehaviour` (`core/mod.rs:788-796`) has three values:
- `Hide`: **end-of-life**, "has issues with Electron apps". Uses `SW_HIDE`.
- `Minimize`: "has issues with frequent workspace switching".
- `Cloak`: the default (`lib.rs:202-203`). Calls `SetCloak(hwnd, 1, 2)` to hide and `SetCloak(hwnd, 1, 0)` to show (`window.rs:488-554`).

The `SetCloak` implementation is `com/mod.rs`. It creates `CoInitializeEx(MTA)` once per thread, but creates a **new `CLSID_ImmersiveShell` on every call**, so every cloak is an out-of-process round trip into explorer. Seelen caches a process-wide proxy instead (`shell_view.rs:23-27, 104-120`), and GlazeWM caches one per thread and refreshes it when explorer restarts (`com.rs:93-115`).

**Known Cloak problem:** [#1755](https://github.com/LGUG2Z/komorebi/issues/1755) (v0.1.41, Win11 25H2, open). Cloaking a workspace that holds only the WinUI3 WhatsApp app makes the desktop (Progman) stop responding to clicks. The reporter suspects how Explorer handles a cloaked WinUI3 view; no root cause yet.

### 1.5 Scrolling layout
It exists. The pure model is the `komorebi-scrolling` crate (`komorebi-scrolling/src/lib.rs`, `forbid(unsafe_code)`, explicitly "written from" niri's docs). Glue is in `komorebi/src/scrolling.rs`, docs in `docs/usage/scrolling-layout.md`. The documented invariants (`lib.rs:69-84`):
- A column's width is its own.
- **The focused column does not move by itself.**
- Scroll as little as possible.
- Temporary changes are undone (closing a just-opened column returns to the old view).
- A column wider than the viewport starts at its left edge.
- Proportions include gaps.
- Fullscreen is a column.

Also:
- Touchpad pans come from whkd reading the touchpad itself, because "Windows does not give three-finger gestures to programs" (`docs/usage/scrolling-layout.md`, Panning; `pan.rs`).
- Known limit: "Windows resting at the edges have borders too, so you may see a thin line of one at the edge of the screen".

### 1.6 Crash recovery and exit
- `cloaked.rs` journal (§0.5). `uncloak_left_behind()` runs from `WindowManager::new` (`window_manager.rs:187`).
- `restore_all_windows` (`window_manager.rs:1467-1517`) restores title bars, opacity and accents, and uncloaks or shows every window.
- There is no watchdog process, and no handling for a window left at the 5 px park position. A killed daemon leaves scrolled-out windows hanging off the screen edge (by reading of `cloaked.rs`, which only tracks cloaks).

### 1.7 Other details
- **Workspaces** are `Ring<Monitor>` → `Ring<Workspace>`. A workspace switch cloaks one workspace's windows and uncloaks the other's.
- **Virtual desktops:** komorebi binds to the one it started on. It reads `HKCU\...\Explorer\VirtualDesktops\CurrentVirtualDesktop`, or on Win10 `SessionInfo\<id>\VirtualDesktops` (`lib.rs:296-330`). While another desktop is active, it **ignores all events** (`process_event.rs:194-258`). No virtual-desktop COM is used.
- **Monitor hotplug:** a hidden window listens for:
  - `WM_DISPLAYCHANGE`
  - `WM_SETTINGCHANGE(SPI_SETWORKAREA)`
  - `WM_DEVICECHANGE`, registered for `GUID_DEVINTERFACE_MONITOR`, `GUID_DEVINTERFACE_DISPLAY_ADAPTER` and `GUID_DEVINTERFACE_VIDEO_OUTPUT_ARRIVAL`
  - `WM_POWERBROADCAST`: suspend, resume, lid via `GUID_LIDSWITCH_STATE_CHANGE`
  - `WM_WTSSESSION_CHANGE`: lock and unlock

  (`monitor_reconciliator/hidden.rs:1-306`.) Workspace state for unplugged monitors goes into `MONITOR_CACHE`, keyed by serial or device id, and is restored when the monitor returns (`monitor_reconciliator/mod.rs:55, 104-119, 640-760`). The troubleshooting docs say Samsung G8/G9 VRR displays lose their device id after sleep (`docs/troubleshooting.md`).
- **Elevation:** no integrity check. The only mention is a border-state fix for after an admin window takes focus (`border_manager/mod.rs:554-557`).

### 1.8 App-specific configuration (188 apps, `komorebi-application-specific-configuration/applications.yaml`)
| Option | Count | Examples |
|---|---|---|
| `float_identifiers` | 82 | `#32770` dialogs, `TaskManagerWindow`, `OperationStatusWindow` (Explorer copy dialog), `msiexec.exe`, `wusa.exe`, `MsoSplash`-style splash classes, 1Password |
| `tray_and_multi_window` | 101 | Discord, Steam, Telegram, Creative Cloud, most chat/launcher apps |
| `object_name_change` | 16 | Firefox, Zen, all JetBrains IDEs, Visual Studio, mpv, Godot Manager |
| `force` | 11 | Affinity suite, conhost, WeChat, Everything 1.5a, GOG Galaxy, Visio |
| `layered` | 5 | Discord, Excel, Outlook, PowerPoint, Word |

mypaper needs a table like this; its absence is the first thing users will hit.

---

## 2. Other WMs

### 2.1 Hiding windows that aren't on the current workspace or view
| WM | Method | Source |
|---|---|---|
| komorebi | Cloak (default), Minimize, Hide (EOL). Scrolling layout parks with 5 px showing, or cloaks next to another monitor | §1.4, §1.5 |
| GlazeWM | `hide_method`: **cloak** (default), `hide` (SW_HIDE; "Legacy… stability issues with some apps"), `place_in_corner` (1 px sliver at a bottom corner; always used on macOS). With cloak + `show_all_in_taskbar:false` it also calls `ITaskbarList2::DeleteTab/AddTab` | `glazewm/packages/wm-common/src/parsed_config.rs:145-150`; `resources/assets/sample-config.yaml:31-41`; `wm/src/commands/general/platform_sync.rs:316-372, 460-468`; `native_window.rs:466-530` |
| Whim | **SW_HIDE** on workspace deactivate (`DeactivateWorkspaceTransform`); `SW_SHOWNOACTIVATE` + SetWindowPos on activate | `Whim/src/Whim/Store/MapSector/Transforms/DeactivateWorkspaceTransform.cs`; `Native/NativeManager.cs:50-80` |
| workspacer | **SW_HIDE** (`WindowsWindow.Hide`) | `workspacer/src/workspacer.Native/Native/WindowsWindow.cs:179-187` |
| Seelen-UI | **Minimize** (`MINIMIZED_BY_WORKSPACES` set). Restores only what it minimized itself; distinguishes the user's own unminimize by queueing its own restores | `Seelen-UI/src/background/virtual_desktops/application/mod.rs:27-40, 514-560` |
| LeopardWM | Off-screen **sentinel (-32768,-32768)** with `SWP_NOSIZE` | `LeopardWM/crates/platform_win32/src/visibility.rs:34-80`; `lib.rs:123` |
| swm | None: one reel across all monitors, windows straddle bezels, focus kept on the virtual desktop | `swm/ScrollingWM/Core/Layout.cs:7-18, 93-118` |
| FancyZones | N/A (zones only, no workspaces) | n/a |
| bug.n / nog | bug.n (AutoHotkey) hides with WinHide, i.e. SW_HIDE; nog is archived **(unverified, not read)** | n/a |

GlazeWM switched to cloaking in PR [#792](https://github.com/glzr-io/glazewm/pull/792) (merged 2024-11-07, v3.6.0). The contributor's reasons: SW_HIDE "completely removes the window from the taskbar", and cloaking fixed #250 (Excel loses its ribbon) and #759 (Electron apps minimized on workspace switch). Some windows have **no application view**, such as the Outlook splash screen and the OneDrive quit screen; `GetViewForHwnd` fails and those windows are "essentially ignored". GlazeWM's changelog also lists "Fixed window cloaking no longer working restarting Explorer (#1273)", so the COM proxy must be recreated after an Explorer restart.

### 2.2 Window filter (what counts as a managed app window)
| WM | Rule | Source |
|---|---|---|
| GlazeWM | visible and **not cloaked**; not `WS_CHILD`; not `WS_EX_NOACTIVATE`/`WS_EX_TOOLWINDOW` ("unavailable in task switcher"); **owned and no `WS_CAPTION` → ignore** (menus/autocomplete such as Notepad++, KeePass); non-resizable (no `WS_THICKFRAME`) → floating; frame covering the workspace (inset 1 px) → fullscreen | `wm/src/commands/window/manage_window.rs:85-146, 265-305`; `native_window.rs:170-194` |
| Whim | `GetAncestor(GA_ROOT)==hwnd`, visible; `WS_POPUP` without thickframe/min/max box → not interesting; not CHILD/DISABLED/TOOLWINDOW/NOACTIVATE; not desktop/shell/`WorkerW`/`Progman`/`Shell_TrayWnd`/`Shell_SecondaryTrayWnd`/`SysListView32`; `MsoSplash` is a splash screen; owner invisible or zero-sized counts as "no visible owner"; a failed DWMWA_CLOAKED query counts as **cloaked** | `Whim/src/Whim/Native/CoreNativeManager.cs:99-252` |
| FancyZones | WS_VISIBLE; not TOOLWINDOW; root; popup kept only if thickframe and (caption or min/max box) ("Calculator, Telegram" yes; start menu, tray, menus no); visible owner → child window (opt-in); exclusion list; **must be on the current virtual desktop** (`IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop`) | `PowerToys/src/modules/fancyzones/FancyZonesLib/FancyZonesWindowProcessing.cpp:15-70`; `VirtualDesktop.cpp:177-199` |
| komorebi | §1.2 (needs `WS_CAPTION && WS_EX_WINDOWEDGE`, rejects LAYERED and DLGMODALFRAME) | §1.2 |
| swm | float rules for `#32770`, `consent.exe`, `ApplicationFrameHost.exe` ("XAML CoreWindow doesn't reliably re-lay-out when the outer host is resized"), `Taskmgr.exe`; auto-float without `WS_THICKFRAME|WS_MAXIMIZEBOX` | `swm/README.md` "Configure" |

### 2.3 Hooks: none of these inject except Seelen's tray hook
| WM | WinEvent ranges (all OUTOFCONTEXT) | Input hooks | Source |
|---|---|---|---|
| komorebi | EVENT_MIN..EVENT_MAX | none in daemon (whkd/AHK do hotkeys) | `winevent_listener.rs:28-36` |
| GlazeWM | DESTROY..HIDE, MINIMIZESTART..END, MOVESIZESTART..END, FOREGROUND, LOCATIONCHANGE..NAMECHANGE, CLOAKED..UNCLOAKED | WH_KEYBOARD_LL, WH_MOUSE_LL | `wm-platform/src/platform_impl/windows/window_listener.rs:63-90`; `keyboard_hook.rs:117` |
| Whim | DESTROY..HIDE, CLOAKED..UNCLOAKED, MOVESIZE, FOREGROUND, LOCATIONCHANGE, MINIMIZE | WH_MOUSE_LL | `Whim/src/Whim/Store/WindowSector/WindowEventListener.cs:35-63` |
| FancyZones | MOVESIZESTART/END, NAMECHANGE, UNCLOAKED, SHOW, CREATE, DESTROY; **LOCATIONCHANGE only between MOVESIZESTART and MOVESIZEEND** | WH_KEYBOARD_LL, WH_MOUSE_LL | `FancyZones/FancyZonesApp.cpp:96-160`; `FancyZonesLib/GenericKeyHook.h:27`; `MouseButtonsHook.cpp:38` |
| LeopardWM | CREATE, DESTROY, SHOW, HIDE, FOREGROUND, MINIMIZE*, MOVESIZE*, LOCATIONCHANGE, FOCUS, NAMECHANGE | keyboard + mouse LL, raw touchpad | `crates/platform_win32/src/event_hooks.rs:19-22, 150-227` |
| Seelen-UI | (WinEvents) + **WH_CALLWNDPROC DLL into Explorer's tray thread** (injection) | — | `tray_hook_loader.rs:44-60`; `src/hook_dll/src/lib.rs` |

The FancyZones approach of hooking LOCATIONCHANGE only during a drag is the right low-idle pattern for mypaper. mypaper can't do exactly that, because apps resize themselves outside drags (§3.4). A cheap filter in the callback (komorebi-style hash-set check) is the compromise.

### 2.4 Focus
- **komorebi:** `SendInput(empty mouse)` → `SetWindowPos(HWND_TOP, SWP_SHOWWINDOW|SWP_ASYNCWINDOWPOS)` → `SetForegroundWindow`; ForegroundLockTimeout=0 (§0.6).
- **GlazeWM:** `SendInput` with `dwExtraInfo = 6379`, a marker its own mouse hook uses to ignore the event (`native_window.rs:53, 285-307`; `mouse_listener.rs:190`).
- **Whim and workspacer:** `keybd_event(0,0,0,0)` + `SetForegroundWindow` (`NativeManager.cs:40-48`, crediting workspacer commit 1c02613).
- **LeopardWM:** `AttachThreadInput` "trick" (`platform_win32/src/focus.rs:192-283`). Not recommended: attaching input queues to a hung app can hang the WM **(unverified, general Win32 lore)**.

### 2.5 Restore on exit or crash
- **komorebi:** journal + `restore_all_windows` (§1.6).
- **GlazeWM:** `Drop for WmState` re-places each window and calls `ShowWindowAsync(SW_SHOWNA)`, `AddTab`, resets border colour and opacity (`wm/src/wm_state.rs:684-712`). The **wm-watcher** process subscribes over IPC; if the WM exits unexpectedly it shows windows, re-adds taskbar tabs, and resets border and opacity (`wm-watcher/src/main.rs:13-55`).
  - Caveat: the watcher calls `show()` (SW_SHOWNA) but **never uncloaks**. That matches the root cause one fork's README gives for windows staying invisible after a GlazeWM crash, which is also issue #1358's symptom (§0.5).
- **Whim:** `WindowSector.Dispose` → `window.Restore()` for every window (`Whim/src/Whim/Store/WindowSector/WindowSector.cs:79-100`). No crash path.
- **workspacer:** separate `workspacer.Watcher` process → `ShowInCurrentState` for the handles it was given (`src/workspacer.Watcher/Program.cs:84-94`).
- **LeopardWM:** watchdog process ("restores hidden windows after abnormal exits"), panic-hook `restore_all_windows_moved_offscreen_best_effort()`, tray-quit deadline (`crates/watchdog/src/main.rs:1`; `visibility.rs:434-770`; `daemon/src/quit_fallback.rs`).
- **swm:** writes `~/.swm/original-rects.json` continuously. Ctrl-C restores the original rects, recentring any that are now off-screen. `swm-rescue.ps1` restores from that file after a kill (`swm/swm-rescue.ps1:1-40`; README "Cleanup-onscreen guarantee").

### 2.6 DPI
- GlazeWM: "When there's a mismatch between the DPI of the monitor and the window, the window might be sized incorrectly after the first move. If we set the position twice, inconsistencies after the first move are resolved" (`platform_sync.rs:447-455`).
- Whim: SetWindowPos runs **twice whenever any monitor isn't at 100 % scale** (`Whim/src/Whim/Native/DeferWindowPosHandle.cs:10-14, 116-128`).
- FancyZones: for DPI-unaware windows on mixed-DPI setups, it clamps the rect to the monitor (`FancyZonesLib/WindowUtils.cpp:96-105`).

### 2.7 FancyZones: how Microsoft's own tool overrides Snap
- **No injection.** It uses `WH_KEYBOARD_LL` and `WH_MOUSE_LL` in its own process, and out-of-context WinEvents.
- With `overrideSnapHotkeys` on, the LL hook **swallows Win+Left/Right/Up/Down** (returns true) and posts its own message instead (`FancyZonesLib/FancyZones.cpp:686-750, 1653-1660`). Explorer's Win+arrow Snap can therefore be intercepted from an LL hook with no explorer patching.
- It moves windows with `SetWindowPlacement(WPF_ASYNCWINDOWPLACEMENT)`. That preserves restore and maximize state. It waits up to 5×100 ms for a just-restored window to leave the minimized state ([PowerToys #1685](https://github.com/microsoft/PowerToys/issues/1685)), and converts screen to workspace coordinates first (`WindowUtils.cpp:65-106, 213-260`).
- Virtual desktops: the current desktop comes from `HKCU\...\Explorer\SessionInfo\%d\VirtualDesktops\CurrentVirtualDesktop`. The public `IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop` filters windows. A desktop switch is detected via **`EVENT_OBJECT_NAMECHANGE` on `GetDesktopWindow()`** (`VirtualDesktop.cpp:10-60`; `FancyZonesApp.cpp:160-170`).

### 2.8 Scrolling WMs on Windows
- **komorebi Scrolling layout** (§1.5): the most complete.
- **LeopardWM:**
  - Scrollable tiling with DwmFlush-paced animation and a three-layer "transient window suppression" for Electron popups that create and hide `Chrome_WidgetWin_1` every 20-120 s (`agent_docs/architecture.md`).
  - Base modifier `Ctrl+Alt` to avoid Win-key conflicts. Its testing found 12 of 33 Win+Alt combos fail (Game Bar, Snap) and Raycast holds many Win+Ctrl combos (same doc, "Conflict tables").
  - Disables Snap Layouts by **removing `WS_MAXIMIZEBOX` from managed windows** and restores it on release (`daemon/src/config.rs:401-405`). Cross-process `SetWindowLongPtr(GWL_STYLE)` is allowed below UIPI ([Learn](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongptrw)), but it changes the app's real style: crash leftovers, and apps that read their own style.
  - Turns off DWM transition smoothing with `DWMWA_TRANSITIONS_FORCEDISABLED` (`placement.rs:29-35, 138-155`).
- **swm** (C#, AHK keys): one reel across monitors, per-virtual-desktop strips, drag-and-drop reorder, and it defers adopting new windows until the mouse button is up, for tab-tear races (`swm/README.md` "Behaviors of note").

---

## 3. Win32 edge-case catalogue

| # | Edge case | API facts (Learn) | What WMs do / recommendation |
|---|---|---|---|
| 3.1 | **Invisible resize borders** | `DWMWA_EXTENDED_FRAME_BOUNDS` "retrieves the extended frame bounds rectangle in screen space" ([DWMWINDOWATTRIBUTE](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)). `DWMWA_VISIBLE_FRAME_BORDER_THICKNESS` gives the visible border width (Win11 22000+) | Everyone computes `GetWindowRect − ExtendedFrameBounds` and adds it to the target (komorebi `windows_api.rs:808-821`, GlazeWM `shadow_borders`, FancyZones `AdjustRectForSizeWindowToRect` `WindowUtils.cpp:364-380`). swm compares against the extended rect to skip no-op moves (`Applier.cs:11-24`). LeopardWM **caches the insets** because DWM "may lose track of extended frame bounds" for windows coming back from off-screen (`placement.rs:414-419`) |
| 3.2 | **Fixed-size windows** (no `WS_THICKFRAME`) | — | GlazeWM, swm and FancyZones float them (§2.2). For a scrolling layout: float, or give them a column of their natural width |
| 3.3 | **Minimum/maximum size** (`WM_GETMINMAXINFO`) | Sent "when the size or position of the window is about to change"; ptMin/MaxTrackSize are the limits for sizing "by using the borders" ([Learn](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-getminmaxinfo)). It goes to the window's own proc and can't be queried cross-process | Learn the limit from the result (§0.4). Note that komorebi and GlazeWM pass `SWP_NOSENDCHANGING`, which suppresses WM_WINDOWPOSCHANGING ([SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)). That removes one of the app's ways to veto a size, though min-track enforcement may still apply **(unverified)** |
| 3.4 | **Apps that move or resize themselves** | — | Firefox moves its window "after an indeterminate timeout on startup" and acts "500ms after" events (Whim `FirefoxWindowProcessor.cs:44-60`, with searchfox links). swm re-reads real positions every pass because apps "move their own windows behind our back (browsers during tab tear, Win+Shift+arrow, snap layouts)" (`Applier.cs:11-14`). Recommendation: on LOCATIONCHANGE of a managed window not caused by us, re-assert the layout, debounced |
| 3.5 | **Per-monitor DPI v2, moving between DPIs** | WM_DPICHANGED is sent when "the window is moved to a new monitor that has a different DPI" or the monitor DPI changes; apps should apply the suggested rect ([Learn](https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged)). Unaware and system-aware windows are bitmap-stretched, and API results may be **virtualized to the calling thread's DPI context** ([High-DPI overview](https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows)) | The WM must be PMv2 itself (komorebi does). Set the position **twice** when a window crosses DPI boundaries (§2.6), because the app answers WM_DPICHANGED with its own suggested rect after the first move. For a strip spanning monitors with different DPIs, expect windows half on each monitor to flip DPI back and forth; keep a column wholly on one monitor while it settles |
| 3.6 | **UWP / ApplicationFrameHost / CoreWindow** | — | The visible frame is `ApplicationFrameWindow` in ApplicationFrameHost.exe. The app's `Windows.UI.Core.CoreWindow` is a child, or a separate **shell-cloaked** top-level window while minimized or off-desktop ([cmdtab #48](https://github.com/stianhoiland/cmdtab/issues/48); Alt-Tab enumerators special-case `Windows.UI.Core.CoreWindow`, per [Keypirinha alttab.py](https://github.com/Keypirinha/Packages/blob/master/TaskSwitcher/lib/alttab.py)). komorebi treats ApplicationFrameHost as tray/multi-window (`lib.rs:117-121`); swm floats it by default. Manage the frame, never the CoreWindow, and read the real process from the CoreWindow child. Ghost frames of closed apps stay cloaked |
| 3.7 | **Electron/Chromium custom frames** | — | `Chrome_RenderWidgetHostHWND` is permanently ignored (komorebi `lib.rs:178-180`). Electron emits spurious HIDE and transient `Chrome_WidgetWin_1` popups; LeopardWM checks `IsWindowVisible` before dropping a window and keeps a 5-min "recently hidden" suppression list (`architecture.md` "Transient Window Suppression"). komorebi: SW_HIDE "has issues with Electron apps" (`core/mod.rs:789`). Chrome and Firefox are tray/multi-window (`lib.rs:99-113`) |
| 3.8 | **Elevated windows (UIPI)** | SetWindowLongPtr "fails if the process that owns the window… is at a higher process privilege in the UIPI hierarchy" ([Learn](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongptrw)). SetWindowPos/MoveWindow against higher-integrity windows fail with ERROR_ACCESS_DENIED ([Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/2152111/not-working-setwindowpos-for-on-screen-keyboard-is), not a reference page). Running elevated or as UIAccess (signed, installed in a secure location) avoids it ([UIAccess policy](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-10/security/threat-protection/security-policy-settings/user-account-control-only-elevate-uiaccess-applications-that-are-installed-in-secure-locations)) | LeopardWM compares token integrity RIDs and skips higher ones. It also skips windows whose process token can't be read: "protected / PPL / anti-cheat" (`elevation.rs`). The tile WM reports hotkeys may not reach its LL hook while an elevated window is in front ([filipmares/tile #140](https://github.com/filipmares/tile/issues/140)) **(unverified for LL hooks in general)**. Live tests must run the tiler at medium integrity (`runas /trustlevel:0x20000`) to see this |
| 3.9 | **Owned windows, dialogs, tool windows, NOACTIVATE, popups** | Taskbar: unowned windows get a button; `WS_EX_APPWINDOW` forces one, `WS_EX_TOOLWINDOW` or a hidden owner prevents one ([The Taskbar](https://learn.microsoft.com/en-us/windows/win32/shell/taskbar)). Topmost propagates to owned windows ([SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)). Alt-Tab algorithm: walk to root owner, back down last active popup; TOOLWINDOW removes, APPWINDOW adds; "implementation detail that can change" ([Raymond Chen 2007](https://devblogs.microsoft.com/oldnewthing/20071008-00/?p=24863)) | Float owned windows that have a caption (dialogs) next to their owner, and ignore owned windows without a caption (GlazeWM). When an owner scrolls off-screen, **owned dialogs don't move with it**, so the WM has to move them, or scroll the owner back in when a dialog appears **(unverified; inference from cloak inheritance, see 3.13)**. `#32770` should float (komorebi config, swm). `WS_EX_DLGMODALFRAME` is rejected (komorebi) |
| 3.10 | **Splash screens** | — | `MsoSplash` (Whim `CoreNativeManager.cs:99-109`; also the cause of GlazeWM PR #792's Outlook crash), `android(splash)` (komorebi config). Generally: no WS_THICKFRAME, or popup without caption, so the filters above already catch them |
| 3.11 | **Fullscreen games** (exclusive vs borderless) | `SHQueryUserNotificationState` returns `QUNS_BUSY`, `QUNS_RUNNING_D3D_FULL_SCREEN`, `QUNS_PRESENTATION_MODE`, etc. "There are no notifications sent when the user starts or stops a full-screen application" ([Learn](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shqueryusernotificationstate)) | GlazeWM treats "window frame ≥ workspace + 1 px and not maximized" as fullscreen and calls `ITaskbarList2::MarkFullscreenWindow` (`manage_window.rs:278-299`; `native_window.rs:494-510`). komorebi [#1191](https://github.com/LGUG2Z/komorebi/issues/1191): native-fullscreen windows minimize on workspace switch whatever the hiding mode. Recommendation: a window whose rect covers its monitor and has no caption becomes a fullscreen column and is never moved or parked. Poll `SHQueryUserNotificationState` only on FOREGROUND changes |
| 3.12 | **Min/max/restore, WINDOWPLACEMENT** | rcNormalPosition is in **workspace coordinates** for non-tool top-level windows; mixing it with SetWindowPos makes windows "creep" ([WINDOWPLACEMENT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-windowplacement)). `WPF_ASYNCWINDOWPLACEMENT` posts to the owner thread. **Arranged** (snapped) is a fourth state: `IsWindowArranged` (User32 export, GetProcAddress only); showCmd can say SW_SHOWNORMAL while arranged ([IsWindowArranged](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-iswindowarranged)). Minimized windows sit at (-32000,-32000) on NT ([Chen](https://devblogs.microsoft.com/oldnewthing/20041028-00/?p=37453)) | GlazeWM restores with `SetWindowPlacement(SW_RESTORE, rcNormalPosition=target, WPF_ASYNCWINDOWPLACEMENT)` "same effect as SW_RESTORE but doesn't cause a flicker" (`platform_sync.rs:410-416`; `native_window.rs:435-463`). komorebi synthesises Maximize/Unmaximize from LOCATIONCHANGE + IsZoomed. LeopardWM: cross-process `ShowWindow(SW_SHOWNOACTIVATE)` "can… leave the window zoomed", so it re-posts with ShowWindowAsync and polls (`focus.rs:99-110`). LeopardWM keeps minimized windows in their column but collapses the gap (`architecture.md` "Minimize Behavior") |
| 3.13 | **Cloaked windows (who, why)** | `DWMWA_CLOAKED` → `DWM_CLOAKED_APP` (1), `_SHELL` (2), `_INHERITED` (4, "inherited from its owner window") ([Learn](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)). A cloaked window keeps WS_VISIBLE, on-monitor coordinates and WM_PAINT; old apps can't tell ([Chen 2020](https://devblogs.microsoft.com/oldnewthing/20200302-00/?p=103507)). Shell-cloaked covers other virtual desktops, suspended UWP and ghost frames | Always AND `IsWindowVisible` with `!DWMWA_CLOAKED` (GlazeWM `is_visible`). Whim treats a failed query as cloaked; LeopardWM does so **only if the HWND is invalid** (`enumeration.rs:645-680`). Seelen tags its own cloaks with a window property (`SetPropW CLOAKED_PROP`) so trackers can tell its transient cloak apart from shell cloaks (`shell_view.rs:30-58`). That is a neat trick, but writing a prop on a foreign HWND is a (benign) mutation |
| 3.14 | **Cloaking other processes' windows** | `DWMWA_CLOAK`: "Cloaks the window such that it is not visible to the user. The window is still composed by DWM." No cross-process statement on Learn | `E_ACCESSDENIED` for foreign HWNDs (§0.1). Alternatives below |
| 3.15 | **Alternatives to cloaking** | `SW_HIDE` removes the taskbar button (Taskbar doc; GlazeWM #792) and fires HIDE, which apps and WMs read as close-to-tray. `ITaskbarList::DeleteTab` "Deletes an item from the taskbar" only ([Learn](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-itaskbarlist-deletetab)); whether it affects Alt-Tab isn't documented **(unverified)** | Ranking for mypaper's scroll-out case: **(a) park off-screen** (public API, no state change, fully reversible, DWM thumbnails keep working) > **(b) SetCloak** (undocumented; best for workspace switches; journal it) > **(c) minimize** (visible animations, focus side effects, Whim's "delicate" ordering comment `DeferWindowPosHandle.cs:130-145`) > **(d) SW_HIDE** (breaks Electron, Excel ribbon, tray detection) |
| 3.16 | **Moving windows far off-screen** | No Learn page states a hard SetWindowPos coordinate maximum **(unverified)**. WM_MOVE/WM_SIZE pack coordinates in 16-bit lParam halves, so beyond ±32767 apps see wrapped values (well-known; Learn [WM_MOVE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-move) uses GET_X_LPARAM) | LeopardWM's sentinel −32768 sits right at the i16 edge. komorebi parks **adjacent to the work area** (5 px visible). That is the safest choice: no huge coordinates, no surprises from monitor-change rescue, and a short animation back in. Its cost is the 5 px border sliver at edges. On monitor removal Windows itself relocates windows, and komorebi ignores the minimizes it causes for 10 s (§1.1). Windows' heuristics for bringing windows back on-screen are not documented **(unverified)**. Apps that clamp themselves on-screen will fight parking; detect "moved back by itself" and fall back to cloak |
| 3.17 | **Alt+Tab / Task View / Win+arrow / Snap** | `RegisterHotKey`: "Keyboard shortcuts that involve the WINDOWS key are reserved for use by the operating system"; it fails if another registrant holds the combo; F12 is reserved ([Learn](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey)). `SPI_SETWINARRANGING` turns window arrangement (dock-moving, snap sizing, drag-from-maximize) on or off globally; sub-settings `SPI_SETDOCKMOVING`, `SPI_SETSNAPSIZING`, `SPI_SETDRAGFROMMAXIMIZE`. `SPIF_UPDATEINIFILE` persists to the profile ([SystemParametersInfo](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-systemparametersinfow)). `EVENT_SYSTEM_SWITCHSTART/END` report Alt+Tab; `EVENT_SYSTEM_ARRANGMENTPREVIEW` "A preview rectangle is being displayed" ([Event Constants](https://learn.microsoft.com/en-us/windows/win32/winauto/event-constants)) | Win+arrow: swallow in WH_KEYBOARD_LL (FancyZones, §2.7). Snap Layouts flyout: per-window removal of WS_MAXIMIZEBOX (LeopardWM), or registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\EnableSnapAssistFlyout=0`, `EnableSnapBar=0`; master switch `HKCU\Control Panel\Desktop\WindowArrangementActive` ([ElevenForum](https://www.elevenforum.com/t/enable-or-disable-snap-layouts-for-maximize-button-in-windows-11.61/), [Win11Debloat reg](https://github.com/Raphire/Win11Debloat/blob/master/Regfiles/Disable_Snap_Layouts.reg); third-party, **Explorer restart may be needed**). Aero Shake: `...\Explorer\Advanced\DisallowShaking=1` or policy `NoWindowMinimizingShortcuts` ([ElevenForum](https://www.elevenforum.com/t/enable-or-disable-title-bar-window-shake-in-windows-11.2078/)). Other value names (`SnapAssist`, `JointResize`, `DITest`, `EnableTaskGroups`) are **(unverified)**. For a shell replacement, prefer `SPI_SETWINARRANGING` **without** `SPIF_UPDATEINIFILE` (session-only, reverts at logoff), so a crash leaves nothing behind, plus LL-hook interception. Whether Win+L and Ctrl+Alt+Del can be blocked by an LL hook: they can't (secure attention / winlogon) **(unverified)** |
| 3.18 | **SetForegroundWindow restrictions** | Allowed only if not a UWP caller, no LockSetForegroundWindow, no active menus, **and** one of: lock timeout expired, caller is foreground, started by foreground, no foreground window, **caller received the last input event**, or debugging. "It is possible for a process to be denied… even if it meets these conditions." Otherwise the taskbar flashes. `SPI_SETFOREGROUNDLOCKTIMEOUT`: "The calling thread must be able to change the foreground window, otherwise the call fails." To bring a window to the top with SetWindowPos, "the process that owns the window must have SetForegroundWindow permission" ([SetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow), [SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)) | Self-SendInput then SetForegroundWindow (§2.4). Since mypaper owns the hotkeys (LL hook in its process), the hotkey keystroke itself makes mypaper the "last input" receiver, so `SetForegroundWindow` from the hotkey handler should usually succeed without tricks **(unverified, but this is how Learn's rule reads)** |
| 3.19 | **WinEvents needed and their cost** | Out-of-context events are queued, in order, delivered on the hooking thread, which **must pump messages**; skip-own-process flag available ([SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)). The system generates SHOW, HIDE (caret/cursor only per doc, though WMs rely on it for windows), DESTROY, CREATE, NAMECHANGE, LOCATIONCHANGE, FOREGROUND, MINIMIZESTART/END, MOVESIZESTART/END, CLOAKED/UNCLOAKED, SWITCHSTART/END. LOCATIONCHANGE fires for caret moves too ([Event Constants](https://learn.microsoft.com/en-us/windows/win32/winauto/event-constants)) | Minimum set: CREATE/SHOW/UNCLOAKED (add), DESTROY/HIDE/CLOAKED (remove), FOREGROUND (scroll to focused), MINIMIZESTART/END, MOVESIZESTART/END (user drag/resize), NAMECHANGE (Firefox/JetBrains launch, desktop-switch detection), LOCATIONCHANGE (self-moves, maximize). Register **narrow ranges** and filter `idObject==OBJID_WINDOW && idChild==CHILDID_SELF` before any cross-process call |
| 3.20 | **LL keyboard hook constraints** | Called on the installing thread via a message, so it needs a message loop. Must return within `LowLevelHooksTimeout` (capped at **1000 ms** since Win10 1709). On timeout, "on Windows 7 and later, the hook is silently removed". Not injected into other processes ([LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)) | A dedicated hook thread that only classifies and posts to a worker (Learn recommends this). Never do window work inside the hook |
| 3.21 | **Virtual desktops** | Public `IVirtualDesktopManager`: `GetWindowDesktopId`, `IsWindowOnCurrentVirtualDesktop`, `MoveWindowToDesktop` ([Learn](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager)). No public "current desktop" or "switch desktop" API. Whether MoveWindowToDesktop works on other processes' windows isn't stated (reported to return E_ACCESSDENIED **(unverified)**) | komorebi and FancyZones read `CurrentVirtualDesktop` from the registry (§1.7, §2.7). FancyZones detects switches with NAMECHANGE on the desktop window. Internal `IVirtualDesktopManagerInternal` (switching) is undocumented and its vtable changes across builds **(unverified)**. Recommendation: one strip per virtual desktop (swm), keyed by `GetWindowDesktopId`. Ignore windows not on the current desktop (they are shell-cloaked) |
| 3.22 | **Win11 Snap groups / taskbar** | `IsWindowArranged` (3.12) | Native taskbar is hidden in mypaper. Turn Snap off (3.17) so no window ever becomes "arranged", or un-arrange via SetWindowPlacement before tiling **(unverified that this clears arranged)** |
| 3.23 | **Tablets / touch** | — | Three-finger gestures aren't delivered to apps; komorebi relies on whkd reading the touchpad (§1.5). LeopardWM has `raw_touchpad.rs` (Raw Input on the precision touchpad HID) |
| 3.24 | **Screen recording / share** | — | Off-screen parked windows still exist and keep rendering, so window-capture (WGC) of a parked window likely still works; cloaked ones keep being composed ("still composed by DWM") **(unverified for WGC)**. Monitor-capture shows the 5 px slivers |
| 3.25 | **Restore everything on exit/crash** | — | Combine: journal (komorebi) + watchdog process (GlazeWM, LeopardWM) + original-rect file (swm). The watchdog must **uncloak** (SetCloak(1,0)), un-park, restore WS_MAXIMIZEBOX or other style edits, opacity and DWM border colour, and undo any session SPI changes. GlazeWM's watcher misses the uncloak step |
| 3.26 | **Hung apps** | `SWP_ASYNCWINDOWPOS` and `WPF_ASYNCWINDOWPLACEMENT` post to the owner thread "to prevent the calling thread from blocking" ([SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)) | Use async for everything except when the result has to be read back (min-width learning). LeopardWM probes with `SendMessageTimeout(WM_NULL, SMTO_ABORTIFHUNG)` (`placement.rs:20-21` imports) |

---

## 4. Recommendations for mypaper (from the evidence above)

1. **Scroll-out:** park adjacent to the work area, komorebi-style. Use the slack for the mypaper top bar and dock (a 1-5 px sliver can hide under the dock or bar), and cloak only where another monitor is adjacent. Animate with a DWM-thumbnail ghost host so apps aren't resized or moved per frame.
2. **Workspaces / far-away columns:** SetCloak via cached `IApplicationViewCollection`, journaled to disk. Recreate the proxy on `RPC_E_DISCONNECTED` and Explorer restart (TaskbarCreated). Have a public-API fallback (park) when `GetViewForHwnd` fails (MsoSplash, OneDrive).
3. **Filter:** GlazeWM rules + Whim system classes + komorebi's LAYERED and DLGMODALFRAME gates, all behind a data-driven rule table seeded from komorebi's 188-app YAML (that repo is MIT, `komorebi-application-specific-configuration/LICENSE`).
4. **Hooks:** narrow out-of-context WinEvent ranges on one pumping thread; WH_KEYBOARD_LL on its own thread; no reaper polling; no ForegroundLockTimeout change.
5. **UIPI:** integrity check at admission; elevated windows float, or are left alone, with a one-time hint.
6. **Snap:** session-only `SPI_SETWINARRANGING(FALSE)` (no UPDATEINIFILE), restored on exit by both daemon and watchdog; swallow Win+arrow in the LL hook.
7. **Watchdog:** a tiny separate process (or the existing mypaper core supervisor) holding the journal: cloaked HWNDs, parked HWNDs with original rects, style edits. On daemon death it uncloaks and un-parks.
8. **Licences (for reading vs copying code):** komorebi itself is under the source-available "Komorebi License 2.0.0" (`komorebi/LICENSE.md`), and GlazeWM and LeopardWM are GPL-3.0. Learn from their designs but don't copy their code. swm is MIT; the Ciantic AltTabAccessor COM declarations are MIT (`komorebi/komorebi/src/com/interfaces.rs:1-3`).
