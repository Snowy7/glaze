# Performance

mypaper promises to be light and to be there the moment you sign in. This page says what that
promise is in numbers (`perf/budget.toml`), how the numbers are taken (`tools/perf`), and what
was done to meet them. Every optimization below has a before and an after from the same machine.

Machine: 16 logical CPUs, an AMD iGPU (the wallpaper's) and an NVIDIA RTX 4070 SUPER, two
2560x1440 monitors, one of them covered by a maximized window throughout (so one screen was
visible). Release builds. Other agents were compiling on the same machine during some runs,
so differences under about 100 ms are within noise. Reports: `perf/results/`.

## Native engine scenarios (N0.3 / N2)

The historical tables below used one visible screen and the WebView shell. The native engine
fixture instead requires **two visible 2560x1440 screens**, a bar, dock and six widget plates
on **each** (16 plates total). `tools/perf` refuses covered screens rather than reporting a
paused engine as a two-screen result. This is an engine/geometry fixture, not an N4 shell
memory or whole-product measurement.

Build only the needed release binaries and plate example, then run:

```powershell
$env:CARGO_TARGET_DIR = 'C:/mypaper-targets/native-engine'
cargo build --release -p mypaper-wallpaper -p mypaper-perf --example plates --bin mypaper-wallpaper --bin mypaper-perf
$build = 'C:/mypaper-targets/native-engine/release'
& "$build/mypaper-perf.exe" --native --build $build --content $pwd.Path --seconds 60 --runs 3 real-look
& "$build/mypaper-perf.exe" --native --build $build --content $pwd.Path --seconds 60 --runs 3 engine-aurora
& "$build/mypaper-perf.exe" --native --build $build --content $pwd.Path --seconds 30 --runs 1 --timestamps real-look
& "$build/mypaper-perf.exe" --native --build $build --content $pwd.Path --seconds 20 --runs 1 --timestamps --composition glass-motion
& "$build/mypaper-perf.exe" --native --build $build --content $pwd.Path --seconds 20 --runs 1 --timestamps still-plates
py -3 tools/perf/summarize-gpu.py "$build/real-look.run0.jsonl" --window "$build/real-look.json"
```

`--exercise-settings --timestamps` checks a live 12 fps cap (20-28 frames per screen in a two-second
window) and integrated/high-performance/auto/revert GPU restart requests, ending on integrated.
The fixture lives through the restarts. Normal runs use default integrated selection and no
user cap; renderer keys and precedence are documented in [engine-notes.md](engine-notes.md#renderer-settings-gpu-and-frame-cap).

Native scenarios are `real-look` (rain-city), `engine-aurora` (full-scale), `glass-motion`
(rain-city plus moving docks), `still-plates` and `still-motion` (photo). The harness takes the
shared desktop lock, stops the user's core, saves/minimizes covered windows, launches only the
engine and plate fixture in isolated settings, samples private bytes and process CPU time,
then stops both, restores window placements and restarts the original core. Sessions remain
under the five-minute lock expiry. CPU is reported in **both** percent of one core and percent
of the machine; existing `perf/budget.toml` values retain their machine-percent meaning.

`--output <path.json>` saves each run; report medians of three independent 60-second runs.
`--timestamps` enables wgpu GPU queries and saves a separate JSONL per run. Keep it disabled
for CPU/private-memory baselines. `--theme <path>` and `--capture <absolute.png>` provide live
before/after weather captures through the same engine; captures retain physical resolution.
The harness restores taskbar/wallpaper state through the existing Session cleanup. It neither
hides the stock taskbar nor starts native shell content during the engine fixture.

Timestamps split wallpaper rendering, swapchain restore/backdrop work, and glass. The GPU
summarizer reports median/p95/max per screen plus the **sum of both screens' plates** for each
engine frame. Geometry-only rows have `wallpaper_drawn: false`. Publish-to-submit is delivery
latency to the CPU queue submission, **not** proof of compositor-visible lag; a display-latency
claim uses the separate `--composition` path: DwmFlush runs once after both screens present
and adds `publish_to_composition_ms`. It is a conservative two-screen upper bound and changes
pacing, so it is excluded from idle CPU samples. `--window <sample.json>` on the summarizer
selects the exact CPU window and excludes startup/teardown publications. The async slots may omit a
sample if all are still mapping, and no timestamp work runs when telemetry is disabled.

### Shell idle hill-climb (6 November 2026)

On the user's live desktop (two 2560x1440 screens, the left one with bars, dock and widgets, the
right one covered by a maximized app; a seconds desk clock, a stats module and a now-playing
widget visible), with other agents building and deploying throughout, so whole-process samples
swing. The wake report is the measure: create `%LOCALAPPDATA%\mypaper\wakes.on` (or deploy with
`MYPAPER_WAKES=1`) and every minute `mypaper.log` gets a `wakes in 60 s:` line with causes, each loop
part's wall time and CPU cycles (`cpu Mcycles`, about 4.5 per ms here), and `bars.tick ms/Mcycles`
by part. Delete the file to stop it; reading the switch costs one file check a minute.

What woke the shell, and what each fix did (quiet minutes, UI thread):

| Source | Before | After |
|---|---|---|
| Every data change (the clock each second) marked all four bars dirty, rebuilt dock items and re-read every app window's rectangle | items 15.3, windows 26.7, paint 62.2 ms/min | 1.0, 2.1, 51.9 ms/min (only bars showing the topic) |
| Top bar clock `HH:mm` (macOS preset) with `seconds: true` ticked every second | bars.tick 270 Mcycles/min | 126-139 (ticks when the format shows seconds) |
| Bars clock (+5 ms), stats (every 2 s from start) and widget clocks (+0 ms) woke separately | 41 bar clock/stats wakes/min | 13-16; the rest share the widgets' wake |
| WinEvent hooks on the UI thread: every cursor move over the foreground app delivered there | UI thread 1,533 Mcycles in 15 s of mouse motion (146 still) | 89 (96 still); a `win-events` thread takes 173 in 25 ms batches |
| Each window event read all 341 top-level windows' class, pid and cloak state (35 visible), up to three times | 0.46 ms per pass | hidden windows answered by one visibility check; the z-order stops at each bar monitor's front app |
| Any `WM_SETTINGCHANGE` reloaded every setting (a tweak burst: 120-199/min, pump 4.9-7.2 s/min) | full reload each | only when accent, scheme or animation changed |
| Media timeline updates (13/min) repainted every bar | 13 full repaints/min | none (only notifications changes repaint) |

Whole UI thread in a quiet minute: 582 -> 444-478 Mcycles (about 0.1 s of CPU a minute). What is left
is visible change: the seconds desk clock's repaint (~4 Mcycles each, about 260 a minute) and the
stats module (every 2 s). No frame-ticker wakes at rest.

Mouse motion still costs the desktop menu's `WH_MOUSE_LL` thread about 930 Mcycles per 15 s of fast
motion (~33 µs a move, in the kernel round trip): the hook must see every mouse event to swallow
right-clicks on the bare desktop, and is already off while a fullscreen app is in front.

Memory: resting 35-37 MiB private. The first launcher open keeps about 2.5 MiB (text field, shell
and icon state); further opens, the dock menu, hover and flyouts stay within it (39-40 MiB), and
handles and GDI/USER objects return to their resting counts.

Exit: the shell no longer crashes on every exit (0xc0000409, or 0xe06d7363 in unwinding debug
builds): the integrations' thread-local state joined its workers after `ExitProcess` had killed
them; `integrations::shutdown()` now joins them before `main` returns.

### Live engine hillclimb (6 November 2026)

On the user's live desktop, not the fixture: rain-city at 30 fps and 0.65 scale on the RTX
(`engine.gpu: auto`), DISPLAY1 covered by a maximized window (paused), DISPLAY2 visible with a
bar and dock, Studio open. CPU is % of one core (`measure.ps1`, 30 s, or per-thread counters).
Other agents built and deployed throughout, so whole-process CPU swung by +-1 point between
identical runs; wins below are interleaved A/B or counted in wakes and milliseconds.

**What was wrong.** At 02:40 the engine used 27.1% of a core, 8x the fixture. Its main thread was
answering Studio's glass preview 6,100-6,700 times a second: previews were served only from the
first screen's kept frame, that screen was covered (never drawn), so every answer was "no frame",
and Studio asks again as soon as it is refused. Each round trip opened, mapped and unmapped
Studio's 10 MiB section. Studio itself spun at ~100% of a core.

| Commit | Change | Before | After |
|---|---|---:|---:|
| `bac945a` | Previews draw from any screen's drawn frame; a repeat of a refused request waits for a frame | 25.99, 25.94% | **3.33, 4.38, 2.92%** (Studio 100% -> 0) |
| `7270919` | Shader inputs as immediate data (`var<immediate>`, root constants); an unchanged blit uploads nothing | 0.293-0.302 ms CPU per frame (lab, RTX) | **0.261-0.262 ms**, frames bit-identical |
| `2efc0ee` | The engine's WinEvent hooks run on their own thread, in 50 ms batches (`crates/desktop/src/cover.rs`) | main thread 1,196 wakes/s, +0.8-1.6 points while the mouse moves | **96 wakes/s**, no measurable cost |
| `6d11015` | Preview glass forks the desktop's compiled renderer | 7.0, 9.1 ms per preview (median) | **5.3, 2.7 ms** |
| `0bd9a57` | No D3D12 debug names (`DISCARD_HAL_LABELS`; wgpu named a command list per pass, every frame) | glass-only 0.229-0.298 ms CPU per frame (lab) | **0.221-0.230 ms** |

After them, rain on one visible screen is 1.9-2.9% of a core (the `measure.ps1` and A/B runs
above) and the main thread's frame is wgpu and the driver: present 32%, command encoding 31%
(half of it NVIDIA's command-list reset), submit 16%, liquid-rust 3%. That is one encoder, one
submit and one present per frame, the minimum for this design. GPU: 0.2 ms per frame on the RTX
(frame lab). Plates are published 0.15 times a second while the desktop is idle.

**What stays open, and the trade-offs.**
- *Covered screens: 0.31% of a core, not 0.* With every screen covered the engine draws nothing,
  but three NVIDIA driver threads still wake about 65 times a second each while the D3D12 device
  exists (197 wakes/s in all). Round 2 (below, "Hidden = free") releases everything but the
  device after 10 s fully covered and measured the device itself.
- *Still image.* Not measured live (it would switch the user's theme). It draws nothing; what
  remains is the covered case's driver wakes plus window events, now batched off the frame loop.
- *Memory: 169 MB private steady on the RTX.* A wgpu device here is 92-112 MiB of it (a bare D3D12
  device alone is 57 MiB). wgpu creates a D3D12 device on all five adapters DXGI lists (the RTX
  three times, the AMD iGPU, WARP) before choosing; exposing only the screens' adapter saves
  about 13 MiB (lab, two runs each), which needs a patched wgpu-hal (`Adapter::expose` is
  private): not done. The ~60 extra threads and ~+40 MB seen some runs (205-212 MB / 80 threads)
  are not a shader-cache miss: round 2 traced them to the first glass overlay's D3D11 device.
- *Studio (not engine code).* It asks again right after a refusal, and waits forever on an
  engine that restarted mid-request (its done event never comes); the engine now copes with
  both, but Studio should stop re-asking an unchanged refused request and time out a request.

Tools (`C:\mypaper-targets\engine-perf\tools`): per-thread CPU and wakes, the preview request
rate and round trip, plate publication rate, cursor-motion and covered-screen probes, and an
engine A/B swapper (`swap-engine.ps1`) that puts another exe live and restarts only the engine.

### Hidden = free (6 November 2026, round 2)

Same live desktop and caveats as above. The decision (made for the user, who counts RAM as much
as CPU): once every screen has been covered by maximized or fullscreen apps for 10 s, let go of
what drawing holds, keep the theme's still on the desktop, and come back the moment a screen is
uncovered, with the still covering the way back so nothing goes blank or flashes.

**What each level frees and what coming back costs** (`lab::release` and `lab::idle`: the
engine's own device path, `engine.gpu: auto` = the RTX 4070 SUPER, rain-city, three cycles):

| Released | Frees (private) | Back to a first frame |
|---|---:|---:|
| Drawing: swapchains, kept frames, blur and backdrop pyramids, the source; the device stays | live **-46 MiB** (one screen's swapchain 28, kept frame 6, baked rain 6, glass and backdrop pyramids); textures on this driver are process memory too | **33-62 ms** live (the source again on the warm device: 19-30 ms) |
| + the wgpu device, its adapter kept | +4 MiB more (the adapter holds the D3D12 device) | +48 ms |
| + device and adapter (no wgpu at all) | +38-40 MiB more; the driver's one ~66 wakes/s thread stops (`lab::idle`: 66 -> 0 wakes/s) | ~1.6 s: open 0.4-0.5 s, rain 0.16 s, liquid-rust's glass 1.0 s (a new device compiles every shader again with FXC) |

So the engine releases the drawing and keeps the device. Releasing the device too was over the
~500 ms budget, and glass overlays (flyouts and menus over apps) draw with that device: the first
flyout after it would wait ~1.6 s for its glass. It would also free less than it looks: with no
wgpu object left, the process still held 143-287 MiB in the lab (driver and compiler memory a
process never gets back), so only a process exit returns the rest.

**What it does** (`Engine::release`, `Engine::resume`). After 10 s with every screen covered,
and only while Windows shows this theme's still on every screen (`static_snapshot`, confirmed by
the snapshot worker; with it off the user's own wallpaper would flash by on the way back, so
nothing is released): the windows hide, so the still shows in their place; every swapchain,
kept frame, glass pyramid and backdrop pyramid is dropped, and so is the source when loading it
again draws the same frame (`Source::releasable`: shaders and still images; a video, slideshow
or scene would restart from its first frame, so it stays loaded). When a screen is uncovered the
source loads again, the windows show (empty, the still shows through) and the next frame makes
the swapchain. Quick alt-tabs inside the 10 s change nothing. Glass overlays keep working while
released: a notifications flyout opened over a maximized window then got its glass 44 ms after
opening (`flyout-released.ps1`).

| Live (`release-check.ps1`: DISPLAY1 covered, a window of our own maximized over DISPLAY2) | Covered, inside the 10 s | Released |
|---|---:|---:|
| Private | 171, 173 MiB | **123, 124, 127, 132 MiB** |
| Wakes, CPU | 135/s, ~0% | 139/s, ~0% |
| Releasing | | 4.4-13.3 ms on the main thread (14 releases) |
| First frame after the uncover was seen | | **33-62 ms**, median 47 (14 resumes) |

The memory comes back about 3 s after the release (the driver frees the swapchain late:
`release-course.ps1`). Five release/resume cycles in a row resumed at 182, 184, 184, 184, 182
MiB: nothing grows per cycle. 101 captures of DISPLAY2 through two uncovers show the still, then
rain: never black, never another wallpaper.

**Also found on the way**

| Commit | Change | Before | After |
|---|---|---:|---:|
| `ef7ad19` | A glass spring caught mid-flight by a window maximizing over its screen kept waking the engine every 4 ms while the screen stayed covered (springs only advance as a screen is drawn); only drawn screens count now | main thread 305 and 278 wakes/s, 0.78% of a core (2 of 3 covers) | **15-28 wakes/s**, 0-0.2% (3 of 3) |
| `67d1c8e` | The overlays' D3D11 capture device is created with `PREVENT_INTERNAL_THREADING_OPTIMIZATIONS`: NVIDIA's D3D11 driver started ~56 worker threads with the first overlay of a process | 80, 81 threads with a flyout open; 212, 194 MiB | **42, 45 threads**; 192, 192 MiB; CPU no worse (3.5, 5.1% vs 5.6, 6.6%) |

The first overlay of a process still adds ~+20-40 MiB (the D3D11 device on NVIDIA); in 3 of 4
runs it went back to ~176 MiB once the capture went cold, 3 s after the last overlay closed.

**Memory while visible, item by item** (rain-city, one visible screen, RTX; `lab::memory` and
the live numbers above; private, MiB):

| Item | MiB | |
|---|---:|---|
| wgpu device and driver | ~98-100 | needs the patched wgpu-hal (adapter probing) to shrink |
| shader compilation (FXC, driver) | ~17 | per process, kept |
| city.jpg with its mips (2560x1111) | 15 | the look depends on its trilinear mips |
| swapchain, 2 x 2560x1440 | 28 | the minimum two buffers |
| glass blur pyramid (half size, Rgba16Float, 7 levels) | 9.4 | per kept frame; 8-bit would band |
| kept frame (1664x936) and baked rain | 6 + 6 | |
| backdrop pyramid between publishes | ~5 -> 0 | dropped after each publish since round 2: settled private 165.7, 169.5, 169.7 MiB against 170.6, 171.1, 173.2 interleaved (its smaller levels share 4 MiB allocator blocks, so 1.6-4.9 MiB comes back) |

What stays open: the driver's device thread (~66 wakes/s, ~0% CPU) while released, which only
releasing the device removes (above: 1.6 s back and slower first flyouts); rain at 2.3-3.3% of a
core visible, of which the main thread's frame is ~0.8 ms and recording plus submitting it
0.22 ms (lab, RTX): the rest is acquire, present and the driver (round 1's profile), one of each
per frame; and the device's ~100 MiB, which needs the patched wgpu-hal.

### Games (6 November 2026)

Core now publishes the conservative fullscreen/game hint by default; explicit
`system.game_activity: false` drops the publisher and the shell's signal subscription. The
[Games contract](system.md#games) defines the lifetime-checked core → child snapshot and
`HostMsg::GameActivity(bool)` relay. Index background work uses the same snapshot.

During a game the shell releases background provider subscriptions and freezes widget work;
clock alignment, stats and custom-command deadlines are absent on every bar. The helper blocks
with no background deadline: plugin timers, retries, move coalescing and idle unloads wait.
Incoming state coalesces, ordered pipe data uses its existing read-credit bound, and polling
writes wait without closing connections. Resume delivers retained state and fires an overdue
periodic timer once, without a catch-up burst. Independent user/session holds remain in force.

The focused tests cover a 10-minute logical hold without events/deadlines, latest state on
resume, one overdue timer, deferred opens/removal, independent holds, an unchanged real
websocket connection, and 32 pipe pause/resume cycles with no incoming peer traffic. All five
pass. Core/config/wire tests pass; shell has 143 passing tests. Widgets has 146 passing tests
with one unrelated registry-count assertion excluded (`registry.widgets.len() >= 23`).

The production helper was also measured in an isolated profile with Hello's one-second timer
(`crates/plugins/examples/quiet_helper.rs`), two 60 s samples. CPU was below the process-time
counter's resolution in both samples; pausing removed timer deliveries rather than claiming
a CPU improvement from two rounded zeros. [Raw result](../crates/core/evidence/game-helper.json).

| Isolated helper, 60 s each | Active | Game-paused |
|---|---:|---:|
| Plugin view updates | 48 | **0** |
| CPU / one-core % | 0 ms / 0% | **0 ms / 0%** |
| Private at end | 4.39 MiB | **4.39 MiB** |

Resume and clean shutdown passed; the temporary profile and both probe processes were removed.

**Limits:** no application background deadline is different from no OS/driver wake. The engine
keeps its GPU device when covered (the ~66 wakes/s driver thread described above). WinHTTP's
transport keepalives and external incoming events remain while sockets stay connected; changing
keepalive on an upgraded live socket was rejected with `ERROR_WINHTTP_INCORRECT_HANDLE_STATE`.
Processes retain memory. No FPS or game frame-time improvement is inferred from quiescence.

### Engine hillclimb (5 November 2026)

Same machine, fixture and harness as the N0.3 measurements below (two visible 2560 x 1440
screens, 16 clear plates, default integrated GPU, rain at 30 fps and 0.65 scale), on the
final build (`bdefbb0`). Visual quality is unchanged: every blur tap,
layer, render scale and frame rate is kept (proof below). Each change was measured and
committed on its own; `docs/engine-notes.md` (Memory, Shader image inputs) explains them.

| Metric | Before (N0.3) | After | Gate |
|---|---:|---:|---|
| Engine private memory, rain (median of 3 x 60 s) | 448.07 MiB | **257.11 MiB** (261.84 / 253.18 / 257.11) | new gate 272.4 MiB, below |
| Engine private memory, aurora (median of 3 x 60 s) | 420.21 MiB | **262.35 MiB** (259.24 / 263.15 / 262.35) | |
| Engine private memory, still photo (60 s) | 363.98 MiB | **233.65 MiB** | |
| Interleaved A/B on the same desktop, rain (baseline / final, two pairs) | 375.18, 445.96 MiB | **253.77, 255.04 MiB** | |
| Rain CPU, % of one core (same A/B pairs) | 4.369, 2.837% | **2.837, 2.369%** | <=4%: pass in the pairs; the busy 3 x 60 s below is above |
| Rain CPU, the final 3 x 60 s above | 2.759% (N0.3, quieter desktop) | 4.580% (5.145 / 4.580 / 4.242) | see note |
| Still CPU, % of one core (interleaved A/B pair) | 0.156% | 0.182% | 0%: **open** |
| GPU per engine frame, both screens, rain (median / p95) | 10.749 / 11.203 ms | **5.410 / 5.762 ms** | |
| GPU per screen: that divided by two | 5.37 ms | **2.71 ms** | <=2 ms: **open**, see below |
| Rain wallpaper pass | 4.32 + 3.82 ms (one per screen) | **3.09 ms** (once for both) | |
| Restore, per screen | 1.12 / 1.14 ms | 0.95 / 1.11 ms | |
| All 16 plates, summed (median / p95) | 0.402 / 0.460 ms | **0.275 / 0.322 ms** | <=0.3 ms: **pass** (median) |
| Glass-only lag, publish -> DWM completion bound, median (DISPLAY1 / DISPLAY2) | 7.49 / 5.86 ms | **6.52 / 6.11 ms** | <=1 frame (4.17 / 5.00 ms): **open** |
| same, p95 | 11.98 / 8.31 ms | **8.86 / 7.93 ms** | |

Measured wins, each in its own commit (live: 15-30 s probe runs; offline: `src/lab.rs`):

| Commit | Win |
|---|---|
| Exact-size GPU heaps (4 MiB blocks) | 450.2 -> 398.7 MiB |
| Backdrop from the kept frame + palette from the kept frame | 398.7 -> 366.1 MiB |
| One wallpaper pass and kept frame for screens showing the same frame | GPU per engine frame 10.75 -> 7.25 ms; 366.1 -> 354.5 MiB |
| Restore without clear or blend | 7.25 -> 6.68 ms; restore 1.27 / 1.33 -> 0.89 / 1.08 ms |
| Rain: baked drop-free frame | offline rain pass 3.69 -> 2.88 ms (+5.9 MiB, one texture) |
| Rain: layer gradients only where the field slopes | offline 2.88 -> 2.52 ms; live engine frame 6.68 -> 5.48 ms |
| Glass: per-plate constants, zero-weight mixes skipped | offline 0.133 -> 0.102 ms per screen; live 16 plates 0.354 -> 0.280 ms |
| 16k view descriptors instead of 1M | 361.7 -> 276.0 MiB |
| One backdrop readback for screens sharing a kept frame | 276.0 -> 257.7 MiB |

**Quality proof.** `cargo test -p mypaper-wallpaper lab::lab -- --ignored --exact` renders
one screen of the engine's real path (wallpaper into the kept frame, restore, the fixture's
eight plates) offscreen on the iGPU and dumps raw frames; `tools/perf/diff-frames.py` compares
two captures. Frames at 3, 12 and 30 s from the original engine against the final one, final
2560 x 1440 frames with glass and the kept frames: rain-city, rain-city-amber and
rain-city-midnight differ by at most **1/255** in at most 0.0005% of pixels (the baked frame's
compiled blur rounds differently in a handful of pixels); snow-city, snow-forest and aurora are
**bit-identical**. The restore and glass changes are bit-identical on their own. Both screens
of a live capture with the shared kept frame are pixel-identical, as before. The scaled-rain
backdrop BMP differs by at most 2/255 (mean 0.12/255) after its 1/64 blur; palette.json is
unchanged.

The baseline engine's own memory moved between 375 and 446 MiB across today's runs, most
likely because its growing 8-64 MiB suballocation blocks were reused or not depending on
timing (not verified further). The final
engine's stayed within 253-263 MiB. CPU depends on the desktop: another agent restarted its
shell about once a minute during the final 3 x 60 s rain runs, and every window event costs
the engine a cover rescan. The interleaved pairs (baseline, final, baseline, final, one minute
each) compare the builds under the same conditions; the final build is lower in both pairs.
Its A/B runs also carried the hook filter tried under Still CPU below, which only changes
event handling and was dropped.

**What stays open, and why.**
- *GPU <= 2 ms per screen.* Rain's own pass is 3.09 ms for both screens, then each screen
  restores (upscales) the kept frame into its swapchain, 0.95-1.11 ms, and draws its plates.
  The same restore costs 0.42 ms into an offscreen texture: writes into these swapchains, an
  iGPU swapchain shown on monitors driven by the RTX, are about 2.5x slower, and wgpu offers no
  control of how DXGI allocates them. What remains in the rain pass is the drop field's ALU
  (about 400 instructions per pixel) and the 18-tap blur where drops refract; exact
  early-outs beyond the two above measured slower. Per engine frame the iGPU is busy 5.4 ms
  of 33 ms (16%).
- *Lag <= 1 frame.* With the minimum two flip buffers a glass-only update can start only after
  DWM releases the previous image at a vblank, and shows at the next: publish -> composition
  stays near 1.5 frames even with no GPU time. A third buffer per swapchain would remove up to
  a frame of it at about +28 MiB or more per screen; that trade is the user's to make.
- *Snow.* The snow ports cost about 18 ms per frame on this iGPU (50 layers of a hash whose
  division by a fractional part moves flakes by pixels on a one-ulp change, so no cache of it
  can be exact). Sharing one pass between both screens halves it to 18 ms per engine frame
  (54% of the iGPU at 30 fps). Aurora (an example theme) costs about 20 ms per screen.
- *Still CPU 0%.* Over a still photo the engine submits nothing, but it wakes for every
  window event other processes cause (WinEvent hooks in `crates/desktop`) and rescans what
  covers each monitor after each burst: 0.13-0.21% of one core on this busy desktop, 0.078%
  on N0.3's quieter one. Ignoring child and tool windows in the hook was tried and showed no
  measurable difference here, so it was not kept.
- *Memory floor.* Probing the RTX and WARP before choosing the iGPU leaves up to about 25 MiB
  (the single-adapter wgpu-hal patch is N5); the two swapchains cost +63 and +40 MiB for
  2 x 2 x 14 MiB of pixels.
- *First static snapshot of a theme* still renders each screen at full size (and, for rain, a
  full-size baked layer), once per theme and screen size; not part of this fixture.

Results: [`tools/perf/results/native-2026-11-05`](../tools/perf/results/native-2026-11-05).

### Native measurements (4-5 November 2026)

Release build, AMD Radeon(TM) integrated GPU (DX12/FXC), Ryzen 7 7800X3D / 16 logical
CPUs. Both physical 2560 x 1440 displays were visible throughout every accepted sample:
DISPLAY1 at 240 Hz, DISPLAY2 at 200 Hz. The default clear fixture publishes a bar, dock and
six widgets on each display (16 plates); writer cost is excluded from engine-process figures.
Each run takes the shared desktop lock, and restores the original core and window placements.

CPU/private memory below are medians of **three independent 60-second samples**, without
queries or composition waits. MiB means private bytes / 1,048,576; these are not VRAM figures.

| Engine fixture | Private MiB (three runs) | Median MiB | CPU, % of one core (three runs) | Median CPU |
|---|---|---:|---|---:|
| Rain-city, 30 fps / 0.65 | 445.56 / 448.07 / 449.09 | **448.07** | 3.175 / 2.759 / 2.134 | **2.759%** |
| Aurora, 60 fps / full scale | 420.21 / 418.10 / 420.68 | **420.21** | 2.629 / 2.212 / 2.889 | **2.629%** |

The engine memory gate is the larger median + 10 MiB: **458.1 MiB private** (448.1 + 10,
rounded to one decimal). Rain meets the <=4% one-core CPU gate on both screens together.
The plan's whole-product allowance remains engine gate + 48 MiB (506.1 MiB); no native-shell
or whole-product pass is claimed by this engine-only fixture.

A separate 20-second timestamp run measures every component. Values below are
**median / p95 / maximum milliseconds**; complete-frame time includes wallpaper + restore/
backdrop + glass, so the 2 ms gate cannot be passed by timing only the wallpaper pass.
All 600 two-screen frame pairs were complete.

| GPU work | DISPLAY1 (240 Hz) | DISPLAY2 (200 Hz) | Gate / result |
|---|---|---|---|
| Wallpaper pass | 4.341 / 4.566 / 4.723 | 3.829 / 3.958 / 4.671 | Both already exceed 2 ms |
| Restore / backdrop | 1.125 / 1.285 / 1.698 | 1.149 / 1.325 / 1.643 | Included in frame time |
| Plates, per screen | 0.204 / 0.230 / 0.248 | 0.215 / 0.240 / 0.275 | Not the all-plate gate |
| **Complete frame** | **5.623 / 5.949 / 6.542** | **5.196 / 5.466 / 6.160** | **FAIL <=2 ms per screen** |
| **All 16 plates, summed** | **0.401 / 0.460 / 0.502** | (both screens combined) | **FAIL <=0.3 ms** |

The remaining median gaps are **3.623 / 3.196 ms** per complete frame, and **0.101 ms** for
all plates. Analytic gradients share Heartfelt field evaluation; one-time float32 row/column
caching removes repeated trig/hash work; photo mipmaps are built once; static sources and
unchanged plates skip rendering; glass reads the kept frame without a scratch copy or spring.
All 18 photo samples, every rain/snow layer, 0.65 scale and 30 fps are retained. A static photo
blur changed the grain and was removed; branchy droplet early-outs were slower and removed.
The row-cache-only comparison was modest: before 4.415 / 3.877 ms wallpaper medians, after
4.341 / 3.829 ms, with scheduling noise. This is not enough to meet the iGPU budget.

A 20-second motion run publishes moving docks at **240 Hz**. Only glass-only rows are used
for lag below: 2,005 on DISPLAY1 and 1,834 on DISPLAY2; 1,200 wallpaper submissions still run
at 30 fps total across both screens. Latest geometry is latched after swapchain acquisition.

| Rain + moving plates | DISPLAY1 | DISPLAY2 |
|---|---|---|
| One display frame | 4.167 ms | 5.000 ms |
| Publish -> queue submit, median / p95 / max | 2.867 / 4.185 / 4.779 ms | 2.369 / 4.207 / 4.897 ms |
| Publish -> DWM completion bound, median / p95 / max | 7.486 / 11.984 / 13.368 ms | 5.858 / 8.312 / 10.137 ms |
| One-frame glass lag gate | **Not established** | **Not established** |

The DwmFlush bound exceeds one frame by 3.319 / 0.858 ms at the median, and 7.817 / 3.312 ms
at p95. This is a conservative completion bound for both screens, not a per-display scanout
measurement. Queue submission alone cannot establish visible lag. Precise scanout remains
unverified; the <=1-frame gate is not claimed. The composition wait changes pacing and is
excluded from the ordinary CPU/memory baselines.

Over a still photo with unchanged plates, the 60-second exact window contains **0 submissions,
0 wallpaper passes and 0 glass passes**, with 362.09 MiB private and 0.104% one-core CPU.
The frame loop has no idle render timer; window/config events can still wake the process.
The telemetry-off 60-second still baseline measures **363.98 MiB / 0.078% of one core**.
The strict 0% still-CPU gate is therefore not met in this desktop session, even though the
separate query run proves zero rendering; event handling still has a measurable CPU cost.

A separate 15-second still + 240 Hz motion run has **0 wallpaper submissions** throughout:
2,158 / 2,110 glass-only rows. Queue delivery is 1.801 / 1.344 ms median,
4.203 / 4.089 ms p95, 4.847 / 4.647 ms maximum. Its DWM completion bound is
6.132 / 5.383 ms median, 8.740 / 8.160 ms p95, and 29.241 / 29.241 ms maximum.
Its all-plate GPU time is 0.377 / 0.431 / 0.589 ms (median / p95 / max).
This verifies source reuse, not the one-frame presentation gate.

Telemetry overhead is measured separately: GPU-query rain uses 4.607% one-core CPU / 454.80 MiB;
rain motion + DwmFlush uses 12.023% / 456.97 MiB; still motion + DwmFlush uses 13.534% /
380.80 MiB. These opt-in runs are not substituted for the telemetry-off idle fixture.

**Default stays integrated** on both desktops and laptops. The earlier RTX candidate's roughly
0.62 ms rain / 0.067 ms all-plate figures used the rejected photo prefilter, so they are not
final-quality measurements and do not justify a policy change. Users can explicitly select
`high-performance` or `auto`; there is no automatic RTX promotion. Studio controls remain N7.

Raw CPU/RAM samples and GPU/lag summaries are committed under
[`tools/perf/results/native-2026-11-04`](../tools/perf/results/native-2026-11-04).
Full JSONL traces and fixed-time images remain in `C:/mypaper-targets/native-engine/`.
The five `visuals/<theme>-web.png` / `-native.png` pairs and `visuals/weather-comparison.png`
were inspected for photo/crop, preset colour, weather density and blur. WebGL/WGSL hash
rounding changes individual rain/snow locations; the port is not pixel-identical to WebGL.
`visuals/quality-comparison.png` compares the native rain shader before/after the row cache at
12 seconds: **99.99507% identical pixels** in a 1800 x 800 crop, RGB MAE 0.00001644,
p99 difference 0 and maximum 1/255. No blur taps, scale or layer counts were reduced.
Clear-glass edge/dock crops were compared with the Tahoe clear references: inward lensing,
clear centre, opposite angle-dependent glints, no clear-material shadow.

The required `measure.ps1 -Label native-engine-restored-desktop -Seconds 60` also ran
under its own lock after the isolated fixture was stopped. It samples the **restored legacy
product**, not the native engine fixture: 24 processes, 98.72% of one core (6.170% of this
machine), 7.73% GPU 3D, 455 MiB GPU memory, 1,069 MiB private / 1,587 MiB working set.
Its 19 WebView2 processes account for 852 MiB private and 96.43% one-core CPU. This separate
counter sample is excluded from N0.3 gates; its raw output is `restored-product-counters.json`.

Cleanup limitation: automatic approval review rejected removal of the stopped test WebView
profile, returning only "blocked by policy". No test WebView processes remain, but
`C:/mypaper-targets/native-engine/release/native-stage/env-0/local/mypaper/webview2` was left
on disk. No user browser profile was targeted.

Desktop cleanup stopped all fixture engines/writers and restarted the original
`D:/Coding/projects/desktop/wallpaper/target/release/mypaper-core.exe`. The existing user's
`bars.json` has `taskbar.mode: "hide"`: a final show request left the primary taskbar hidden
and the secondary visible. That user setting was not changed. Another agent subsequently
acquired the desktop lock for its own session; its processes/lock were left alone.

The native runner's success exit means the sample was accepted (including visibility), not
that GPU/lag/zero-CPU budgets passed. These open gates are reported explicitly above.

Validation: 132 scoped Rust tests pass (config 11, IPC 2, theme 14, wallpaper 87, perf 18),
with two existing wallpaper dev tests ignored (real desktop wallpaper mutation and the still
export helper). All-target Clippy is clean on the five changed crates. The GPU port-compile
test ran under the desktop lock; all 66 catalog asset byte counts and SHA-256 hashes match.
Live config verified a 12 fps cap without restart and high-performance/auto/integrated restart
requests using core's existing EXIT_RESTART contract. No bars/widgets compatibility patch,
N7 Studio UI, or overlay code was changed. GPU, lag and strict still-CPU gates remain open for further optimization.


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
(the paint probe) and set its region, or after 15 s. Paint and layout are tracked separately for
each bar on each monitor, in either arrival order. An empty region is a completed layout too:
auto-hiding docks initially draw nothing, so their stand-ins must go when those pages paint.
The empty initial shape is cached, so the next start does not show a dock that starts hidden.
A stand-in only appears where the bar will
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
