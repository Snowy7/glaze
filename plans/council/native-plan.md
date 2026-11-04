# mypaper native: the final plan, v8

This plan merges the round-1 native plans (Grok, GPT Sol, Claude Opus) with the user's decisions and
six rounds of review. Final scores: Sol 9, Opus 9, Grok 9.

It replaces `plans/council/plan.md`, which kept WebView2. The user builds the whole plan in one go,
so the slices are build order, not releases.

## User decisions (fixed, not up for review)
- **Everything goes native.** Minimize CPU, RAM and GPU, at idle and in use.
- **A destructive rebuild.** No dual paths, no keeping the old shell usable, no WebView2 fallback.
  Delete as we go.
- **Anything may be rebuilt or ported:** the shell, the wallpaper engine, core, Studio and themes.
- **Several styles, and Liquid Glass is the default.** The others are One UI-like, solid, Fluent
  and mono.
- **Liquid Glass on every surface**: the top bar, dock, flyouts, notification center, menus and
  widgets. It is always **live** to what is behind it. The clear↔tinted amount and the blur are
  adjustable.
- **The top bar and dock float over the wallpaper**, as the widgets do.
- **Web and app wallpapers stay**, with live glass over them (D-W).
- **Standalone:** no third-party tools. Restyling the inside of Explorer and Settings is a later,
  separate project.
- **Keep:** the panic hotkey (which restores the stock desktop) and the undo journal for Windows
  settings.

## Rule 1: one hardware GPU device for the whole product, the engine's
On this machine, any process with a hardware D3D device pays about **235–246 MB private** for the
vendor driver stack (`docs/engine-notes.md:27-40`).
- The **shell** renders with **software Direct2D + DirectWrite** (a WIC-bitmap render target) into
  per-pixel-alpha layered windows (`UpdateLayeredWindowIndirect`).
  - Software D2D runs on WARP, so `d3d11`, `dxgi` and `d3d10warp` load. That is allowed. A hardware
    adapter is not.
  - Tested invariant: no vendor driver DLL (`nv*umx`, `nvgpucomp64`, `amdxc64`, `atidx*`) ever loads
    into the shell.
  - N0.1 records WARP's private MB and the full DLL list.
  - If WARP costs too much, the fallback is a CPU rasterizer (tiny-skia) plus DirectWrite's
    `IDWriteBitmapRenderTarget1` in grayscale mode, which writes premultiplied alpha.
    - RichEdit's GDI `TxDraw` writes no alpha, so under the fallback the text fields (launcher,
      notes, todo) get an **opaque field fill**, and their rect's alpha is set to 255 after `TxDraw`.
- The **engine** is the only GPU process. It draws the wallpaper **and all Liquid Glass**.
- `crates/desktop/src/menu_ui.rs:126` (`D2D1_RENDER_TARGET_TYPE_DEFAULT`) loads the driver into core
  on the first right-click. The menu moves to the shell, on a software target, in N1.

## Rule 2: if nothing changes, nothing is drawn
- A surface redraws only its changed region, and only when its display list changes.
- Animation timers run only while something moves.
- Glass redraws only when the pixels behind *that surface* change (see Overlays).

## Processes
| Process | Role | Always on |
|---|---|---|
| `mypaper-core` | Supervisor, tray icon, panic hotkey, styling journal, restoring the taskbar when the shell dies | yes |
| `mypaper-wallpaper` (the engine) | wgpu: wallpaper, glass plates, glass overlays | yes |
| `mypaper-shell` (new) | Top bar, dock, flyouts, notification center, launcher, all menus (the desktop menu moves here from core), widgets, tray host, window list | yes |
| JSON-RPC plugins | Third-party data providers | only while used |
| Studio | Settings app | only while open |

- **`mypaper-bars` and `mypaper-widgets` are deleted.** Their Rust services (app bars, tray host,
  WinEvent window list, media, audio, network, notifications, stats, icons, journal) become a library
  in the shell, with the duplicates between the two crates merged.
- **`kind: web` widgets, the HTML widget SDK and `packages/widget-sdk` are cut.** No third-party
  HTML widget exists, and native built-ins plus JSON-RPC plugins cover the need.
