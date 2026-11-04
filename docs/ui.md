# Native UI: `crates/ui` and `mypaper-shell`

The native rebuild (`plans/council/native-plan.md`) draws every shell surface (menus now; the top
bar, dock and flyouts next; widgets in N4) with one small UI layer, `crates/ui` (`mypaper-ui`), hosted by
one process, `mypaper-shell`. This page is how both work, and what they measured.

## Rules it follows

- **No hardware GPU device in the shell** (plan Rule 1). A process with one pays about 240 MB
  private for the vendor driver stack here. The UI draws with **software Direct2D** into a **WIC
  bitmap** (`D2D1_RENDER_TARGET_TYPE_SOFTWARE`), and text with **DirectWrite**. WARP's DLLs
  (`d3d11`, `dxgi`, `d3d10warp`) are allowed; vendor drivers (`nv*umx`, `nvgpucomp64`, `amdxc64`,
  `atidx*`) never load. A test checks it (`crates/ui/tests/no_gpu_driver.rs`), and the shell logs an
  error if one ever does.
- **If nothing changes, nothing is drawn** (plan Rule 2). A surface redraws only the bounds of
  the paint operations that changed, and an identical frame costs a list comparison. The frame
  ticker runs only while something moves. The shell's loop sleeps in
  `MsgWaitForMultipleObjectsEx` until a message, a scheduled widget tick, or its supervisor's exit wakes it.

## A frame

```text
view(&model, &tokens) ──► DisplayList { ops, hits, focus }
                                   │
Surface::present(ui, origin, size, list)
  1. damage = bounds of the ops that differ from the list on screen   (none: stop)
  2. software Direct2D redraws that rectangle only (clip, clear, ops that touch it)
  3. the rectangle is copied from the WIC bitmap into the window's DIB section
  4. UpdateLayeredWindowIndirect with prcDirty = that rectangle
  5. the accessibility tree follows the hits (only once a client has asked)
```

- **Ops** (`Op`): `Fill`, `Stroke` (rounded or not), `Line`, `Text` (a string or an icon glyph,
  placed by its measured size), `Shadow` (stepped rings), `Arc`, `Image` and `TextField`.
- **Hits** (`Hit`): rectangle, id, AccessKit role, label, enabled, checked. Input hit-tests them,
  keyboard focus steps over them (`focus::step`), and assistive technology sees them.
- **Damage** is computed op by op in list order: a changed clock redraws the clock; a moved fill
  redraws where it was and where it is.
- **Text** goes through a cache of `IDWriteTextFormat` and `IDWriteTextLayout`: a label is shaped
  once and redrawn from its layout. Text is grayscale-antialiased with fixed rendering parameters
  (layered windows have alpha, so no ClearType; and golden images must not depend on the machine's
  ClearType tuning).
- **Icons** are glyphs of an icon font drawn as text: Segoe Fluent Icons (Segoe MDL2 Assets on
  Windows 10). The styles (N6) add Lucide and Fluent UI System Icons as bundled TTFs, registered in
  a private font collection; views keep naming a family.
