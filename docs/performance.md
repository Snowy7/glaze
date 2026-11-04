# Performance

mypaper promises to be light and to be there the moment you sign in. This page says what that
promise is in numbers (`perf/budget.toml`), how the numbers are taken (`tools/perf`), and what
was done to meet them. Every optimization below has a before and an after from the same machine.

Machine: 16 logical CPUs, an AMD iGPU (the wallpaper's) and an NVIDIA RTX 4070 SUPER, two
2560x1440 monitors, one of them covered by a maximized window throughout (so one screen was
visible). Release builds. Other agents were compiling on the same machine during some runs,
so differences under about 100 ms are within noise. Reports: `perf/results/`.

## What you see after sign-in

README-ready (milliseconds from the moment core starts, warm start, `full` scenario unless noted):

| Moment | Before | After |
|---|---|---|
| The theme's look on the desktop | when the engine's first frame lands | **at once**: Windows itself shows a still of the theme ([static snapshot](#static-snapshot)) |
| Native taskbar gone (`bars-hide`) | 156 (cold 434) | **23–60** (cold 84–101) |
| Top bar and dock on screen | 1590 (when WebView2 painted) | **224** (native stand-in), the page at 1606 |
| Live wallpaper, first frame | 1172 | **1083** (shader alone: 1146 → **755**) |
| Widgets painted | 981 | 1143 (about 1100 in all four runs: the shared browser, see 5. below) |
| Core has started every process | 111 | **69** |
| Memory, everything running | 797 MB private / 1009 MB working set | **670 / 741** |

`perf/budget.toml` holds all of this as limits: the final run meets all 102 of them; the baseline
breaks 23.

## Results

`mypaper-perf` before (`2026-11-04-0403-baseline`; the shell scenarios from `-0418-baseline-shell`
and `-0424-baseline-widgets`, re-run after two harness fixes) and after (`2026-11-04-0502-final`;
`-0451-after3` is an earlier run of the same build). Startup in ms from core's creation, warm;
CPU percent of the whole machine with one screen visible; memory private MB, every process of
the tree.

| Scenario | Wallpaper first frame | Shell on screen | Steady CPU | Held CPU | Private MB |
|---|---|---|---|---|---|
| `shader` | 1146 → **755** | | 0.136 → 0.214 | 0.010 → 0.000 | 289 → **261** |
| `image` | 801 → **759** | | 0.010 → 0.000 | 0.010 → 0.015 | 306 → **255** |
| `slideshow` | 1141 → **802** | | 0.000 → 0.029 | 0.010 → 0.005 | 354 → **290** |
| `video` | 953 → 934 | | 0.331 → 0.351 | 0.000 → 0.005 | 404 → **355** |
| `scene` | 950 → **761** | | 0.185 → 0.185 | 0.010 → 0.000 | 410 → **380** |
| `web` | 555 → 570 | | 0.536 → 0.536 | 0.010 → 0.010 | 353 → 353 |
| `widgets` | 797 → 883 | widgets 1937\* → **930** | 0.244 → **0.068** | 0.019 → 0.054 | 551 → **504** |
| `bars` | 1075 → 953 | bars 1455 → **173** (stand-in; page 1662) | 0.146 → **0.019** | 0.049 → 0.005 | 563 → **515** |
| `bars-hide` | 1027 → 927 | bars 1286 → **219** (stand-in; page 1572) | 0.039 → 0.058 | 0.039 → 0.039 | 584 → **533** |
| `full` | 1172 → 1083 | bars 1590 → **224** (stand-in; page 1606), widgets 981 → 1143 | 0.292 → 0.380 | 0.273 → **0.102** | 797 → **670** |

\* The baseline's warm widget paint was inflated by a harness effect fixed later: the previous
start's WebView2 browser was still exiting when the next host attached to the same profile (the
harness now waits up to 15 s for the whole tree between starts). Its cold start was 1242 ms.

Steady and held CPU at this level are a handful of scheduler ticks: process times move in
15.6 ms steps, about 0.01% of this machine per 10 s (the held window is 20 s, the baseline's was
10 s). Held CPU with the shell is mostly the shared WebView2: the bars keep running when paused
(they're the taskbar), and a paused widget layer stays on screen, frozen, for 30 s before it
lets its WebView go. A wallpaper alone holds at 0.000–0.015%.

## What was changed, and what each change measured

Each step was measured on its own build (`baseline` → `after1` → `after2` → `after3` in
`perf/results/`).

**1. Core starts its children first (kept).** Core used to validate the config and theme,
create its tray icon (a round trip to Explorer) and apply Windows styling (registry and DWM
work) before starting anything. Now it takes the instance mutex and the panic hotkey, waits for
Explorer only if Explorer isn't up yet (a logon task can start before it), hides the taskbar when
the bars will, and spawns the wallpaper, widget host and bars; the tray and styling follow.
`core spawn_ms` (all children created), warm: **64–111 → 41–69 ms**. Spawning from threads was
not tried: three `CreateProcess` calls take a few milliseconds together, so there is nothing to
win. Core reaches `main` in about 17 ms warm.

**2. Early taskbar hide (kept, in its second form).** When `bars.json` says `hide`, core hides the
native taskbar before starting anything (journaled through the bars' library, so every exit
path puts it back). Taskbar gone, warm: **156 → 23–60 ms** in two runs (cold 434 → 84–101).
The first version
applied the whole mode (auto-hide too); auto-hide makes Explorer lay out the desktop again,
which held up the wallpaper attaching to it: its windows took 755 ms instead of 60, and its first
frame went from 1027 to 1944 ms. Core now only hides the taskbar windows and leaves auto-hide and
alignment to the bars, which apply the whole mode right after (`HideOnly` in
`crates/core/src/bars.rs`).

**3. The wallpaper opens the GPU on its own thread (kept).** wgpu creates a D3D12 device on every
adapter before it picks one: here an RTX 4070 SUPER listed three times, the AMD iGPU and WARP,
about 350 ms (`request_adapter` alone; creating the chosen device is 35 ms). wgpu has no way to
probe only the low-power adapter (`Adapter::expose` is private), so the open now overlaps
everything the main thread does first (config, theme, desktop host, windows: about 60 ms).
Wallpaper's own time to a ready device, median of the six wallpaper scenarios: **564 → 493 ms**;
to its first frame: **905 → 788 ms**. `wgpu::PipelineCache` would be the next step for shader
compiles but is Vulkan-only, and shader compiles are 20–100 ms of the start here.

**4. Covered screens start paused (kept).** The engine paused a covered monitor only after its
first window event, so at startup it still made a swapchain for it and drew it first. A first
frame costs 150–320 ms on the first swapchain and about 50 ms on the next, nearly all in
creating and acquiring the swapchain (recording and submitting take under 1 ms). Now a screen
that a window already covers starts paused and makes its swapchain when it's uncovered. Own first
frame, median: **788 → 711 ms**, and **27–66 MB less private memory** (no swapchain for the
hidden screen). At a sign-in with no windows open yet, every screen is visible and this changes
nothing.

**5. One WebView2 browser process for the widget host and the bars (kept).** Both hosts now
create their environment through one helper (`crates/desktop/src/webview_env.rs`) with the same
user data folder (`%LOCALAPPDATA%\mypaper\webview2-shell`) and the same options, so WebView2 runs
one browser process (and one GPU and utility set) for both. Their pages have different origins,
so their storage stays apart. `full`, everything running: **797 / 1009 → 658–692 / 734–745 MB**
(private / working set, three runs): about **140 MB private and 275 MB working set** less. A host
that restarts attaches to the running browser: the widget host's first paint after a restart was
**238 ms** instead of 1389.
  - **The cost, measured**: with both hosts starting at once, the widget page now paints at
    about 1100 ms in `full` (1094–1143 in four runs) against 981 in the one clean baseline run,
    so roughly 100 ms later, while the bars' page is unchanged (1590 → 1520–1606) and the bars
    are on screen at 224 ms anyway through their stand-ins. Steady CPU of the WebView2 processes
    in `full` is 0.156% before and 0.195–0.302% after (one 10 s window each, at the noise floor).
    Decision on these numbers: **keep the shared process**; 140 MB is about a third of the
    shell's memory and a crashed host comes back in a quarter of a second.
  - **Crash isolation, verified**: with `full` running, killing the widget host left the bars'
    WebViews running on the same browser process (no `ProcessFailed` in the bars, the same 10
    WebView2 processes); core restarted the widget host, which painted 238 ms later. Killing the
    bars host left the widgets running the same way. Renderers are per site, so a page that
    crashes takes only its own renderer.
  - **The trade-off**: a crash of the browser process itself now affects both hosts at once
    (each already rebuilds its WebViews on `BROWSER_PROCESS_EXITED`; this path wasn't triggered
    by hand). The widget host releasing its WebView after 30 s held no longer frees the browser
    process while the bars use it.

**6. Native first paint for the bars (kept).** The bar window shows nothing until its page has
laid out and set the window's region, about 1.4 s after core starts, most of it WebView2
starting. Each bar now remembers where its page drew (`bars-first-paint.json`), and on the next
start a layered window paints those parts in the bar's current look (the page's own background:
the tint at its opacity, rounded like the page) over the wallpaper backdrop
(`%LOCALAPPDATA%\mypaper\backdrop\<DISPLAYn>.bmp`) when one exists. It goes up as soon as the host
knows the look, before the bars reserve their space, and goes away once the page has painted
(the paint probe) and set its region, or after 15 s. A stand-in only appears where the bar will
land (same monitor, scale and window rect) and never on a monitor with a fullscreen app.
Bars on screen, warm: **1455 → 152–173 ms** (`bars`), **1590 → 207–224 ms** (`full`). The
first version
put the stand-ins up after the bars had registered as app bars (several synchronous round trips
to Explorer): 740 ms; moving them first gave the numbers above.
  - The last values of the modules whose providers answer late (media, tray icons, system
    stats) are kept in the same file and sent to a page that asks before the providers report;
    their first report replaces them (both providers always report an initial state, so a stale
    value can't stick). This is about the first render's content, not its time, so it has no
    timing of its own.

**7. Static snapshot (kept, default on).** See [Static snapshot](#static-snapshot): from the
second sign-in on, Windows shows the theme's still before mypaper starts. About 10 ms of the
engine thread once per theme load, after the first frame.

**8. Release profile (checked, unchanged).** `lto = "thin"`, `codegen-units = 1`,
`panic = "abort"` and `strip = true` were already set. Nothing debug-only runs in release:
wgpu's validation follows `debug_assertions` (release gets only `VALIDATION_INDIRECT_CALL`), and
the only `debug_assertions` switch in our crates picks the log target. Core reaches `main` in
about 17 ms warm and the wallpaper's windows exist 60 ms in, so binary size isn't where startup
time goes; fat LTO wasn't worth a measurement.

**Not done: a native first paint for the widget layer.** The layer is a child of the desktop
window and stays transparent until its page paints, so until then the desktop shows the wallpaper
(the live one or the snapshot) and the icons: no stock UI shows through. Painting the widgets'
cards natively would mean rendering every widget's look twice for about a second.

**Not measured: autostart timing.** The logon task's head start over the `Run` key needs a real
sign-in, which a background agent can't do.

## Limitations

- The wgpu adapter probe (about 350 ms) is the largest part of the wallpaper's start left. It
  needs a way in wgpu to expose only the low-power adapter (or a patched `wgpu-hal`).
- One of the two screens was covered during every run, so steady CPU is for one visible screen.
- Cold starts include whatever the system does on a process's first launch. The harness now runs
  each copied executable once before measuring, since the antivirus scan of a new executable cost
  1 to 7 s on the first start of a session.
- GPU time isn't measured.
- The `bars` steady CPU once read 0.37% (0.32% held) in one run of an unchanged build; every other
  run was under 0.05%. A single 10 s window can catch an unrelated WebView2 background task.

## The budget

`perf/budget.toml` holds the limits; `mypaper-perf` exits with 1 and lists every one a run breaks.
A key is `<component>.<measure>` and binds a scenario only when that component runs in it.
`[default]` applies everywhere, `[scenario.<name>]` adds or overrides.

| Promise | Limit |
|---|---|
| Core has started every process (warm / cold) | 90 / 500 ms (130 warm in `bars-hide`, which hides the taskbar first) |
| Wallpaper's first frame (warm / cold) | 1000 / 1400 ms (1200 in `bars-hide`, 1250 in `full`) |
| Bars on screen (native stand-in, warm) | 300 ms |
| Bars' and widgets' pages painted (warm) | 1800 / 1600 ms |
| Native taskbar gone (warm / cold) | 80 / 150 ms |
| CPU while paused from the tray: wallpaper, core / everything | 0.02% each / 0.05% (0.1% with the bars or widgets, 0.15% with both: the bars keep running, see Results) |
| Idle bars on a still wallpaper | 0.2% |
| Steady CPU per wallpaper kind | image and slideshow 0.05%, shader and scene 0.3%, video 0.4%, web 0.8% |
| Memory, private | from 290 MB (`image`) to 740 MB (`full`), per scenario |

The limits were set from the measurements below, with room for the noise of a busy machine.
Tighten one when a change makes it faster; never loosen one to make a change pass.
`mypaper-perf --check perf/results/<report>.json` evaluates a saved run against the current
budget without running anything (the baseline breaks 23 of the 102 limits; the final run, none).

## The harness: `mypaper-perf`

`tools/perf` is a workspace binary that measures a release build and checks it against
`perf/budget.toml`:

```text
cargo build --release                                  # the build to measure
pnpm -C widgets/shell build && pnpm -C bars/shell build
target\release\mypaper-perf.exe                         # every scenario, writes perf/latest.md
target\release\mypaper-perf.exe --build DIR --label after full bars   # some scenarios, another build
target\release\mypaper-perf.exe --check perf\results\2026-11-04-0502-final.json   # no run
target\release\mypaper-perf.exe --list
```

Run it from the repo root (it reads `perf/budget.toml` and the content from the current folder).
A full run takes about 15 minutes, in sessions of up to 4 minutes.

Options: `--build DIR` (the folder with the `mypaper-*.exe`, default the harness's own folder),
`--content DIR` (the repo: themes, widgets, shell pages, `perf/`; default the current folder),
`--runs N` (starts per scenario, default 4: 1 cold + 3 warm), `--seconds N` (each steady CPU
window, default 10; the held window is twice as long), `--label TEXT`, `--restart EXE` (the core
to start afterwards if none was running), `--hold SECONDS` (keep the last start running, to poke
at it by hand), `--no-write`, `--check REPORT.json`.

It writes `perf/results/<date>[-label].md` and `.json`, and `perf/latest.md` / `.json`. It
exits with **1 when any limit is broken**, listing every one, and 2 when it couldn't run.

### What a run does

1. **Takes the desktop lock** (`%LOCALAPPDATA%\Temp\mypaper-tools\desktop.lock`, an atomic
   `mkdir`, retried every 5 s, removed when older than 5 minutes), shared with every other tool
   that stops the user's mypaper.
2. **Stops the user's mypaper**: `WM_QUIT` to every thread of `mypaper-core`, then waits until
   no `mypaper-core`/`-wallpaper`/`-widgets`/`-bars` process is left. It records the core's path.
3. **Runs the scenarios**, each in a fresh temporary `%APPDATA%` and `%LOCALAPPDATA%`
   (`%TEMP%\mypaper-perf\<scenario>`), so a run never reads or writes the user's settings. The
   executables are copied to `target\perf-stage\run-<scenario>\` (a scenario without widgets or
   bars simply has no `mypaper-widgets.exe` / `mypaper-bars.exe` there, which core reads as
   "off"), from where they find the repo's themes, widgets and shell pages. Each copy is run
   once with an argument that only prints (`--help`), so the antivirus scan of a new executable
   doesn't land in the first start. Between starts the harness waits (up to 15 s) until every
   process of the previous one, WebView2 included, is gone.
4. **Always puts everything back**, also after an error, a panic (the release profile aborts,
   so this runs from a panic hook) or Ctrl+C: stops leftover test processes (only ones running
   from the stage folder), replays any test taskbar journal, puts the Windows wallpaper back if
   it changed, restarts the user's mypaper from its own folder, releases the lock.
5. **Splits itself into sessions** of at most 4 minutes (estimated per scenario), each with its
   own lock, stop and restart, so the user's mypaper is never down for more than 5 minutes.

### The scenarios

Fixed test configs, generated per run: one theme of each kind alone, then the shell on a still
wallpaper (so its own cost shows), then everything.

| Scenario | Wallpaper | Widgets | Bars |
|---|---|---|---|
| `shader` | `aurora` (60 fps) | off | off |
| `image` | `photo` | off | off |
| `slideshow` | `gallery` | off | off |
| `video` | `earth-orbit` | off | off |
| `scene` | `islands` (30 fps) | off | off |
| `web` | `webdemo` | off | off |
| `widgets` | `photo` | clock, system monitor, now playing | off |
| `bars` | `photo` | off | top bar + dock, taskbar kept |
| `bars-hide` | `photo` | off | top bar + dock, taskbar hidden |
| `full` | `aurora` | the three widgets | top bar + dock |

The theme is a copy of the bundled one with its asset paths made absolute and **its `windows`
styling section removed** (styling would change the user's registry). `config.json` turns off
fullscreen and battery pausing (the user's own windows and power source must not hold a run) and
`static_snapshot` (runs never touch the Windows wallpaper). The widgets go on the first monitor
no window covers, since a covered monitor hides its widget layer.

### What is measured

**Startup.** Every component logs a structured marker when it reaches a milestone, measured from
its own process creation time (`GetProcessTimes`), so the loader and runtime start count too
(`crates/desktop/src/startup.rs`):

```text
READY component=wallpaper first_frame_ms=84.2 epoch_us=1793612345678901
```

`epoch_us` is the wall clock then. The harness reads core's creation time from its process
handle and reports every milestone **from core's creation**: what a user sees after sign-in.

| Marker | When |
|---|---|
| `core main_ms` | `main` reached (loader and logging done) |
| `core taskbar_hidden_ms` | the native taskbar hidden (only when `bars.json` hides it) |
| `core spawn_ms` | every child process created |
| `wallpaper windows_ms` / `device_ms` / `source_ms` / `loop_ms` | windows made / GPU device ready / theme loaded / main loop entered |
| `wallpaper first_frame_ms` | the first frame presented (a `web`/`app` source: first laid out) |
| `widgets first_paint_ms`, `bars first_paint_ms` | the page has its first data on screen |
| `bars native_paint_ms` | the native stand-in of a bar is on screen (see below) |

*First paint* is the paint probe (`crates/desktop/src/webview_env.rs`): right after the host
sent a page its first state, it runs `requestAnimationFrame(() => requestAnimationFrame(...))`
in the page, which answers once the frame holding that state was produced.

A start counts as **cold** when it's the first on a fresh profile (new WebView2 user data
folder, no caches, executables just copied) and **warm** for the starts after it; warm is the
median of 3.

The native taskbar's disappearance (`bars-hide`) is timed by the harness itself: a thread polls
`IsWindowVisible(Shell_TrayWnd)` every millisecond from just before core starts.

**Steady state.** After the last start settles (5 s: theme load, the 2 s working-set trim,
page loads), the harness samples kernel+user CPU time of every process in core's tree, twice,
10 s apart, and reports percent of the whole machine (what Task Manager shows) and of one core.
Then it pauses mypaper the way the tray does (`paused: true` in `config.json`), waits 3 s, and
samples for twice as long (process times move in 15.6 ms ticks, so a 20 s window resolves about
0.005% of this machine): the **held** cost.

**Memory.** Private bytes (committed) and working set of every process in the tree, at the end of
the steady window.

**Attribution.** The tree is core and everything it started, parents before children, a process
only counting as a child when it was created after its parent (Windows keeps dead parents' ids
and reuses them: without that check a Spotify started long ago showed up as a child of a
WebView2 process). mypaper's own processes are themselves. A `msedgewebview2.exe` belongs to the
host whose **user data folder** its command line names (`--user-data-dir`, read with
`NtQueryInformationProcess`): `webview2-widgets`, `webview2-bars`, `webview2` (web wallpapers),
or, for the folder both shell hosts share, **shared WebView2**. A WebView2 process whose command
line names no folder follows its parent. Anything else (plugins, an app wallpaper) counts for
its parent.

**Coverage.** The user's own windows stay as they are, so a monitor covered by a maximized window
pauses its wallpaper and widgets during a run. The report says how many screens were visible;
CPU numbers are for that state.

**GPU time** is not measured (D3D12 timestamp queries would need a hook in every source; the CPU
side and the frame pacing are what the budgets bind).

### The pure parts

Marker parsing (`markers.rs`), CPU sampling math (`cpu.rs`: deltas, new processes, reused pids,
medians), budget evaluation (`budget.rs`), the process tree and user-data-folder parsing
(`procs.rs`), grouping (`run.rs`) and the report cells (`report.rs`) have unit tests.

## Static snapshot

`static_snapshot` in `config.json` (default **on**): after each theme load, the engine renders the
theme's first frame once more per screen at full size, and a background thread encodes it as a
JPEG under `%LOCALAPPDATA%\mypaper\snapshot\` and sets it as **that monitor's Windows wallpaper**
(`IDesktopWallpaper::SetWallpaper`). So from the next sign-in on, before mypaper has even
started, Windows itself shows the theme's look instead of the stock wallpaper, and the live
wallpaper takes over from an identical still.

- **Reversible.** Before the first change, each monitor's original wallpaper (and the global fit
  mode, if it was *span*) goes into `%LOCALAPPDATA%\mypaper\wallpaper.json`. A wallpaper the user
  picks themselves while mypaper runs becomes the new original. The originals come back when
  `static_snapshot` is turned off (live), on the **panic hotkey**, and on
  **`mypaper-core --autostart off`** (Studio's "Start with Windows" off: mypaper won't run at
  sign-in, so Windows shouldn't show its still). A normal quit keeps the still: that's what Windows
  shows until mypaper starts again. The API is `mypaper_desktop::desktop_wallpaper::restore()`.
- **Cheap.** About 10 ms of the engine thread once per theme load (one extra draw per screen and
  a readback), after the first frame is already on screen; nothing at all when the theme's files
  haven't changed (the still is named by a hash of the theme folder's file metadata and the screen
  size, and a monitor already showing it isn't set again). Encoding and `SetWallpaper` (about
  100 ms, Explorer re-renders its desktop) run on a below-normal thread.
- **Not for every theme.** `web` and `app` sources have no frame to read back, a theme that failed
  to load shows the red screen, and an all-black first frame (a video not decoded yet) is retried
  up to 10 times: none of those become a still. A slideshow or Spotlight background of the user's
  is never replaced (a path can't bring it back). The dev window (`--windowed`) never touches the
  Windows wallpaper.
- **Tested** with a temporary journal and folder; the real-wallpaper round trip
  (`snapshot::tests::real_wallpaper_round_trip`, `#[ignore]`) left both monitors exactly as before
  (`D:\Pictures\Wallpaper\bg.jpg`) in three runs. The perf harness runs with it off and also checks
  the Windows wallpaper before and after every session.

## Autostart

`mypaper-core --autostart on|off|status` prints one JSON line
(`{"enabled":true,"method":"task","command":"\"C:\...\mypaper-core.exe\""}`), and Studio's
"Start with Windows" runs it.

- **On** registers a per-user **Task Scheduler logon task** (`mypaper (<user>)` in the root folder)
  through `ITaskService`: a logon trigger for the current user only, no delay,
  `TASK_LOGON_INTERACTIVE_TOKEN`, `TASK_RUNLEVEL_LUA` (limited, no elevation), no execution time
  limit (the default 72 h would end mypaper), priority 4 (normal; the default 7 is below-normal CPU
  *and* low I/O priority), no battery conditions, hidden, ignore-new for a second instance. The
  `Run` value is then removed so Explorer doesn't start a second copy. If the task can't be
  registered (Task Scheduler unavailable, policy), it falls back to the `HKCU\...\Run` value.
- **Answer to research section 7:** **yes, a limited per-user logon task can be created without
  admin rights.** Verified from a non-elevated session (medium integrity, the Administrators group
  deny-only): `RegisterTaskDefinition` succeeds in the root folder and in a subfolder (created on
  the fly), the read-back matches (trigger user, LUA run level, PT0S, priority 4, battery flags),
  and an on-demand `IRegisteredTask::Run` completes with `LastTaskResult` 0. All with test task
  names, deleted afterwards (`cargo nextest run -p mypaper-core --run-ignored only`).
- Task Scheduler starts the task at logon itself, not from Explorer's `Run` processing, which
  Microsoft says it may delay. Core therefore can start **before Explorer**: it waits for
  Explorer's desktop window (checking every 25 ms, up to a minute) before starting its children,
  and the bars hide the taskbar the moment it appears. How much earlier than the `Run` key it
  starts was not measured: that takes a real sign-in.
