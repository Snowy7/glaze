# Scrollable tiling on Windows: can the animations be smooth and cheap?

Spike for mypaper, 2026-11-08. Question: can a niri-style strip scroll / Overview animate
*other processes'* windows smoothly and cheaply, and how should off-view windows be kept?

**Short answer.** Moving real foreign windows every frame works only for small counts and stays
smooth only while the apps keep up. It costs about 0.4–0.85 ms of CPU per window per frame, and
that cost lands on whichever thread does the move. It blocks on slow or hung apps. Resizing
every frame is visibly bad. Animate **DWM thumbnails** of the windows in our own overlay
instead: about 40 µs per thumbnail per frame on our thread, nothing asked of the apps, and the
update shows in the next DWM frame. Then commit the real layout once, with one
`DeferWindowPos` batch. Park off-view windows **outside the virtual-screen rectangle**:
Chromium/Electron then mark themselves occluded, and DWM stops updating GDI windows there. Do
not minimize or `SW_HIDE` them. Cross-process `DWMWA_CLOAK` is refused (measured). The
undocumented `IApplicationView::SetCloak` (komorebi's and GlazeWM's default) is the only real
cloak, and it is fragile.

---

## Method

- **Spike**: `C:\mypaper-targets\tiling-research\spike\` (a standalone Rust crate, `windows`
  0.62, with its own target dir). `tiling-spike run <report> [sections]` spawns
  `tiling-spike child …` as a **separate process**. The child is the "foreign app": 30 plain
  GDI `WS_OVERLAPPEDWINDOW` windows plus 1 window drawn through a **flip-model DXGI swap
  chain** (FLIP_DISCARD, like Chromium, games and WinUI). Each window fills its client area
  with a colour that encodes its index. With `anim`, it repaints every 16 ms (about 36 paints/s
  per window).
- **Safety**: every HWND is checked with `GetWindowThreadProcessId == child pid` before it is
  touched. All windows are `WS_EX_NOACTIVATE`, shown with `SW_SHOWNOACTIVATE`, with no input
  and no activation. The child runs in a kill-on-close Job object and is quit or killed in
  `Drop`. The tests ran on the monitor the foreground app was **not** on (DISPLAY2,
  2560×1440 @ 200 Hz; the primary is 240 Hz).
- **Integrity**: runs 2–3 used `runas /trustlevel:0x20000`, i.e. **medium integrity**,
  like a real non-elevated shell. The report confirms RID 0x2000. Because of this, dwm.exe's CPU
  could not be read (OpenProcess denied), so the DWM-side cost is only sampled as GPU use (PDH
  `GPU Engine` counters), which was noisy.
- **Pacing**: one batch per `DwmFlush()`. On this machine `DwmFlush` paced at about 4.2 ms
  (the 240 Hz primary), even for windows on the 200 Hz monitor. A frame counts as "missed"
  when the interval was over 7.5 ms (1.5× the 200 Hz period).
- **What DWM actually composed**: after each `DwmFlush`, one screen row was read with
  `GetDC(NULL)` + `BitBlt` (about 4.3 ms per read, so these passes run at about 50 Hz), and
  the colour edges were compared with the commanded positions. The pixels were only checked in
  memory for our own colours and never stored.
- **Live desktop**: mypaper shell, T3 and other WinEvent/UIA clients were running throughout,
  so these are real-world numbers, not a clean lab. Process CPU from `GetProcessTimes` has
  15.6 ms granularity, so "ms/s" figures over 1–2 s carry about ±8 ms/s.
- Raw output: `spike\report.txt` (run 1, elevated launcher, see the note at the end),
  `spike\report2.txt.log` (run 2), `spike\report3.txt.log` (run 3, after fixing a test
  artefact).

## 1. Moving real foreign windows per frame

Run 2, cross-process, N windows moved every frame. Columns: our batch wall time (mean), our
thread's CPU, the share of frames that missed, and the child app's CPU.

| N | mode | batch µs | our Mcycles | missed | app CPU ms/s |
|---|---|---:|---:|---:|---:|
| 4 | `SetWindowPos` sync, move only | 3 412 | 10.3 | 38 % | 31 |
| 4 | + `SWP_NOREDRAW` | 2 429 | 6.9 | 5 % | 23 |
| 4 | komorebi flags (NOSENDCHANGING, NOCOPYBITS, NOREDRAW, DEFERERASE) | 2 280 | 6.8 | 4 % | 26 |
| 4 | `SWP_ASYNCWINDOWPOS` | 653 | 0.24 | 0 % | **542** |
| 4 | `DeferWindowPos` batch | 1 661 | 3.7 | 2 % | 71 |
| 10 | sync | 8 623 | 25.4 | 99 % | 52 |
| 10 | async | 1 843 | 0.65 | 22 % | **770** |
| 10 | defer | 4 436 | 6.6 | 60 % | 83 |
| 30 | sync | 31 882 | 83.7 | 97 % | 31 |
| 30 | defer | 11 998 | 15.0 | 98 % | 197 |
| 4 | move+resize, sync | 6 920 | 14.3 | 93 % | 104 |
| 4 | move+resize, defer | 4 225 | 7.3 | 72 % | 114 |
| 10 | move+resize, defer | 10 580 | 16.1 | 95 % | 155 |
| 30 | move+resize, defer | 30 969 | 43.3 | 100 % | 255 |

Our own same-thread windows cost the same (run 1 §1c: N=10 sync move 8.6 ms, defer 3.7 ms).
So **the cost is the move itself, not the process boundary**: about 0.85 ms wall and about
2.5 Mcycles (about 0.55 ms CPU) per top-level `SetWindowPos`, and about 0.4 ms per window
batched.

What this means:

- **Async does not make moves cheaper. It moves the cost into the app.** The app burned
  540–770 ms of CPU per second (most of a core) to apply about 1 000 posted moves per second.
  After the next `DwmFlush`, 94–99 % of async windows were not yet at their target. In the
  composed-row pass, **101 of 126 frames showed windows at different lags** (25–34 % of
  samples one frame behind). That is tearing between windows. The Learn docs say async "posts
  the request to the thread that owns the window. This prevents the calling thread from
  blocking" ([SetWindowPos][swp]). komorebi uses async only to avoid hanging on unresponsive
  apps (`komorebi/src/windows_api.rs` ~L518).
- **Sync and Defer are coherent.** In the composed pass, 100 % of samples showed the commanded
  position in the very next composition, with no window lagging another (0/126 frames). Learn:
  EndDeferWindowPos "updates … one or more windows in a single screen-refreshing cycle"
  ([EndDeferWindowPos][edwp]). Caveat: at the pass's ~50 Hz the batch always finished well
  before the vsync, so a full-rate sync loop longer than one frame (N≥10 at 240 Hz) would still
  split across vsyncs. The batch is not proven atomic.
- **Both block on the app.** A WM_PAINT that takes 8 ms or 30 ms barely slows *move-only*
  (the moves are sent messages, and paints come later). With **move+resize** it does: frame
  time p95 reached 150 ms (8 ms paint) and 415 ms (30 ms paint) for sync and defer. Async stayed
  at about 6 ms per frame for us, but the windows trail behind.
- **Resizing every frame is ugly.** Apps repaint 0.35–0.6 times per resize frame. After
  resizing, `SWP_NOREDRAW` or the komorebi flag set left **84 % of the client row showing stale
  or garbage pixels** until the app was made to repaint (run 3 §1r: sync, async and defer
  without NOREDRAW: 100 % correct). Never use NOREDRAW or NOCOPYBITS for live resizes.
- **Budget**: at 240 Hz a frame is 4.2 ms. Moving even the 2–4 windows a strip shows uses most
  of a frame on one core (or most of a core inside the apps, with async). For a 250 ms scroll
  that is affordable once, but it is not "cheap", and it cannot scale to an Overview of 30
  windows.

Prior art: komorebi animates with per-window `SetWindowPos` threads in its legacy path and now
uses ghost thumbnails (below). Whim's "DeferWindowPosHandle" actually calls `SetWindowPos` per
window in `Parallel.ForEach` (`Whim/src/Whim/Native/DeferWindowPosHandle.cs`). GlazeWM
positions per window.

## 2. Proxy animation: DWM thumbnails in our overlay

Our own topmost `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE` overlay, with one
`DwmRegisterThumbnail` per foreign window. Each frame, one `DwmUpdateThumbnailProperties` call
per thumbnail moves and scales its `rcDestination` (an Overview-like zoom and translate).

| N thumbnails | update µs/frame (mean / p95) | missed frames |
|---|---:|---:|
| 4 | 178 / 214 | 0 % |
| 10 | 416 / 493 | 24 %* |
| 30 | 1 223 / 1 388 | 0 % |

\*This run also did a 4 ms screen read every 4th frame.

- That is **about 40 µs per thumbnail per frame**, about 20× cheaper than moving the window,
  **all of it in our process**, with **zero work for the app** and no blocking on hung apps. 44
  of 46 matched samples showed the thumbnail in the very next composition (2 lagged by 2
  frames).
- **Looks**: the thumbnail is square-cornered and has **no shadow**. In the run-3 corner test,
  pixel (0,0) of a whole-window thumbnail cropped to `DWMWA_EXTENDED_FRAME_BOUNDS` was window
  content, and no darkening showed beside it. Rounded corners and shadows have to come from us.
  A thumbnail is drawn by DWM into the destination window, which we cannot clip with our own
  DirectComposition content in the same window. Shaping the destination window's region to
  clip them is **(unverified)**.
- **Constraints** ([DwmRegisterThumbnail][dwmrt]): the destination must be top-level and owned
  by the calling process, and the source must be top-level. Fine for an overlay.
- **Liveness by source state** (12 samples over about 1.2 s while the source repainted;
  "frozen" = the last frame stays shown):

| source state | GDI window | flip-model window |
|---|---|---|
| on screen (under our overlay) | live | live |
| entirely off every monitor | **frozen** | **live** |
| 1 px sliver on a monitor | frozen (centre) | live |
| minimized (`-32000,-32000`) | frozen | frozen |
| `SW_HIDE` | frozen (still drawn!) | frozen |
| cloaked by its own process | live | live |
| layered alpha-0 "veil" set cross-process | live | live |

  So the task's assumption ("minimized ones don't render") is half right. The thumbnail still
  draws the **last frame**; it just stops updating.
- **Windows.Graphics.Capture** as the alternative (frames/s from a window repainting at ~36 Hz):

| state | GDI | flip-model |
|---|---:|---:|
| on screen | 35 | 31 |
| off every monitor | **0** | 35 |
| 1 px sliver | 35 | 35 |
| minimized | 0 | 0 |
| `SW_HIDE` | 0 | 0 |
| self-cloaked | 38 | 33 |

  Costs: **+51 MB private for a D3D11 device** (mypaper's overlay doc measured +16 MB for a
  device on wgpu's adapter, `crates/wallpaper/src/overlay/capture.rs`) and about +0.2 MB per
  session. The first frame arrives after 83–282 ms (mypaper's overlay docs: 100–250 ms). Our
  CPU stayed within measurement noise for 11 sessions. **No yellow border**:
  `RequestAccessAsync(Borderless)` returned Allowed and `SetIsBorderRequired(false)` succeeded,
  as mypaper's overlay already does (`capture.rs` `ask_borderless`). WGC only hands over
  textures, so we would composite them ourselves (DComp/wgpu), with GPU copies and our own
  device. That only pays off if the proxies need effects (rounded corners, shadows, blur),
  which mypaper's overlay pipeline can already draw.
- **DirectComposition**: the documented DComp path to another window's content is only for
  *your own* layered child windows ([DWMWA_CLOAK note][dwma]; "How to animate the bitmap of a
  layered child window"). There is no public DComp visual for a foreign HWND. The shell's
  private shared-thumbnail visuals (`DwmpCreateSharedThumbnailVisual` and similar) are
  **(unverified)** and off-limits.
- **Task View / Alt-Tab**: Explorer's `MultitaskingViewFrame` window (the class name is
  confirmed by Chromium's occlusion tracker, which treats Alt-Tab as "show all windows").
  How it draws its thumbnails is undocumented; it is believed to use private DWM thumbnail
  visuals **(unverified)**. Virtual desktops hide windows with a shell cloak
  (`DWM_CLOAKED_SHELL`, [DWMWINDOWATTRIBUTE][dwma]).
- **komorebi already ships this design** (commit 72aa4c9, 2026-10-30):
  `animation/ghost.rs` composes "every thumbnail … onto one shared host window that covers the
  whole virtual screen", with one `DwmUpdateThumbnailProperties` per ghost per frame.
  `animation/snapshot.rs` explains why the ghost is a *snapshot*
  (`PrintWindow(PW_RENDERFULLCONTENT)` into a layered window of its own, then thumbnailed)
  rather than a live thumbnail: "a window that has just been resized paints a blank frame, or
  nothing at all". `animation/ghost_dispatcher.rs` cloaks the real window (`SetCloak`), resizes
  it during the animation, and reveals it at 85 % under the fading ghost. `animation/engine.rs`
  paces frames with `DwmFlush`.

**Overview**: thumbnails of all managed windows scaled into one overlay cost about 1.2 ms per
frame for 30 windows during the zoom and nothing while it holds still. Windows that are visible
or flip-model stay live. GDI windows parked off-screen show their last frame. Chromium windows
parked off-screen will have throttled themselves (§3), so they show a recent but stale frame.
That is acceptable for an Overview.

## 3. Keeping off-view windows

Measured on our own windows (run 2 §3, 10 windows repainting, 2 s per state, interleaved with
on-screen):

| state | app WM_PAINTs/s | thumbnail | WGC | notes |
|---|---:|---|---|---|
| on screen | ~355 | live | live | |
| outside the virtual screen | ~359 (GDI apps keep painting) | GDI frozen / DX live | GDI 0 / DX 35 fps | `IsWindowVisible` true, taskbar/Alt-Tab unchanged |
| minimized | ~349 | frozen | 0 | rect -32000; minimize/restore animations and state changes |
| `SW_HIDE` | **0** | frozen | 0 | taskbar button and Alt-Tab entry disappear |
| self-cloaked (own process only) | ~350 | live | live | DWM keeps composing ("The window is still composed by DWM", [DWMWA_CLOAK][dwma]) |
| alpha-0 layered veil (cross-process) | ~350 | live | live | still composed; intrusive |

- **dwm.exe GPU** sat at a noisy 13–16 % baseline here (the other monitor at 240 Hz, wallpaper,
  T3), and swung ±2 points between identical states. 10 small repainting windows did not add a
  difference above that noise in any state, so **the GPU difference between parking strategies
  could not be measured on this live machine**. What *could* be measured is whether DWM
  processes the content at all (the thumbnail and WGC columns). DWM does no content work for GDI
  windows outside every monitor, or for minimized or hidden windows. It keeps composing
  flip-model windows anywhere, and cloaked or veiled windows.
- **Chromium/Electron** (`ui/aura/native_window_occlusion_tracker_win.cc`, main): it starts
  from the virtual-screen rectangle (`SM_XVIRTUALSCREEN`/`SM_CXVIRTUALSCREEN`) and subtracts
  windows in z-order, so a window whose rect lies **outside the virtual screen is OCCLUDED**.
  Minimized windows are HIDDEN. Cloaked windows are OCCLUDED only when the
  `VirtualDesktopManager` says they are on another desktop. A window cloaked by `SetCloak` on
  the current desktop would then go through geometry and could stay VISIBLE, so it would keep
  rendering **(unverified)**. While Alt-Tab (`MultitaskingViewFrame`) or taskbar thumbnails show,
  every window is treated as VISIBLE. So **off-screen parking gives the net resource saving for
  the heaviest apps**, and it beats cloaking on that. Caveat: the bounding rectangle of an L- or
  gap-shaped monitor layout contains areas that are on no monitor, so park beyond the bounding
  box itself.
- **Cross-process `DWMWA_CLOAK` is refused**: `E_ACCESSDENIED (0x80070005)` from a
  same-user, same-integrity process, and the window stayed uncloaked. This matches mypaper's
  own note in `crates/bars/src/taskbar.rs` (`veil`). The child cloaking *itself* worked.
- **`IApplicationView::SetCloak`** (undocumented immersive-shell COM): komorebi's
  `HidingBehaviour::Cloak` (`komorebi/src/core/mod.rs`, `com/mod.rs`) and GlazeWM's default
  `hide_method: cloak` (`wm-platform/.../native_window.rs` `set_cloak(1, 2)`, which cites
  AltTabAccessor issue #1). Risks: the interface IIDs change between Windows builds; it needs
  the ImmersiveShell service from Explorer; it depends on Explorer, which mypaper may replace;
  and windows stay cloaked if the WM dies. komorebi keeps an on-disk record of the windows it
  cloaked to undo after a crash (`cloaked.rs`). GlazeWM also hides taskbar buttons separately.
  Not tested here (off-limits).
- **komorebi's verdicts**: `Hide` (SW_HIDE) is "END OF LIFE … has issues with Electron
  apps"; `Minimize` "has issues with frequent workspace switching". GlazeWM's
  `PlaceInCorner` keeps a 1 px sliver on the monitor. That is the opposite of what we want: our
  test shows a sliver keeps GDI content live in WGC and counts as on-screen for occlusion
  logic.
- **Off-screen risks, (unverified) on this spike**: display topology changes (Windows or apps
  may pull windows back onto a monitor); apps that clamp themselves to the work area on
  `WM_DISPLAYCHANGE`/DPI changes; Alt-Tab or a taskbar click activating an off-screen window
  (the tiler must follow `EVENT_SYSTEM_FOREGROUND` and scroll to it, which is niri behaviour
  anyway); windows stranded after a tiler crash (keep a parked-windows record like komorebi's
  `cloaked.json`, and restore on start and exit). No display change was triggered.
- **UIPI**: a medium-integrity tiler cannot reposition elevated apps' windows
  **(unverified here)**; every run above was medium against medium.

## 4. Where it lives in mypaper, and its budget

- **Budget** (`docs/performance.md`): CPU while paused 0.02 % per process; idle bars 0.2 %; no
  frame ticker at rest; shell resting 35–37 MiB private; "Hidden = free". The rule is to save
  more than we use (net-negative resources). So the tiler must be **event-driven at rest**, with
  zero timers, and must animate only during a scroll or Overview.
- **Events**: the shell already has one WinEvent source on its own `win-events` thread that
  batches events and wakes the UI thread once per batch (`crates/desktop/src/win_events.rs`).
  It was moved off the UI thread because location hooks follow the foreground app and every
  cursor move over it arrives as an event (`docs/performance.md`, "Shell idle hill-climb":
  1 533 → 89 Mcycles per 15 s of mouse motion). The tiler should subscribe there
  (show/hide/destroy/foreground/move-size-end/cloaked). It should not hook location changes of
  every managed window.
- **Thumbnails**: the shell already registers live DWM thumbnails into a native popup for dock
  previews (`crates/bars/src/preview.rs`), with no capture and no device. The tiler's overlay
  is the same mechanism scaled up. It belongs in the **shell process**, with a small animation
  thread paced by `DwmFlush`, because thumbnail updates have no thread affinity (komorebi
  `ghost.rs`). That keeps the per-frame 0.2–1.2 ms off the UI thread, and nothing runs once the
  spring settles.
- **WGC proxies**, if effects are wanted, would reuse the engine's overlay pipeline
  (`crates/wallpaper/src/overlay/`: one WGC session per monitor with an exclusion list,
  D3D11↔D3D12 shared textures, DComp output, and kept warm after use). It costs a device
  (+16–51 MB) and capture GPU work, and it should only run during an animation.

## Recommendation

1. **Scroll, focus and Overview animate DWM thumbnails in one shell-owned overlay** per
   monitor (or across the virtual screen, as komorebi does). Freeze the content of windows that
   are about to resize by thumbnailing a `PrintWindow(PW_RENDERFULLCONTENT)` snapshot, as
   komorebi does. Spring physics drives `rcDestination` from a `DwmFlush`-paced thread. That
   is about 40 µs per window per frame, and the apps do nothing.
2. **Commit once at the end** with a single `DeferWindowPos` batch (move and resize; never
   NOREDRAW or NOCOPYBITS). Keep the proxy over the real window until the app has repainted,
   then fade it out. Use `SWP_ASYNCWINDOWPOS` only as a fallback for windows that don't answer
   within a timeout.
3. **While the overlay covers them, leave the real windows where they are.** We cannot cloak
   them cross-process. The alpha-0 layered "veil" works cross-process and keeps thumbnails
   live, but it rewrites a foreign window's extended style: keep it as an experimental option
   only.
4. **Park off-view columns outside the virtual-screen bounding box** at real coordinates.
   Chromium/Electron then throttle themselves; DWM stops processing GDI windows there; the
   taskbar, Alt-Tab and window state are unchanged; and flip-model windows stay live for the
   Overview. Add a crash-safe "parked windows" record, recovery after display and DPI changes,
   and follow-focus scrolling. Treat `SetCloak` as an opt-in experiment, behind a flag.
5. **Open checks** before building: elevated (UIPI) windows; display hot-plug with parked
   windows; real Chromium, UWP/CoreWindow and Office apps (only a GDI and a raw DXGI app were
   tested); DWM-side cost of 30 live thumbnails, measured from an elevated sampler on an idle
   machine; rounded corners and shadows for proxies.

## Notes on the runs

- Run 1 was launched through `runas … cmd /c …`, which opened a visible console window. Runs
  2–3 used a windows-subsystem exe with no console.
- About 60 s into run 1, a **"Windows Security" Windows Defender Firewall notification**
  (`PickerHost.exe FirewallNotificationDialogServer`, opened 18:54:22) became the foreground
  window. The spike makes no network calls, and its windows can't activate. Runs 2 and 3
  reported the foreground unchanged. The dialog was still open when this was written; I did not
  touch it.
- All test processes exited (verified with `tasklist`), and every window was destroyed with
  its process.

[swp]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos
[edwp]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enddeferwindowpos
[dwma]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
[dwmrt]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmregisterthumbnail

Other sources: Chromium `ui/aura/native_window_occlusion_tracker_win.cc` (main, fetched
2026-11-08); komorebi @72aa4c9 (`C:\mypaper-targets\tiling-research\src\komorebi`); GlazeWM
@5709ad0; Whim @1e86b57; mypaper `docs/performance.md`, `docs/glass.md`,
`crates/desktop/src/win_events.rs`, `crates/bars/src/preview.rs`, `crates/bars/src/taskbar.rs`,
`crates/wallpaper/src/overlay/{mod,capture}.rs`.