- **Tokens** (`Tokens`): type, colours, material and metrics in design pixels; a view multiplies
  by its scale (DPI times the theme's `scale`). Today they cover menus; N6 adds the styles.

## Surfaces

A `Surface` is one per-pixel-alpha layered window (`WS_EX_LAYERED`, updated only through
`UpdateLayeredWindowIndirect`). Kinds:

| `Kind` | Window | For |
|---|---|---|
| `Popup { activate }` | topmost popup; `activate: false` adds `WS_EX_NOACTIVATE` and answers `MA_NOACTIVATE` | menus, flyouts |
| `Bar` | topmost, never activated (the app-bar reservation is the bar's, N2) | top bar, dock |
| `DesktopChild { parent }` | **top-level** layered tool window in the bottom band immediately above `parent` (Progman); the historic name describes its layer, not its Win32 parentage | widgets and edit chrome |

- **Hit-testing follows alpha.** A layered window lets clicks through where alpha is 0 and keeps
  them everywhere else. So views fill each surface's whole shape at least with `Rgba::HIT`
  (alpha 1/255, invisible), even where the engine draws glass behind it; only the space between
  surfaces is 0. Golden tests check every body pixel of the menu is hit-testable.
- **Slides and fades** change only position and alpha (`move_to`, `set_alpha`): no redraw.
- **Memory follows visibility.** `release` drops a hidden surface's bitmaps; the next `present`
  draws everything again. Bitmaps are cropped to each surface.


### Desktop windows (N4)

`DesktopChild` uses `WS_POPUP | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`.
It has no Win32 parent or owner. `parent` is the Explorer **z-order anchor**. Screen coordinates
and per-monitor physical pixels apply. The shell manifest declares Windows 8/8.1/10 compatibility
and PerMonitorV2. The manifest does not make ULW children visible: N0.2 measured that raised
Progman hides GDI/ULW children, and WGC exclusion lists ignore children. A top-level surface can
be excluded from the desktop glass overlay capture; the engine's order is widget > glass > Progman.
See [engine-notes.md](engine-notes.md#glass-overlays).

`Surface::keep_above_desktop() -> Result<bool>` repairs the bottom band when necessary and
preserves directly underlying `mypaper.wallpaper.glass` overlays. `desktop_order_valid() -> bool`
is a read-only harness probe. `raise_in_desktop_band() -> Result<()>` lifts the edit palette above
other widgets while keeping ordinary apps above it. `WM_WINDOWPOSCHANGING` suppresses automatic
activation raises; explicit repairs use a guarded move. No periodic z-order timer runs. The host
reacts to window events and restores surfaces hidden by Show Desktop; Explorer destruction drops
surfaces and `DesktopEvents::Ready` recreates them against the new anchor. `DpiChanged(u32)` tells
the host to rebuild its monitor geometry and text/image scale. Mixed-DPI hardware must still be
validated when available; physical-pixel fixtures alone cannot establish that behavior.

### Images, rings and reusable calendar (N4)

`Ui::decode_image(bytes: &[u8], max_size: (u32, u32)) -> Result<Image>` uses WIC, scales before
converting to premultiplied BGRA, and bounds decoded dimensions. `Op::Image { rect, image:
Arc<Image>, opacity, source: Option<Rect> }` shares pixel storage between frames. `source` crops
in source pixels; `None` uses the full image. `Op::Arc { center, radius, sweep, width, color }`
draws a clockwise ring segment starting at twelve o'clock, with `sweep` in radians. These ops
participate in normal damage detection. Crossfades arm the ticker only for their finite duration.

`calendar::Date { year: i32, month: u32, day: u32 }` supplies checked `new`, `parse`, `key`,
`ordinal`, `from_ordinal`, `weekday` (Sunday = 0), `iso_week`, `add_days`, and `shift_month`.
`MonthGrid::new(month: Date, week_start: u32)` builds the reusable calendar grid. Its `view(&self,
list: &mut DisplayList, rect: Rect, today: Date, text: Rgba, accent: Rgba, base: u32,
font: &TextStyle)` appends cells and hits `base + day`. Consumers own selection and event storage;
the calendar flyout can reuse the same component without depending on widget state.

### Windowless text fields (N4)

`TextField::new(window: HWND, rect: Rect, multiline: bool, size: f32, color: Rgba) -> Result<Self>`
creates windowless RichEdit text services from `Msftedit.dll`. It is UI-thread-local and cloneable.
Call `set_text(&str)`, `text() -> Result<String>`, `set_rect(Rect)`, `focus()`, `blur()`, and
`revision() -> u64`. Attach it with `Surface::set_text_field(Option<TextField>)`, and append
`Op::TextField { rect, field: TextField, revision: u64 }` to the display list. Detach and blur on
close. Text and its undo history stay in the component while it is attached.

`ITextServices2::TxDrawD2D` paints into the same software Direct2D target. The host supplies
selection/capture, timers, caret invalidation, scrolling and the surface's IMM context. Raw Win32
keyboard, pointer and IME messages enter RichEdit synchronously; app callbacks remain queued.
`Event::TextChanged` covers text, selection and caret changes; compare/read text before saving.
`Event::TextSubmit` is Enter in a single-line field. Multiline Enter stays with RichEdit. Native
selection, bidirectional shaping and undo are covered by the component test. The live fixture
also enters and persists Arabic/CJK through Unicode keyboard events; that check does not exercise
a language-specific IME candidate/composition UI.

## Input

Window procedures never run app callbacks. They turn messages into `Event`s (`Move`, `Press`,
`Release`, `Key`, `Deactivated`, `Invoke`, `Focus`, `DpiChanged`, `VisibilityChanged`,
`Cancelled`, `TextChanged`, `TextSubmit`) on the thread's queue, and the app's loop takes
them with `next_event()` after dispatching. So app state is never borrowed twice, even when Windows
sends a message from inside our own call (showing, focusing, a COM call to Explorer whose wait loop
dispatches). `Surface::discard_events` drops what a hidden or reused surface said.

## Motion

`Spring` is motion.ts's spring (`spring`, `spring-soft`, `spring-bouncy`; the closed-form solution
of m·x'' + c·x' + k·(x − 1) = 0, with the same settle rule). `Tween` moves a value along one.
`Ticker` is a high-resolution waitable timer (`CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`) armed one
display frame ahead (DWM's refresh rate) only while something moves: wait on `handle()` with the
messages, and call `schedule(moving)` after each pass.

## Accessibility

Each surface has an `accesskit_windows` adapter from the start, holding no tree. The tree (a root
with the surface's role and name, and one node per hit with its role, label, bounds, disabled and
toggled state) is built from the hits on the first `WM_GETOBJECT`. Until a client asks, updates do
nothing. Click and focus requests come back as `Event::Invoke` and `Event::Focus`.

## The menu component

`Menu<C>` is a popup menu with cascading submenus, generic over the command type `C`:

- `Entry<C>`: `Separator`, `Action { label, command, checked: Option<bool>, enabled }`,
  `Submenu { label, items }`;
- `menu::layout` measures and stacks rows; `menu::view` is the panel's display list (shadow,
  tint, border, separators, highlight, check marks, labels, chevrons, one hit per row);
- `menu::place` opens at the cursor and flips away from the work area's edges;
  `menu::place_submenu` puts a submenu **against** its parent's edge, with no gap and no overlap;
- `Menu::open`, `close`, `contains`, `owns`, and `handle(ui, surface, event) -> Option<Outcome<C>>`
  (`Chosen(command)` or `Closed`). Keys drive the innermost panel: arrows, Enter, Escape, Left and
  Right. The pointer drives whichever panel's body is under it, so over a submenu's shadow it
  still reaches the parent underneath.
- The root window and one submenu window are created hidden in `Menu::new` and reused: creating a
  layered window costs about 20 ms, more than the rest of opening a menu. A closed menu's windows
  keep no bitmaps.

## `mypaper-shell`

One UI thread, plus the desktop menu's mouse-hook thread (a low-level hook must answer in
microseconds, whatever the UI is doing). Core starts it as a supervised child (restart policy as
for the other hosts).

- **The loop** waits on its parent process and the message queue; it dispatches, then drains the
  inbox and the UI events in order.
- **The inbox** is a message-only window (class `mypaper.shell.inbox`) that only queues: the hook,
  the `config.json` and `palette.json` watchers post to it from their threads. `WM_CLOSE` to it
  quits the shell.
- **Lifetime:** one shell at a time (a named mutex; a second one exits at once); it exits when the
  process that started it does, so a crashed core never leaves a shell (and its hook) behind.
- **It hosts** the native desktop widgets and the desktop right-click menu ([desktop-extras.md](desktop-extras.md)): it
  follows `desktop_menu` in `config.json` and the theme's look live; with the menu off, nothing is
  installed. Core loads no Direct2D and installs no hook.

## Tests

- `cargo test -p mypaper-ui`: damage, clipping and premultiplied output on the software target,
  springs and the ticker, focus stepping, menu layout and placement (0 gap), the accessibility
  tree, and the GPU-driver check (an integration test: a process of its own).
- `cargo test -p mypaper-shell`: the desktop menu's items, the theme look, and **golden PNGs** of
  the default menu and its View submenu (`crates/shell/tests/golden/`), rendered headless through
  the same software target the screen gets. `MYPAPER_BLESS=1` rewrites them; a mismatch saves the
  actual image to `%TEMP%\mypaper-golden`.

## Measured

Release builds, 2026-11-04, Ryzen laptop (16 logical cores), two 1440p monitors at 96 DPI, theme
rain-city. CPU is a percentage of one core; RAM is private MB.

### N0.1: a 3840×36 bar surface

`cargo run --release -p mypaper-ui --example mypaper-bar-spike -- --x -2560 --y 9` (the bar spans
both monitors), self-measured, with `measure.ps1` alongside for 60 s.

| | Measured | Gate |
|---|---:|---:|
| Private memory, idle after 400 redraws | **8.5 MB** (`measure.ps1`: 9 MB) | ≤10 MB |
| CPU over 95 s idle | **0 ns, 1 wakeup** (`measure.ps1`: 0%) | 0% |
| Redraw, whole bar (draw + copy + `UpdateLayeredWindowIndirect`) | median **0.33 ms**, p95 0.52, max 0.88 | ≤1 ms |
| Redraw, the clock only | median **0.21 ms**, p95 0.42, max 1.23 | ≤1 ms |
| An identical frame | not drawn, 0.001 ms | |
| First paint after process creation | **73 ms** (23 ms of it drawing and showing) | recorded |
| Slide-in, position and alpha only | 118 frames in 537 ms, then the ticker disarmed | |
| GPU vendor drivers loaded | **none** | none |

Private memory step by step (live): 1.4 MB at start, 1.6 after the Direct2D, DirectWrite and WIC
factories, 2.1 after the window, 6.8 after the first paint, 8.7 after 400 redraws, 8.5 idle.

**WARP's cost.** Measured headless (no window): creating the first software target and drawing a
fill (which brings WARP up) adds **1.5 MB** private; the first text adds 0.8 MB (font loading).
Each cached `IDWriteTextLayout` costs about **19 KB**, which is why the layout cache holds 64 and
drops the least recently used half when full (an unbounded first version reached 11.2 MB in this
spike). Making and dropping targets plateaus (no leak). A software target keeps its pixels about
three times (the WIC bitmap, WARP's surface, the window's DIB). WARP is well within the budget, so
the plan's fallback (tiny-skia + `IDWriteBitmapRenderTarget1`) is not needed.

**DLLs** loaded into the spike (42): advapi32 bcryptprimitives cfgmgr32 clbcatq combase **d2d1
d3d10warp d3d11** dcomp directxdatabasehelper dwmapi **dwrite** dxcore **dxgi** gdi32 gdi32full
imm32 kernel.appcore kernel32 kernelbase microsoft.internal.warppal msctf msvcp_win msvcrt ntdll
ole32 oleaut32 powrprof psapi rpcrt4 sechost sxs textshaping ucrtbase uiautomationcore umpdc user32
uxtheme vcruntime140 win32u windowscodecs. No `nv*umx`, `nvgpucomp64`, `amdxc64` or `atidx*`; the
shell's own list (with the menu open) has none either.

### N1: `mypaper-shell` with the desktop menu

This branch's core, wallpaper and shell, in place of the user's mypaper (no bars or widgets);
`crates/shell/examples/menu_live.rs` injecting right-clicks at the left monitor's centre.

| | Measured | Gate |
|---|---:|---:|
| Menu on screen, quick click (press and release together) | median **13.1 ms**, best 11.3, worst 14.5 | ≤25 ms |
| Menu on screen, a person's click (70 ms hold), after the release | median **1.8 ms**, worst 21.4 | |
| Escape closes it (it took the focus) | 30 of 30 | |
| Core loads Direct2D, DirectWrite, D3D or DXGI | **no** | no |
| Shell idle, before any menu | **3.0 MB**, 0% CPU | |
| Shell idle after 30 menus (`measure.ps1`, 60 s) | **11 MB**, 0% CPU; core 2 MB, 0.03% | |
| A menu open, over the warm closed state | **+0.9 MB**; with the View submenu +1.35 MB | ≤+6 MB, freed on close |
| Shell start to its loop running | 83 ms (`READY ready_ms`) | |

- The **first** menu costs the shell about 7 MB once (WARP, DirectWrite's fonts, UI Automation and
  the COM link to Explorer); in N2 the bars pay that at start, so a menu adds only its bitmaps.
- **Freed on close:** a closed menu's bitmaps and render targets are released, but the shell's
  private bytes stay at the heap's high-water (about 1 MB, reused by the next menu: no growth over
  30 menus). `HeapSetInformation(HeapOptimizeResources)` gave back only 0.08 MB, so it is not used.
- One quick click of 15 in the second run did not open our menu (it may have opened Windows' own);
  60 clicks over two runs had no other miss. The hook's decision logic is unchanged from core's.
- Golden PNGs of the default menu and its View submenu match the live menu's content; the only
  visible change is the submenu, which now touches the root instead of overlapping it by 4 px.

## N2 additions: bar icons and pointer events

- `Op::Image { rect, image: Arc<Image> }` draws premultiplied BGRA on the same software target.
  `Image::load(&Ui, &Path)` uses WIC to decode native PNG/ICO/JPEG/etc. A target caches up to 128
  shared bitmaps; releasing the surface releases them. The shell handles SVG icon-pack rasterization
  once with CPU-only resvg and shares decoded images across monitors.
- `Event::Leave` uses `TrackMouseEvent`; `Event::Wheel(Pointer, i16)` converts Windows' screen
  coordinates to the surface's local coordinates and retains the screen anchor. Positive delta is
  upward. `Event::DoubleClick` preserves native tray double-click callbacks. `Event::CaptureLost` lets dock drags cancel cleanly, and `Event::DpiChanged` asks the owner
  to recalculate monitor geometry. Existing menu/event consumers continue to work.
- N2 uses `Kind::Bar` for top bars, cropped docks and one-pixel auto-hide edge surfaces. Invisible
  glass bodies are explicitly filled at `Rgba::HIT`; only the gaps are alpha zero. Reservations live
  in the bars service library, separate from visible window geometry. Slides use `move_to` only.
- The shell loop also waits on the bars' disarmed-at-rest `Ticker` and virtual-desktop event. Its
  timeout is the earliest active provider/config/auto-hide deadline; no periodic message pump was
  introduced. Bar data/actions are Rust calls into `mypaper-bars` rather than a web bridge.

See [bars.md](bars.md) for configuration, recovery ownership and the N3 flyout/plate seams.

N2 bar measurements (2026-11-05, two visible 1440p screens, still photo, paused media): clean idle
median 14 MB and 0.05% of one core; observed peak 21 MB. Three maximized samples have a 0.08% median,
above the 0.05% gate. The final bar paint was at process age 164.4 ms (143.9 ms after core's post-spawn
marker), above the 73 ms reference. Shell kill/repaint took 370-572 ms, above 300 ms. Alpha hit tests,
the native clock click, panic restoration and six style PNGs pass. See [bars.md](bars.md#live-n2-measurements-2026-11-05)
for raw samples, the interrupted Explorer sample and the missing engine/glass/N4 fixture checks.


### N4: top-level desktop widget fixture (2026-11-05)

Six production widgets and a warmed closed menu, both 1440p monitors visible at 96 DPI. Three
60-second official samples: private memory 21/22/22 MB (median 22 MB), CPU 0.18/0.21/0.18% of one core
(median 0.18%, **above the 0.1% gate**). Actual-wall-time harness CPU median was 0.15623%. No vendor
GPU drivers or widget WebViews loaded. Consecutive minute samples added exactly 30 visible stats
presents and one clock present. The locked interaction fixture passed 12 app activations, a real
widget click, Show Desktop/restore, six new HWNDs after Explorer restart, grid drag/resize, and saved
Arabic/CJK Unicode keyboard input. The glass publisher is a stub and these are not merged-shell or
whole-product numbers. Mixed DPI and actual language-IME composition remain unmeasured. Full evidence
and remaining gates: [N4 report](../artifacts/n4/report.md).
