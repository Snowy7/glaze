# Scrolling tiling for Glaze: can we replace Windows Snap with a niri-style strip?

Study of scrollable / "infinite canvas" tiling (niri, PaperWM, Karousel, Hyprland's scrolling layout,
driftwm) and whether Glaze can replace Windows' Snap with it on Win32, every edge case included.

- **Written:** 2026-11-08, on Windows 11 Pro 10.0.26300.
- **Evidence:** three source notes beside this file. Each cites the file, doc page or issue it relies on, at
  pinned commits:
  - [linux-prior-art.md](scrolling-tiling/linux-prior-art.md): niri (in depth, from its source and wiki),
    PaperWM, PaperWM.spoon, Karousel, Hyprland, scroll, papersway, driftwm and other 2D canvases, plus which
    distros ship them.
  - [windows-mechanics.md](scrolling-tiling/windows-mechanics.md): komorebi (including its Scrolling
    layout), GlazeWM, Whim, workspacer, FancyZones, Seelen UI, LeopardWM and swm, plus a 26-row Win32
    edge-case table.
  - [animation-feasibility.md](scrolling-tiling/animation-feasibility.md): a measured spike with 30 GDI
    windows and 1 flip-model window in a separate process, run at medium integrity on a live desktop. Spike
    code: `C:\mypaper-targets\tiling-research\spike\`.
- **Rule of evidence:** claims in this file point to a section of those notes. **(unverified)** marks claims
  that no primary source or measurement backs.

## 0. Verdict

**Yes.** We can replace Snap with a scrolling strip that feels like niri, using public APIs and no
injection, and it would cost nothing at rest. Two things are out of reach: we are not the compositor, so a
few behaviours can only be approximated, and some windows can't be managed at all. Both are listed in §4.

The research turned up one surprise in our favour: **it can save resources.** Chromium and Electron windows
parked outside the virtual screen classify themselves as occluded and stop rendering. GDI windows there also
stop being updated by DWM. A strip that keeps everything off-view parked outside the screen therefore makes
background browsers, Discord, VS Code and similar apps cheaper than they are under Windows' own window
management. That is the product's "save more than we use" rule working for us. See §2.3.

Prior art on Windows confirms this works. komorebi has shipped a niri-inspired Scrolling layout since
2025-05-17, and in 2026-10 it moved its animations to DWM-thumbnail "ghosts", which is the same design this
study arrives at independently ([windows-mechanics §1.5][wm15], [animation §2][af2]). Apeiron and WinCanvas do
pan and zoom over live thumbnails ([linux §2.7][lx27]). What nobody has yet is the whole package in one
product: the strip, a proper overview, the dock and bar integrated with it, crash safety, and a curated table
of per-app rules.

## 1. What "scrolling tiling" is, and which model to copy

**Model:**
- **Monitor → workspaces → strip:** each monitor has a vertical list of workspaces. Each workspace is an
  endless horizontal row of **columns**.
- **Columns:** a column holds one or more windows stacked vertically, or tabbed.
- **Opening a window:** a new window opens as a column to the right of the focused one, and nothing else
  resizes.
- **The view:** it scrolls just enough to show the focused column. Everything else waits off-screen.
- **Floating:** a floating layer sits above the strip, for dialogs and small windows
  ([linux §1.1][lx11]).

**Copy niri, not the others.** It is the only full compositor in the field, and its decisions are written
down as rules ([linux §0.1][lx0]). We adopt them as test invariants:

1. Opening a window never resizes the others.
2. The focused window never moves on its own.
3. Actions apply immediately, even mid-animation.
4. Every edge case gets a decision, even when the decision is "not supported".
5. Keep invisible state to a minimum: remember one previous view per workspace, and forget it when focus
   moves.

**2D canvases** (driftwm, where windows sit at their real size on a plane you pan and zoom) are newer and
looser. They give up the single navigation axis, so the keyboard needs nearest-window search, bookmarks and
zoom-to-fit. niri's maintainer rejected free placement inside the strip for exactly that reason
([linux §2.7][lx27]). **Recommendation:** ship the 1D strip. Make the layout axis-agnostic so portrait
monitors can scroll vertically (niri #1071, its most-requested feature). Put the "infinite canvas" feel into
the overview's zoom instead of into the layout.

**Who runs it on Linux** ([linux §2.8][lx28]):
- **KaOS:** made niri with the Noctalia shell its default in 2026.02, replacing Plasma.
- **CachyOS:** offers niri in its installer.
- **Hyprland:** moved scrolling into its core on 2026-02-21.
- **Omarchy:** uses Hyprland with the dwindle layout; scrolling is a commented-out option in its config.

## 2. How it maps onto Win32

### 2.1 What we can't do that a compositor can, and what to do instead

| A compositor (niri) can… | On Windows | Our substitute |
|---|---|---|
| Not draw off-screen windows at all | Any window overlapping a monitor shows on it | Park off-view windows **outside the virtual-screen bounding box** (§2.3) |
| Move every window per frame for free | `SetWindowPos` ≈ 0.85 ms per window (≈ 0.4 ms batched); it blocks on slow apps, and resizing every frame leaves 84 % stale pixels with NOREDRAW ([animation §1][af1]) | **Animate DWM thumbnails** in our own overlay, at about 40 µs per window per frame, with no work for the app. Then commit once with `DeferWindowPos` ([animation §2][af2]) |
| Commit a multi-window resize atomically (300 ms deadline) | No transactions across processes | One `DeferWindowPos` batch, with the proxy kept over each window until it has repainted, using a `PrintWindow` snapshot for windows being resized (komorebi's `snapshot.rs`) |
| Know a window's min/max size | `WM_GETMINMAXINFO` goes only to the app's own window procedure | **Learn it:** resize, read back `DWMWA_EXTENDED_FRAME_BOUNDS`, and widen the column if the window refused. Remember the minimum per app ([windows §0.4][wm0]) |
| Hide a window cleanly | `DWMWA_CLOAK` on another process's window returns `E_ACCESSDENIED` (measured, [animation §3][af3]) | Parking. The undocumented `IApplicationView::SetCloak` stays an opt-in experiment (§2.3) |
| Throttle off-screen clients (niri: about 1 Hz frame callbacks) | No control over other apps' rendering | Parking outside the screen gets the same result for Chromium/Electron and GDI windows (§2.3) |
| Receive three- and four-finger touchpad gestures | Windows keeps them for itself | Read the precision touchpad over Raw Input in our own process, as komorebi's whkd and LeopardWM do, and ask the user to set Windows' own gestures to "nothing" ([windows §3.23][wm3]) |
| Scale real windows (driftwm zoom) | Impossible | The overview zooms thumbnails, which is how Apeiron does it |

### 2.2 Architecture that fits Glaze's rules

| Piece | Where | Notes |
|---|---|---|
| **Layout model** | New pure crate, e.g. `crates/tiling` (no `unsafe`, no Win32) | State: monitors → workspaces → columns → tiles, plus the view offset. Operations return target rectangles. Property tests over random operation sequences (add or remove monitors and DPI scales, arbitrary min/max sizes, owner loops), asserting the invariants after every step, as niri's `layout/tests.rs` does ([linux §1.13][lx113]) |
| **Window intake** | The shell's existing `win-events` thread (`crates/desktop/src/win_events.rs`) | Narrow event ranges only: CREATE/SHOW/UNCLOAKED, DESTROY/HIDE/CLOAKED, FOREGROUND, MINIMIZESTART/END, MOVESIZESTART/END, NAMECHANGE, and LOCATIONCHANGE filtered through a hash-set check first. No polling and no reaper thread (komorebi polls every 20 ms; we don't). Never hook the full event range ([windows §2.3, §3.19][wm23]) |
| **Keys** | Extend the existing `WH_KEYBOARD_LL` in `crates/bars/src/flyouts/winkey.rs` | It already masks Win-key combinations away from Explorer. Swallow Win+arrow (as FancyZones does) plus our own bindings. The hook only classifies keys and posts them on; it never does window work. Because the hotkey is the last input, `SetForegroundWindow` from the handler is allowed without changing the user-wide foreground lock timeout ([windows §3.18][wm3]) |
| **Animation and overview** | Shell process, a small thread paced by `DwmFlush` | Same mechanism as the dock previews (`crates/bars/src/preview.rs`). The overlay exists only while animating. Frames stop when the spring settles. Rounded corners and shadows are drawn by us, because thumbnails have neither ([animation §2][af2]) |
| **Crash safety** | Undo journal (`crates/tweaks/src/undo.rs` pattern) plus the core supervisor | Before parking, cloaking, removing Snap or changing a style, record what is needed to undo it. Restore when the tiler starts, and from core if the shell dies. GlazeWM's watcher failed because it forgot to uncloak (#1358); ours restores everything it changed ([windows §3.25][wm3]) |
| **Snap** | Session-only `SPI_SETWINARRANGING(FALSE)` without `SPIF_UPDATEINIFILE`, plus Win+arrow swallowed | Nothing persists past a crash or logoff. Restored on exit. Alternative: Karousel's "pinned" model, where a window snapped natively carves its rectangle out of the strip (§3) |
| **App rules** | Data file (JSON), seeded from komorebi's MIT-licensed `applications.yaml` (188 apps) | Float, ignore, tray-and-multi-window, late-title, and layered rules. Users will hit the missing ones first ([windows §1.8][wm18]) |
| **Flag** | `experimental.tiling`, off by default | Per the placement rule: new and unproven features are experimental toggles |

**Licences:** komorebi's own code is source-available, and GlazeWM and LeopardWM are GPL-3.0. Read their
designs; don't copy their code. komorebi's app list and swm are MIT ([windows §4.8][wm4]).

### 2.3 Off-view windows: park outside the virtual screen

The notes disagree on one point, which we resolve here. komorebi parks windows **at the work-area edge with
5 px still visible**, and windows-mechanics recommended that. The spike's evidence favours parking **outside
the bounding box of all monitors**:

| | Park 5 px at the edge (komorebi) | **Park outside the virtual screen** | `SetCloak` (undocumented) | Minimize | `SW_HIDE` |
|---|---|---|---|---|---|
| Chromium/Electron stop rendering | no: they count as visible | **yes: OCCLUDED** (Chromium `native_window_occlusion_tracker_win.cc` starts from the `SM_*VIRTUALSCREEN` rectangle; re-checked 2026-11-08) | probably not; Chromium only treats cloaks from other desktops as occluded **(unverified)** | yes (HIDDEN) | yes |
| DWM updates GDI content | yes (sliver) | **no** | yes | no | no |
| Thumbnail for the overview | live | GDI: last frame; DX: live | live | last frame | last frame |
| Taskbar, Alt-Tab and window state unchanged | yes | **yes** | yes | no | no (button gone) |
| Public API, no state left behind | yes | **yes** | no; breaks on Explorer restart or new builds | yes, but animates | yes; breaks Electron and Excel |
| Visible artefact | a 5 px border line | **none** | none | restore animation | — |

**Decision:** park just beyond the bounding box of all monitors, not at ±32 000. Values past ±32 767 wrap in
`WM_MOVE`'s 16-bit fields ([windows §3.16][wm3]). Shaped multi-monitor layouts have holes inside the bounding
box, so the box itself is the boundary ([animation §3][af3]).

**What can go wrong, and what we do:**
- **Display change:** Windows or the app may pull a parked window back onto a monitor. We treat that as
  "moved by itself", re-park it, and after repeated fights float the window and record a rule. Karousel gives
  up after 4 external moves in 1 s ([linux §2.3][lx23]).
- **Alt-Tab or activation from outside:** follow `EVENT_SYSTEM_FOREGROUND` and scroll to the window.
- **Overview stale frames:** parked GDI windows show a frozen frame in the overview. We accept that; niri
  shows off-screen windows at 1 Hz too.
- **Screen sharing a parked window:** for GDI windows WGC delivers 0 frames
  ([animation §2][af2]). niri has the same problem (#539). An "un-park while captured" rule would need
  capture detection, which no Win32 API offers **(unverified)**.

## 3. "And more": what we can do that the Linux ones don't

1. **The wallpaper scrolls with the strip.** Glaze owns the wallpaper engine, so the background can
   parallax-pan with the view and zoom with the overview. niri gets this only for layer-shell backgrounds.
2. **The dock and bar know the strip:**
   - a PaperWM-style position indicator in the top bar;
   - a dock click scrolls to the column;
   - minimize removes the column and closes the gap. niri has no minimize (disc. #3982) because it has no
     dock; we have one ([linux §3, Minimize][lx3]).
3. **Coexisting with Snap the way Karousel does:** a window the user snaps with Windows' own layouts becomes
   a pinned rectangle, and the strip uses the largest free area left ([linux §2.3][lx23]). It's a smoother
   migration than turning Snap off cold.
4. **Learned app behaviour persists:** minimum widths, late titles and rules discovered for "self-movers" are
   saved per app, so the second launch is right first time.
5. **Thumbnail-first overview:** live thumbnails zoom out of every workspace on every monitor. Windows drag
   between workspaces, and dropping a window between two workspaces creates a new one (as niri does). This
   costs about 1.2 ms per frame for 30 windows, and nothing while it holds still ([animation §2][af2]).
6. **Per-monitor direction** (vertical strips on portrait monitors), which niri still lacks.

## 4. Edge cases

Each row says what we do and where the evidence is. The source notes carry the citations.

| Case | Decision | Evidence |
|---|---|---|
| **Which windows tile** | Root, visible, not cloaked, has a caption, not `WS_CHILD`, `WS_EX_TOOLWINDOW` or `WS_EX_NOACTIVATE`, not shell classes (Progman, WorkerW, `Shell_TrayWnd`, our own windows), then the app rules. `WS_EX_LAYERED` only by rule. Manage the `ApplicationFrameWindow`, never the `CoreWindow` | windows §1.2, §2.2, §3.6 |
| **Dialogs / owned windows** | Float centred over the owner, on the owner's workspace, z-ordered with it, focused only if the owner had focus. When a dialog opens for a parked owner, scroll the owner into view first: owned windows don't follow the owner off-screen. Owned windows with no caption (autocomplete lists, menus) are ignored | linux §1.5; windows §3.9 |
| **Menus, tooltips, popups** (`#32768`, `WS_POPUP`) | Never managed. Before menus can open, the focused window is fully on screen (scroll on activation) | linux §3 |
| **Fixed-size windows** | Float if either axis can't resize (no `WS_THICKFRAME`, or a refused resize), as Hyprland does. Override by rule | linux §3; windows §3.2 |
| **Min size bigger than the column** | Grow the column to the observed size, capped at the work area. Left-align a column wider than the view. Never shrink back automatically | linux §1.2; windows §0.4 |
| **Apps that move or resize themselves** (Firefox at startup, browsers during tab tear-off) | Re-apply the layout, debounced. After 4 fights in 1 s, accept the window's size as its column width. Repeat offenders float and get a recorded rule | linux §2.3; windows §3.4 |
| **Late title / class** (Firefox, JetBrains send only NAMECHANGE) | Admit windows on NAMECHANGE too. Rules are re-checked when the title changes | windows §1.1 |
| **Maximize** (button, double-click, `SC_MAXIMIZE`) | Becomes "maximize column to edges" and stays in the strip. Restore returns the previous width. Without native Snap, the Snap Layouts flyout over the maximize button goes too | linux §1.6; windows §3.17 |
| **Fullscreen apps and games** (rect covers the monitor, no caption; `SHQueryUserNotificationState` checked on foreground change) | **Hands off:** take the window out of the strip, never move or park it, never steal focus from it, and pause the tiler's overlays. When it leaves fullscreen, return it to its old column (niri #426 shows users want this) | linux §1.6; windows §3.11 |
| **Always-on-top / PiP** (`WS_EX_TOPMOST`) | Never tiled. Fixed to the screen above the strip, kept across workspace switches. Ship PiP rules for Firefox, Chrome and Edge. Never make tiled windows topmost (it spreads to owned windows) | linux §3; windows §1.3 |
| **Splash screens** (`MsoSplash`, JetBrains splash) | Float without focus. Caught by the fixed-size and no-caption rules, plus the rule list | windows §3.10 |
| **Tray / multi-window apps** (Discord, Steam, Telegram) | HIDE means "closed to tray": drop the column and keep the app's rule. HIDE from a window we hid ourselves is ignored | windows §1.1 |
| **Electron transient windows** | `Chrome_RenderWidgetHostHWND` is always ignored. Short-lived `Chrome_WidgetWin_1` popups are suppressed (LeopardWM's recently-hidden list) | windows §3.7 |
| **New window placement** | Right of the focused column. A window with an owner opens next to its owner. Focus it only if the user launched it, or its owner was active. Never take focus from fullscreen. A background `SetForegroundWindow` makes the dock icon urgent instead | linux §1.4, §3 |
| **New window while the overview is open** | Insert it and animate it in. It gets focus when the overview closes | linux §1.10 |
| **Closing the focused column** | Return to the column it was opened from, with the old view, if focus hasn't moved; otherwise the right neighbour. Closing a column to the left doesn't move the focused window | linux §1.7 |
| **Minimize** | The column leaves the strip and the gap closes. Restoring puts it back to the right of the focused column. Minimizes during the 10 s after a display change are ignored (komorebi's grace period) | windows §1.1, §3.12 |
| **Arranged (snapped) windows** | Covered by row 3 of §3, or by turning Snap off. Use `IsWindowArranged`, loaded with `GetProcAddress` | windows §3.12, §3.22 |
| **Monitor hotplug** | The workspaces of a removed monitor move to a surviving one and remember their monitor, keyed by EDID or device path rather than index. They return when it reconnects, unless a new window was opened on them meanwhile (niri's rule). Zero monitors is a valid state. `WM_DISPLAYCHANGE` bursts are debounced | linux §1.8; windows §1.7 |
| **Mixed DPI** | Widths are stored as proportions of each monitor's logical size. Moving between DPIs sets the position twice (GlazeWM and Whim do), and a column stays wholly on one monitor while it settles | linux §1.8; windows §2.6, §3.5 |
| **Invisible resize borders** | Layout uses `DWMWA_EXTENDED_FRAME_BOUNDS`. The difference from `GetWindowRect` is added to targets and cached per window | windows §3.1 |
| **Elevated windows (UIPI)** | A medium-integrity shell can't move them. Check integrity when the window is admitted and leave higher-integrity windows floating, with a one-time hint. An elevated companion or UIAccess is a possible later step **(unverified cost)** | windows §0.7, §3.8 |
| **Hung apps** | Probe with `SendMessageTimeout(WM_NULL, SMTO_ABORTIFHUNG)`. Hung windows get `SWP_ASYNCWINDOWPOS` and are skipped by the min-size learning | windows §3.26 |
| **UWP / WinUI3** | Manage the frame. The process is read from the `CoreWindow` child. Cloaked ghost frames are ignored | windows §3.6 |
| **Virtual desktops** | One strip set per virtual desktop, keyed by the public `IVirtualDesktopManager::GetWindowDesktopId`. Windows on other desktops are shell-cloaked and ignored. A desktop switch shows up as NAMECHANGE on the desktop window | windows §2.7, §3.21 |
| **User drags a title bar** | `MOVESIZESTART` means the window pops out after a rubber-band threshold, with an insert hint drawn by us. Drop near a column edge (30 %) for a new column; drop inside a column to join it. At the screen edge the strip scrolls (30 px band). Resizing a column edge resizes the column | linux §1.9, §2.4 |
| **Focus follows mouse** | Off by default. If enabled, it never scrolls on hover (niri's `max-scroll-amount 0%`). Clicks scroll on release | linux §3 |
| **Touchpad and touch** | Three-finger horizontal swipe scrolls with momentum and snaps to a column (niri's projection). Three-finger vertical switches workspace; four-finger opens the overview. Read via Raw Input | linux §1.9; windows §3.23 |
| **Win-key conflicts** | Bindings sit under one modifier family. LeopardWM found 12 of 33 Win+Alt combinations taken by Game Bar or Snap. Win+L and Ctrl+Alt+Del can't be taken (secure attention) **(unverified)** | windows §2.8, §3.17 |
| **Explorer restart** | Re-subscribe hooks. Any `SetCloak` proxy is recreated after `TaskbarCreated`. Parked windows are unaffected | windows §2.1 |
| **Crash or exit** | The journal restores every parked window to an on-screen rectangle, every cloak, every style edit and Snap. Run on next start and from the core supervisor | windows §3.25 |

**Not supported, deliberately:**
- **Exclusive-fullscreen games:** left alone.
- **Windows of other users or sessions:** out of scope.
- **Protected/PPL and anti-cheat processes:** skipped (LeopardWM does the same).
- **Elevated windows:** not managed until there is an elevated design.

## 5. Implementation checks (9 November 2026)

The original spike checks remain release gates. See [the implementation guide](../tiling.md#validation-and-limits)
and [raw measurements](scrolling-tiling/measurements-2026-11-09.json). All live runs used only own fixture
HWNDs at medium integrity, no input or foreground requests, private journals/data and an unchanged
foreground check. Scoped runs did not alter Snap. No user's desktop build was replaced or stopped.

| Check | Measured evidence / remaining work |
|---|---|
| Display hotplug with parked windows | Pure migration/reconnect/zero-output and floating/DPI regressions pass. Physical unplug/replug and app pull-back frequency remain untested. |
| Real Chromium/Electron, Office, UWP and WinUI3 parking | GDI fixture intake/parking/restoration and own overview capture pass. Real-client occlusion CPU/GPU savings remain unmeasured; no new claim replaces the spike evidence. |
| 30 live thumbnails and DWM cost | Three runs each at 4/10/30 windows: update mean 1.398/1.606/2.871 ms; p95 2.356/2.592/3.948 ms. Interval p95 9.713/10.290/13.050 ms on a 200 Hz output, while DWM reports a 240 Hz primary. Shared-desktop pacing and isolated DWM/GPU attribution remain open. |
| Rounded proxies and shadows | Own overlay regions clip/crop proxies per output, preserving source scale. Own overview/tiled and layered resize-source/proxy captures inspected. Proxy shadows and a repaint-acknowledged fade are not implemented; client-specific corners remain unverified. No application's region is changed. |
| LL hook with elevated foreground | Reserved bindings and hook state-machine tests pass; actual elevated-foreground key delivery remains untested. Elevated clients are filtered before admission. |
| Session Snap suppression | `SPI_SETWINARRANGING` without `SPIF_UPDATEINIFILE`, with journal restoration, is implemented. Scoped tests deliberately skip the global change; flyout/drag-to-edge behavior remains unverified. |
| Idle and disabled overhead | Adapter flag-on: 60.006 s, 0 CPU ms, 0 UI wakes, 2,068,480 private bytes. Flag-off: 60.005 s, 15.625 CPU ms, 0 UI wakes, 1,560,576 bytes. No whole-shell baseline/wakes A/B was taken, so flag-off equivalence with main is unproven. |
| Native correctness and recovery | Own window admission, minima, tray/minimize, hung client, native fullscreen, topmost ordering, overview and exact disable restoration pass. A renamed hard-crash fixture/new recovery process restores four parked windows. Core-death and panic-hotkey wiring were not invoked on the owner's shell. |

Layout tests cover 38 cases, including randomized operation sequences and saved counterexamples. Full
hardware/real-client sign-off and the performance gates above remain open. Gesture input and wallpaper
parallax were withheld while pacing is unresolved; their incremental cost has not been measured.

## 6. Proposed phases

1. **Layout crate and property tests:** pure, with no Win32. The invariants from §1.
2. **Intake and rules:** the window filter, app-rules file, journal and Snap off. No animation yet: rectangles
   are applied directly.
3. **Thumbnail animation and overview,** reusing the dock-preview mechanism.
4. **Parking and measurement:** parking, plus the §5 measurements, then a decision on whether parking stays
   the default.
5. **Shell integration:** bar indicator, dock, wallpaper parallax, and touchpad gestures.

[lx0]: scrolling-tiling/linux-prior-art.md#0-the-ten-findings-that-matter-most-for-glaze
[lx11]: scrolling-tiling/linux-prior-art.md#11-model
[lx113]: scrolling-tiling/linux-prior-art.md#113-how-niri-tests-the-layout
[lx23]: scrolling-tiling/linux-prior-art.md#23-karousel-kde-kwin-script
[lx27]: scrolling-tiling/linux-prior-art.md#27-2d-infinite-canvas-wms
[lx28]: scrolling-tiling/linux-prior-art.md#28-who-ships-it-distros
[lx3]: scrolling-tiling/linux-prior-art.md#3-edge-case-catalogue
[wm0]: scrolling-tiling/windows-mechanics.md#0-the-findings-that-matter-most-for-glaze
[wm15]: scrolling-tiling/windows-mechanics.md#15-scrolling-layout
[wm18]: scrolling-tiling/windows-mechanics.md#18-app-specific-configuration-188-apps-komorebi-application-specific-configurationapplicationsyaml
[wm23]: scrolling-tiling/windows-mechanics.md#23-hooks-none-of-these-inject-except-seelens-tray-hook
[wm3]: scrolling-tiling/windows-mechanics.md#3-win32-edge-case-catalogue
[wm4]: scrolling-tiling/windows-mechanics.md#4-recommendations-for-glaze-from-the-evidence-above
[af1]: scrolling-tiling/animation-feasibility.md#1-moving-real-foreign-windows-per-frame
[af2]: scrolling-tiling/animation-feasibility.md#2-proxy-animation-dwm-thumbnails-in-our-overlay
[af3]: scrolling-tiling/animation-feasibility.md#3-keeping-off-view-windows
[af5]: scrolling-tiling/animation-feasibility.md#recommendation