- **Crashes:** the shell keeps the workspace's `panic = "abort"` (`Cargo.toml:54`), so there are no
  per-widget error cards. If the shell dies, core replays the taskbar journal at once and restarts the
  shell (✅ back in ≤300 ms). Core never waits on the shell, so the panic hotkey works even while the
  shell hangs.

## Budgets (merge gates)
- **Measurement:** `tools/perf`, with a new `real-look` scenario. The report converts to percent of
  one core; `perf/budget.toml` stays in machine percent. Median of 3×60 s.
- **The idle fixture:** the user's 2 monitors at 1440p, **both visible**; the default theme (rain-city,
  ported to the engine); the default Liquid style on the bar, dock and 6 widgets; media paused; a
  minute clock; no flyout open. CPU is in percent of one core; RAM is private MB per process.
- **Today's baselines are not comparable.** The 255 MB "image" figure is core plus engine with one
  monitor covered (`docs/performance.md:7-9,34-35`). A second visible swapchain adds 27–66 MB
  (`:97-98`), and the first plate allocates a full-frame scratch of about 14 MB per screen
  (`glass_pass.rs:338-339,634-640`). So the engine's gate is set **from N0's measurement**, not guessed.

| | Today | Gate |
|---|---:|---:|
| Steady processes | ~14 (4 ours plus ~10 WebView2) | **3** |
| core | (part of 255) | ≤8 MB, ~0% |
| shell, idle fixture | ~415 MB | **≤40 MB, ≤0.1%** (bars over maximized apps ≤0.05%) |
| shell, a flyout or menu open | n/a | ≤+6 MB, freed on close; content on screen ≤30 ms; 0 redraws once settled |
| engine, idle fixture | n/a (see above) | **≤458.1 MiB private** (N0.3: 448.1 + 10 MiB); CPU per the rain-city row (0% over a still image) |
| plates on an animated wallpaper | n/a | ≤0.3 ms GPU per frame for all plates, measured with wgpu timestamp queries added in N2, only on frames drawn anyway |
| default rain-city wallpaper, 2 screens at 30 fps, keeping its 0.65 render scale | ~30% (web) | ≤4% CPU (the per-frame cost is ~0.57 ms, from aurora's 3.43% at 60 fps, `perf/results/2026-11-04-0502-final.md:38,69`); GPU ≤2 ms per frame per screen with wgpu timestamps, which covers `rain.frag`'s inclusive blur loop (18 samples including the centre) |
| glass overlay open | n/a | glass fades in within the 300 ms open animation; engine ≤3% CPU and ≤0.5 ms GPU per update; frames whose dirty rects miss the region are skipped; ≤ +30 MB while open, freed on close |
| dwm.exe vs the same wallpaper without the shell | n/a | ≤ +0.5% CPU, ≤ +1% GPU at idle |
| Whole product, idle fixture | ~670 MB | **≤ engine gate + 48 MB** (engine fixture gate: 458.1 MiB; combined allowance 506.1 MiB) |

## UI technology: software D2D + DirectWrite + layered windows (`crates/ui`)
Chosen over:
- **wgpu + Vello:** needs a device (+235 MB here), and has no IME or UI Automation.
- **Slint, iced, egui:** winit windows fight app bars, desktop-layer children and no-activate popups,
  and none uses DirectWrite.
- **WinUI 3:** a XAML stack and a device per process.
- **Sciter, Ultralight:** licences, and a second CSS dialect.
- **Blitz:** alpha quality and GPU-only.

`crates/ui` is hand-rolled from `menu_ui.rs` and `menu_model.rs`, with no line cap. It contains:
- **Surface:** a layered window (topmost popup, floating bar, or a layered desktop-layer child for
  widgets), updated over dirty rects. Bitmaps are cropped to each surface.
  - Hit-testing follows alpha, and alpha-0 pixels let clicks through. So the shell **fills every
    surface's shape at alpha ≥1/255** (invisible), even where the glass behind it is drawn by the
    engine. Only the space between surfaces is alpha 0, and that clicks through.
  - Every surface gets a click test in its slice's check.
- **Scene:** `view(&Model, &Tokens, now) -> DisplayList` (fills, strokes, text, icons, images, and hit
  items with rect, id, role and label). If the list is unchanged, nothing happens. Otherwise only the
  changed bounds are redrawn.
- **Text:** a cache of `IDWriteTextLayout`, with grayscale antialiasing on alpha.
  - **Text entry** uses **windowless RichEdit** (`ITextServices2::TxDrawD2D` on our target) for the
    launcher search, notes and todo. That provides IME, bidirectional text, the clipboard and undo.
  - Wi-Fi passwords use Windows' own prompt. Per-widget settings live in Studio.
- **Icons:** icon fonts through DirectWrite (Segoe Fluent Icons, Fluent UI System Icons TTF, Lucide
  TTF). App icons come from the existing cache.
- **Layout:** row, column and grid helpers using style metrics.
- **Input:** hit ids map to the existing host handlers (the bridge's methods, without the JSON).
  Keyboard focus generalizes `menu_model::step`.
- **Animation:** springs ported from `bars/shell/src/style/motion.ts`, on a high-resolution waitable
  timer armed only while something moves. Slides and fades change only the window's position and alpha.
- **Accessibility:** `accesskit_windows`, built lazily on `WM_GETOBJECT`.
- **Tests:** golden PNGs of every surface, rendered headless from the same software target, in
  `cargo test`.

## Liquid Glass everywhere, live, drawn by the engine
1. **Plates, for surfaces over the wallpaper** (the floating top bar and dock, and widgets).
   - The shell sends each surface's rects, corner radii and material to the engine.
   - The engine draws the glass into its own frame (`crates/wallpaper/src/glass_pass.rs`): bevel
     refraction concentrated at the edge, blur, tint and an angle-dependent specular, following
     `plans/council/liquid-glass-research.md`, which moves to `docs/research/liquid-glass.md` in N2.
     The shell draws its content on transparent pixels above.
   - The glass is live by construction. Over a still wallpaper it draws only when geometry or
     material changes, and settles within ≤1 s.
   - **Geometry transport:** the shell writes plate geometry into **shared memory** (a seqlock plus a
     wake event). The engine uses it **as is** each frame, with **no engine-side spring**: today's
     spring (`glass_pass.rs:10-12,22-25`) and the JSON-file transport (`crates/theme/src/glass.rs`)
     are deleted. The shell owns all motion.
   - **Moving surfaces** (dock magnify and slide, widget drags) change the plate shape at display
     rate, while the wallpaper may run at 30 fps.
     - The engine keeps the last wallpaper frame as a texture: rain-city's existing 0.65 offscreen
       target (`shader.rs:40`), or a per-screen texture for full-scale sources (N0.3 measures its MB).
     - On a geometry change it restores the swapchain image first (rain-city's 0.65 upscale blit,
       `shader.rs:44-48`, or a copy of the kept full-scale texture), then draws the glass, sampling
       the kept frame directly. That replaces the scratch copy, which **N2 deletes** along with the
       ~14.7 MB per-screen scratches (`glass_pass.rs:338-339,634-640`). The ≤1-frame check measures
       blit plus glass.

     ✅ (N2 and N4): plate glass is ≤1 frame behind its moving surface.
2. **Overlays, for surfaces over app windows** (flyouts, the notification center, the launcher, tray
   overflow, **all menus including the desktop menu**, and a bar while a window overlaps it).
   - The engine owns one overlay window per open surface. It is no-activate,
     `WS_EX_LAYERED | WS_EX_TRANSPARENT` (never hit), `WS_EX_NOREDIRECTIONBITMAP`, and z-ordered
     directly under the shell's popup. `WS_EX_TRANSPARENT` passes clicks through only on a layered
     window, so `WS_EX_LAYERED` stays; N0.2 confirms DComp content still shows with it.
   - The window draws through **DirectComposition**: `CreateSwapChainForComposition` with
     premultiplied alpha, under a DComp visual with a **rounded clip** matching the surface's radius.
     That gives the true glass shape, not a rectangle (an HWND swapchain can't be premultiplied).
   - The backdrop comes from **one Windows.Graphics.Capture session per monitor**, shared by every
     overlay on it, with a single frame-pool buffer.
     - It uses `DirtyRegionMode = ReportOnly` (build 26100+; the user is on 26300). Frames whose dirty
       rects miss every open surface's region (plus its blur margin) are dropped without processing,
       so a video elsewhere costs only WGC's frame delivery. N0 measures that cost.
     - The session exists only while at least one overlay is open on that monitor.
   - **Keeping our windows out of the backdrop:** first choice is a per-session window exclusion list
     (`SetWindowExclusionList`, confirmed by all three reviewers), so our windows still show in screenshots
     and screen shares. N0.2 verifies the API exists and works. Only if it doesn't, use
     `WDA_EXCLUDEFROMCAPTURE`, and the user decides between that screenshot loss and a "visible in
     screenshots" setting that turns menus into a shell-drawn tint.
   - **No popups overlap one another.** Submenus butt against their parent with a gap of 0 (today they
     overlap by 4 px, `menu_model.rs:322`). A rim that samples past the edge reaches only what is
     behind the parent, because popups are excluded from capture. So no glass ever needs one of our
     own surfaces behind it.
   - The overlay is created on open, its glass fades in during the open animation, and it is
     destroyed on close.
3. **Material controls**, per style and adjustable by the user:
   - `clarity` (clear↔tinted: tint opacity and luminance adaptation together);
   - `blur`, `refraction`, `highlight`;
   - the `tint` colour.

   - **Blurred styles** (oneui, fluent) use the same plate and overlay paths with refraction 0: one
     implementation, and the DWM host backdrop (`blur.rs`) goes away.
   - **Materials with no blur** (solid, mono's tint) need no backdrop: the shell draws the fill itself,
     with no plate, overlay or capture.
4. **Wallpapers:**
   - **N2 ports the web themes that are a shader over a photo** (rain-city, rain-city-amber,
     rain-city-midnight, snow-city, snow-forest) to the `shader` kind.
   - To support that, `shader` gains **image inputs**: a texture binding declared in `theme.json`
     (`shader.rs:85,135-142` today has only a uniform buffer).
   - **The `web` and `app` kinds are kept** (user decision D-W: fluids, vanta-waves, user web pages,
     Unity `-parentHWND` apps). They render themselves, so there's no engine frame for plates.
     - Surfaces over such a wallpaper use the **overlay path** with the same per-monitor WGC session
       and exclusion list. The difference is the overlay window's layer:
       - over the floating bars, it's topmost, directly under the bar;
       - under widgets, it's a child of the desktop host, z-ordered between the wallpaper window and
         the widget window.
     - Dirty regions keep it change-driven. Over an animated web or app wallpaper the glass updates
       at that wallpaper's own frame rate, only where glass is visible, and only while such a
       wallpaper is shown. Over a still one it costs nothing.
     - The engine keeps hosting web wallpapers in its own WebView2. That is the only WebView2 left in
       the product, and it runs only while a web wallpaper is chosen. App wallpapers stay as they
       are today.
     - N0.2 spikes the desktop-layer overlay under a widget over a web wallpaper.

## Styles
A style is Rust data: metrics, glyph font, panel layouts, motion, and default material. Themes override
tokens in layers: style ← `theme.json` `appearance` ← `bars.json` `look` ← per monitor. CSS is removed.

| Style | Character | Default material |
|---|---|---|
| **liquid** (default) | macOS Tahoe calm, SF-like metrics, Lucide icons | Liquid Glass, mostly clear |
| **oneui** | Large radii, bold type, roomy cards | Blur, tinted, no refraction |
| **solid** | Opaque surfaces, crisp borders | None |
| **fluent** | Windows 11 metrics, Fluent icons | Blur, tinted |
| **mono** | Today's mono | Tint |

Dropped fields (gradients, images, noise, glow, neon, bevel corners, CSS) log a warning once. One
switch also sets the DWM frame and accent, light/dark mode and `ColorPrevalence`.

## Widgets: all 17 native
1. **Edit mode,** reusing `crates/widgets/src/layout.rs`.
2. Clock and calendar (sharing the calendar flyout's grid), a single media view (now-playing and
   media-pill), quick-toggles and the launcher.
3. Weather, system-monitor (2 s ticks, only while visible), timer, countdown and photo.
4. Notes, todo, rss, crypto, heatmap and quote.

Fetchers use WinHTTP on a worker lane and sleep between fetches; the weather plugin's code is linked
in. There is no ambient motion, and the spectrum view is opt-in. Instance ids, settings and storage
are preserved.

## Slices (destructive, in dependency order)
- **N0 Spikes (measure before porting).**
  1. **Shell surface:** a software-D2D layered bar is ≤10 MB, 0% over 60 s, redraws a 3840×36 bar
     in ≤1 ms, and loads no driver DLLs. It also records the shell's **first-paint time** from
     process start.
  2. **Overlay:**
     - a DComp rounded overlay from the engine sits under a shell popup with correct z-order while
       both move;
     - the per-session exclusion list
       ([`IDisplayGraphicsCaptureSession::SetWindowExclusionList`](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.idisplaygraphicscapturesession.setwindowexclusionlist),
       SDK 10.0.26100.8249+) keeps both out of the backdrop, while a Snipping Tool screenshot still
       shows the popup. The installed SDK and `windows-0.62.2` lack it, so N0 adds updated bindings
       or a small ABI definition, and detects support with `QueryInterface`, not by build number.
       If that fails, WDA plus the user's decision;
     - exclusion is async: frames with an older `ConfigurationIteration` are dropped;
     - **the capture device:** WGC's frame pool needs a D3D11 device, and the engine is DX12-only
       (`crates/wallpaper/src/gpu.rs:300`). Measure D3D11On12 against a separate D3D11 device, and
       keep the cheaper one within the overlay budget;
     - **border and cursor:** `IsBorderRequired = false` and `IsCursorCaptureEnabled = false`.
       Otherwise every open menu draws a yellow border around the monitor and blurs the cursor into
       the glass.
       - N0.2 first tries the **unpackaged** flow OBS uses: `RequestAccessAsync(Borderless)`, then
         `IsBorderRequired = false`. That flow is governed by Settings › Privacy & security ›
         Screenshot borders.
       - Only if access is not `Allowed` (for example `NotDeclaredByApp`) does it add package identity
         through a **sparse package**
         (MSIX with an external location, signed with a dev certificate) that declares
         `graphicsCaptureWithoutBorder`.
       - ✅ `RequestAccessAsync(Borderless)` returns `Allowed` (it may show a one-time consent
         prompt; a denial leaves the border even though `IsBorderRequired = false` succeeds), and no
         border appears when a menu opens.
     - one session serves two overlays on a monitor;
     - clicks on the popup's glass body reach the popup;
     - glass lags ≤1 frame;
     - with a video playing elsewhere on the monitor and nothing changing behind the popup, the
       engine is ≤1% CPU and does 0 shader passes;
     - **no feedback loop:** 0 shader passes while only our own windows change (the overlay's
       presents, and the popup's hover or clock repaints);
     - open-to-glass time and memory are recorded;
     - **the desktop-layer variant (D-W):** an engine overlay that's a child of the desktop host sits
       between a web wallpaper window and a widget window, and its glass stays live.
  3. **Engine with plates:** private MB with 2 visible screens and the bar, dock and 6 widget plates,
     measured on the rebuilt engine, default integrated GPU: full-scale aurora **420.21 MiB**,
     ported rain-city **448.07 MiB** (median of three 60-second runs each; 16 plates across both
     screens). Rain retains two 1664 x 936 frames and aurora two 2560 x 1440 frames; no plate
     scratch remains. The larger measured median + 10 MiB sets **458.1 MiB** as the engine gate.
     Reproduction and remaining GPU/lag gaps: `docs/performance.md`, Native measurements.

  If spike 2 fails, redesign the overlay inside the engine before N3. There is no shell device.
- **N1 `crates/ui` and `mypaper-shell`, with the desktop menu** (tint, no glass yet). ✅ A golden PNG
  of the content; opens in ≤25 ms; core loads no D2D. Delete: menu drawing and wiring in core.
- **N2 Top bar and dock, with plates, plus the rain-city/snow shader port** (image inputs), so the
  default theme gets glass from day one.
  - **Floating bars keep their app-bar reservation** (`docs/bars.md:19-20`), so maximized windows
    stop at the bar. A bar needs an overlay and capture only while someone drags a window under it.
  - **Startup order:** core keeps `wait_for_shell` and `stop_leftover` (`crates/core/src/main.rs:102-107`)
    and spawns the shell right after them. The early taskbar hide and the journal replay at start
    (`main.rs:111`) move into the shell, which owns the taskbar journal. Core replays the journal only
    after the shell has exited (crash or panic hotkey), so the two never write it at once.
  - ✅
    - the shell and engine rows on the idle fixture;
    - 0 WebView2 for the bars;
    - bars on screen ≤ the moment core spawns the shell + N0.1's first paint;
    - golden PNGs and click tests;
    - rain-city on the engine at ≤4% of a core on both screens;
    - the plates' GPU time measured;
    - plate glass ≤1 frame behind a moving dock.
  - Delete: `bars/shell/`, `crates/bars/src/web.rs`, `first_paint.rs`, `capture.rs`, `blur.rs`, the JSON
    bridge, the `mypaper-bars` binary, the plate scratch, the JSON plate transport and the engine
    spring, and the five ported web theme folders' JS.
- **N3 Overlays, plus flyouts, the notification center, launcher, tray overflow and all menus.**
  Calendar, mixer, quick settings, Wi-Fi, language, power and notifications; Win+N and Win+A open
  ours; a bar switches to an overlay while a window overlaps it. ✅ The overlay row, content ≤30 ms,
  0 redraws once settled, and Arabic and CJK input in the launcher.
- **N4 Widgets** (all 17, edit mode, plates). ✅ The whole-product gate, the shell ≤40 MB, no WebView2
  anywhere in the default setup. Delete: `widgets/shell/`, `widgets/*`, `packages/widget-sdk`,
  `crates/widgets` (except its library services), `webview_env.rs`'s shell environment,
  `plates.rs`, `backdrop.rs`, CapturePreview and the request files.
- **N5 Engine:**
  - **Web and app wallpapers get live glass through overlays** (D-W): bars use topmost overlays, and
    widgets use desktop-layer overlays. ✅ Glass is live over fluids and a Unity app wallpaper, costs
    0 over a still web page, and the web wallpaper's own cost is unchanged.
  - Delete the 1/8 BMP backdrop pipeline.
  - Probe only the selected GPU (the `wgpu-hal` patch, `docs/performance.md` Limitations).
    **Default remains integrated/low-power on desktops and laptops.** Honour explicit
    `engine.gpu` / per-wallpaper `integrated`, `high-performance`, or `auto` settings; never
    promote a desktop to discrete graphics because a budget is missed. GPU and wallpaper FPS
    preferences already have a live config/theme schema; Studio exposes them in N7.

  ✅ The engine gate improves or holds; the only WebView2 left is the web wallpaper source (used only
  while a web wallpaper is chosen) and Studio's, until N7.
- **N6 Styles and materials:** five styles, material controls, icon fonts, and Windows token sync.
  ✅ One switch restyles everything; journal round-trip and undo; golden PNGs per style; budgets
  unchanged.
- **N7 Studio native, last and deferrable,** on `crates/ui`, so previews use the real renderer.
  Delete Tauri, `apps/studio`'s web code and pnpm. ✅ ≤60 MB while open, 0 when closed.

## Losses taken on purpose
- **Only if N0.2's per-session exclusion fails:** open flyouts and menus vanish from screenshots
  (`WDA_EXCLUDEFROMCAPTURE`). The user then chooses between that and a "visible in screenshots"
  setting.
- Theme CSS, gradients, image, noise and glow fills, neon presets, ambient widget motion, and two of
  the four launcher layouts.
- Settings edited on the widget itself, HTML widgets, the DWM host backdrop, and today's stand-ins.

## Risks
- **Text quality:** grayscale DirectWrite on layered windows. The user compares N0 side by side.
- **The overlay (N0.2)** is the biggest unknown, so it is measured before any port.
- **IME:** windowless RichEdit with Arabic and CJK input, tested in N3.
- **Effort:** roughly 15k new Rust lines replace about 21k lines of TS, CSS and glue, plus Studio.
