# mypaper: a total retheme at near-zero cost. Merged council plan, v6

This plan merges three round-1 plans (Grok, GPT Sol, Claude Opus) with the fixes from rounds 2 to 6.
Its scope is visuals only: the shell, widgets, wallpaper and native app styling. Filesystem and OS
tuning are out of scope. We adopt proven techniques and measure narrowly; no research platform.

## The rule
**If nothing on screen changes, our WebViews present nothing and the engine draws nothing new.**
A scheduled data update (a minute clock, media progress, a stats tick) may present once per tick
and must not start an animation. This is checked by counting frames, not inferred.

**The idle fixture:** a static image wallpaper, media paused, a minute clock, maximized apps or a
visible desktop, no flyout open.

**The check:** a 10 s trace that shows 0 presents per bars/widgets WebView, apart from the scheduled
ticks it lists (CDP `Tracing` frame events per page, plus an ETW DxgKrnl present count per process).
`document.getAnimations()` is a diagnostic only.

## Budgets
Budgets are in percent of one core, across all mypaper processes (the WebView2 browser, GPU and
renderer processes included). Measure them with `measure.ps1`, which S0 checks into `tools/` from
`%TEMP%`. Take the median of 3 runs of 60 s after settling, on the user's real 2-monitor setup.
`perf/budget.toml` stays in whole-machine percent (`:5`, 16 logical CPUs); only the report
converts to one core.

| Situation | Today | Budget (merge gate) |
|---|---:|---:|
| Bars idle over maximized apps, idle fixture | ~38% | ≤1% |
| Bars + widgets on a visible desktop, idle fixture | ~87% | ≤2% (a seconds clock adds ≤1%) |
| A flyout after opening | n/a | 0 presents within 1 s |
| Visible web wallpaper, per screen | ~30% for rain-city on 2 screens (from Opus's `%TEMP%` samples; S0 confirms) | ≤8.6% (the measured webdemo floor at 30 fps, `perf/results/2026-11-04-0502-final.md:44`); 0 when covered |
| dwm.exe, vs the **same wallpaper with no bars or widgets** | n/a | ≤ +1% CPU, ≤ +2% GPU 3D |

Fullscreen (1.2% today) is a diagnostic, not a gate. S0 names its cause, and it is fixed only if
the fix is trivial.

## Architecture decisions
1. **Keep WebView2** for the bars, flyouts and widgets. A WebView that presents nothing costs about
   0 CPU; its cost is memory, which is not this goal. There is **one** fallback threshold: if the bars
   are still above **3%** after S1, prototype a native D2D/DComp top bar reusing
   `crates/desktop/src/menu_ui*`. Between 1% and 3%, find the cause in the WebView instead.
2. **Pages draw no filtered glass.** No SVG filters, no `backdrop-filter`, and no `filter:` on large
   layers. Pages draw a flat tint and a static hairline. That includes the bars' "wallpaper" and
   "image" backgrounds: `look.ts:218,224` set `blur()/brightness/saturate` on the full-size `.bg`
   layer (`style.css:179`).
   - **Wallpaper backgrounds** show the engine's already-blurred 1/8 BMP as-is, published once per
     theme load, and **only over a still image wallpaper**. Over any animated source (slideshow,
     video, shader, scene), web or app wallpaper, they fall back to the flat tint, because without
     a refresh the snapshot would freeze. `"wallpaper"` stops being a lens fill (`look.ts:128`), so no
     DWM host-backdrop sprite sits under the BMP (`style.css:216-222`). Without `liquid`,
     `glassStyleOf` falls back to `"frosted"` (`look.ts:124`). Since `lensFill` then rejects both
     `"none"` and `"wallpaper"` (`look.ts:130-137`), neither fill turns on `data-native-glass`
     (`look.ts:332`).
   - **Request files deleted.** The bars stop writing `backdrop/request.json`
     (`crates/bars/src/app/looks.rs:228-240`). The widgets stop writing `widgets-request.json`
     (`crates/widgets/src/app.rs:1449-1452`, `crates/widgets/src/backdrop.rs:10-24`). With both gone,
     there is no 10 s refresh anywhere.
   - **Web capture deleted.** The web CapturePreview path (`sources/web.rs:312-327,408`) is removed,
     along with the request handling in `crates/wallpaper/src/backdrop.rs:579`. Web `redraw()`
     becomes `OnChange` instead of `Every(10 s)` (`web.rs:350-352`), so the engine stops waking.
   - **Image backgrounds** are shown unfiltered, and their blur option is removed.
3. **Glass pixels come only from layers that are already composited:**
   - **Over windows**, DWM host-backdrop sprites (`crates/bars/src/blur.rs`), no refraction. These
     already exist for flyouts, the auto-hide dock and `background.type: blur` (`docs/bars.md:416`).
     A clear bar over a window stays clear, as it is today; no switch is built.
   - **Under widgets, over engine-drawn wallpapers** (shader, image, video, scene), opt-in Liquid: the
     existing plate pass (`crates/wallpaper/src/glass_pass.rs`, wired at `engine.rs:22,77,115`).
     It adds a small measured cost (0.11 ms GPU plus 0.065 ms CPU encoding for 4 plates,
     `engine-notes.md:244`) on frames an animated wallpaper draws anyway, but no extra presents.
     Over a still wallpaper it may draw a **bounded** settle after a plate's geometry changes (the
     grow/fade spring, which settles within 1 s per `glass_pass.rs:24-27,716`), and then 0.
   - **Over web and app wallpapers:** a flat tint, Liquid or not. A live web screen is native
     (`sources/web.rs:360`), so plates cannot draw there. S1 deletes the CapturePreview, and app
     wallpapers publish nothing (`backdrop.rs:17-20`). After S1, "a snapshot exists" is true exactly
     where plates can draw: both depend on `copy_src` (`backdrop.rs:18-19`, `engine.rs:460`,
     `glass_pass.rs`). S5 uses that for the widget's acknowledgement.
4. **Delete:**
   - the BitBlt capture (`crates/bars/src/capture.rs`);
   - the in-page lens (`bars/shell/src/liquid.ts`, `packages/widget-sdk/src/lens.ts`);
   - the filters in `widgets/shell/src/glass.ts`;
   - **every** `backdrop-filter` in the shells and the kit:
     - `packages/widget-sdk/widget-kit/base.css:116-117,530-540,770` and their 17 copies in
       `widgets/*/base.css`;
     - `widgets/shell/src/style.css:243,366`;
     - the bars' tint fill at `bars/shell/src/style.css:337-339`.

     S1 checks with a grep that none remain.

   Do **not** merge the unfinished `crates/glass` (worktree t3-f25cc0ca). Move its research doc to
   `docs/research/liquid-glass.md`.
5. **Default look = the user's Mac:** a transparent menu bar (Tahoe's has no glass), a low-opacity
   tinted dock and widgets, one accent, and no gloss, gradients or rims. Liquid is opt-in.
6. **Shell surfaces: replace, don't restyle.** Our bars replace the taskbar (`taskbar.mode: hide`,
   the app-bar reservation, the journal). Our flyouts and notification center replace the native
   ones. Keep:
   - the per-monitor hosts, the tray host, the flyout lanes and the WinEvent window list;
   - `look.family`, the presets and the D2D desktop menu;
   - the existing low-level Win-key hook (`crates/bars/src/flyouts/winkey.rs`);
   - `bars/shell/src/menu.ts` (it already has keyboard navigation);
   - `bars/shell/src/style/motion.ts` (it already has `linear()` springs).

## Native Windows apps
**Ship (public APIs, journaled and undoable, ~0 cost):**
- **Already exists:**
  - DWM border, caption, caption text, corners and backdrop for allowlisted windows
    (`crates/tweaks/src/dwm.rs`, through the unchanged `crates/inject` allowlist);
  - apps and system light/dark (`crates/tweaks/src/plan.rs`), and the DWM accent (`plan.rs:57-80`);
  - the palette and cursors.
- **New:** `ColorPrevalence` (HKCU DWM), one more journaled value next to `AccentColor`. Windows then
  tints title bars and borders for apps that use the standard frame. Apps that draw their own title
  bar (Chromium, Electron, many WinUI apps) ignore it. We add no per-window code, and the "never
  games, never third-party" allowlist does not change.
- Each design family sets these tokens, so one switch restyles everything.

**What this delivers for native apps:** an accented and dark frame, borders, corners and
backdrop (Mica) on allowlisted windows, and the accent on standard frames elsewhere. It does not
restyle interiors.

**D1, the user's decision: Explorer and Settings interiors.** We never ship a DLL or an injector. If
the user says yes, we add an opt-in that exports our family tokens as a Windhawk XAML-styler theme,
for a Windhawk the user installs themselves, limited to `explorer.exe` and `SystemSettings.exe`.
**Honest reach:**
- **Explorer:** only its XAML parts (the header, command bar, Home and Gallery). The file view
  (DirectUI) and the navigation pane (a Win32 tree) stay stock, because only msstyles reaches them.
  The styler has broken on 24H2+ ([windhawk-mods#3119](https://github.com/ramensoftware/windhawk-mods/issues/3119));
  the user is on build 26300.
- **Settings:** its XAML pages, via the [Settings styling guide](https://github.com/ramensoftware/windows-11-settings-styling-guide).
- **Other apps' interiors:** never.

The S6 check is the Explorer header and command bar plus one Settings page at ≤0.1% idle, and it
fails closed if the styler does not load on this build.

**Never (by us):**
- msstyles via SecureUxTheme or UxTheme patchers, ExplorerBlurMica, StartAllBack-style patching or
  DWMBlurGlass;
- restyling the native taskbar's XAML, FontSubstitutes, or `DwmExtendFrameIntoClientArea` on other
  apps;
- hooks that load our code into other processes, or capturing other apps to fake their chrome.

## Slices, in order. Each merges only if its check passes
- **S0 Attribution (no feature code).**
  - Measure stock Windows, the wallpaper alone, the bars alone, the widgets alone, and everything
    together. Then switch off liquid, glass, widgets and the web wallpaper (replaced by an image) one
    at a time, with a per-page frame trace.
  - Suspects:
    - widget infinite animations (`widgets/weather/style.css:579-615`, `now-playing/style.css:69,179`,
      `media-pill/style.css:110,311`);
    - 1 s timers (`media-pill/main.js:106`, `now-playing/main.js:65`);
    - in the bars, the tint `backdrop-filter` (`style.css:338-339`), the lens path, and the `.bg` filter
      plus the 10 s refresh of the wallpaper background (decision 2). The rAF chain
      (`glass.ts:73-77,93`) is not a suspect: the only infinite bars animation, the launch bounce on
      `.pic`, is outside `CANDIDATES` and is removed after 4 s (`dock.ts:38,236-246`);
    - `backdrop-filter` and the in-page filters in widgets;
    - rain-city's two loops (`script.js:94-102` rain at `settings.fps`, `:314-326` parallax that never
      settles);
    - the WebView2 browser at 17.7%, the third GPU process, and the fullscreen 1.2%.

  ✅ The 38%, the +50% and the web wallpaper cost are each attributed to named causes within ±5%.
- **S1 Zero idle presents and the calm default** (decisions 2, 4 and 5 in one slice).
  - Do every deletion in decision 4, including all `backdrop-filter`, plus everything decision 2
    removes: the `.bg` filters, both request files and the web capture.
  - Bar Liquid becomes the tint:
    - remove `glass.style: "liquid"` from `crates/bars/presets/top/macos.json:54`,
      `dock/macos.json:48` and `top/capsule.json:56`;
    - remove the `[data-liquid]` hover gloss (`style.css:317-322`) and `recapture_glass`
      (`looks.rs:245-252`);
    - remove the fallback that makes bars follow liquid widgets (`crates/bars/src/app.rs:1474-1476`
      setting `look.style = "liquid"`, resolved at `crates/bars/src/look.rs:133-134`).
  - Remove infinite animations from the default widgets.
  - Put ambient motion behind an opt-in "lively" switch, driven by a 10 fps timer, not rAF.
    `frames()` in `widgets/clock/kit.js:147` queues rAF on every vsync, so it presents at the
    display rate.
  - Progress updates every 5 s with no transition; 1 s timers run only while their value is visible
    and changing.
  - Add a `real-look` perf scenario with the frame trace and dwm counters. Gate on the idle fixture
    twice:
    - with the bars set to a wallpaper background over a still image;
    - with a `background.type: blur` dock over maximized apps, which exercises the DWM host-backdrop
      path for the dwm.exe row.

    The web and Liquid variants are diagnostics.

  ✅ The bars, desktop and dwm.exe budgets hold on the idle fixture, 0 unscheduled presents, and
  the screenshot harness looks unchanged apart from the glass.
- **S2 Web wallpapers.**
  - **Required:** in all three copies (rain-city, -amber, -midnight), start the parallax loop
    (`script.js:314-326`) only on the first `mousemove`. Only interactive pages receive mouse input
    (`web.rs:149`), and none of the three sets `interactive`, so today the loop runs and does nothing.
    The page needs no bridge.
  - If S0 shows the 30 fps render loop (`script.js:94-97`) still misses the budget, lower the page's
    own cap in `settings` (`script.js:9`). Fps is the lever (`docs/web-and-app-wallpapers.md:191-193`).
    No new theme fields and no property bridge.
  - Verify the covered suspend (`sources/web.rs:556-567`).

  ✅ ≤8.6% per screen, and 0 when covered.
- **S3 Three families on existing chrome:** macOS-calm, Fluent and an original (mono), through
  `look.family` and the presets. One switch restyles the bars, flyouts, widgets, the D2D menu, the
  DWM tokens and `ColorPrevalence`. No family preset may reintroduce a large-layer `filter`.
  ✅ Within 0.5 points of S1, and the journal round-trip and undo test passes.
- **S4 Icons and flyout polish.**
  - One icon sprite per family: Fluent UI System Icons (MIT) for Fluent, Lucide (ISC) for macOS-calm,
    and the current mono set for the original. No SF Symbols. Add a credits file.
  - Skin the existing flyouts (`bars/shell/src/flyouts/`) to each family's metrics.
  - Add edge-aware placement in `menu.ts`, and give the D2D desktop menu the same metrics.
  - Transform and opacity only; reduced motion is respected.

  ✅ Every flyout presents 0 frames within 1 s of opening, and no ad-hoc icons remain.
- **S5 Liquid Glass on widgets via engine plates (opt-in; bars get none).**
  - Restore the **whole** plate path from `8f925e3^`, not only `crates/widgets/src/plates.rs`:
    - the shell's geometry messages (`widgets/shell/src/frames.ts:115` now draws snapshot plates
      locally);
    - the host's publication and cleanup (`crates/widgets/src/app.rs:1488` now only logs geometry);
    - delete the local snapshot panes in `widgets/shell/src/glass.ts:37-56,85`. Replace the
      acknowledgement `drawsPlate(state) && glass.has(inst.instance)` (`frames.ts:99`) with
      "liquid look **and** a snapshot exists **and** a plate was sent".
    - Do not restore the old `plate: onDesktop()` (`frames.ts:97` at `8f925e3^`). It was true over web
      and app wallpapers too, and gave invisible widgets there.
  - The S5 check also runs over a web wallpaper: 0 captures, and the widgets show the tint.
  - Update `docs/widgets.md` ("Why a snapshot") to say this reverses `8f925e3` on purpose: refraction
    comes only from plates, and web and app wallpapers get a tint.
  - Fix `docs/engine-notes.md:233`, which still calls video native.

  ✅ `glass_cost` ≤0.11 ms GPU per frame at the 4-plate reference (`docs/engine-notes.md`, the
  liquid-glass table). Over a still wallpaper, 0 frames after a settle of at most 1 s. WebView 0
  presents. Side by side with `C:\mypaper-targets\refs\liquid-glass\crops\tahoe-clear-dock.png`.
- **S6 (only if D1 = yes)** the Windhawk token export, with the honest reach above.

**Cut:** the "every app" per-window DWM mode, live notifications (the polls run only while the flyout
or badge is visible, `service.rs:30-33`), the rain-city shader port (it is Three.js plus a photo),
type-ahead, the safe triangle, plates under the bars, and the fullscreen gate.

## Kill criteria
- The bars are above 3% after S1 → prototype the native bar (decision 1).
- S5 misses 0.11 ms or does not settle to 0 → Liquid Glass ships as a tint plus hairline.
- dwm.exe is more than 2% GPU above the same wallpaper with no bars or widgets → use a flat tint.
- The Windhawk styler fails on this build → S6 ships nothing, and D1's interiors are reported as
  unavailable.
- Rejected outright: per-frame capture, an SVG filter or `backdrop-filter` on an idle surface, a DLL
  shipped by us, hooks that load into other processes, an msstyles loader, a new shell, per-widget
  WebViews, or a second menu system.
