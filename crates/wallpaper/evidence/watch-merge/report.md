# Native watcher consolidation and game-hold still

2026-11-06, shared `main`, Windows desktop with two 2560×1440 monitors.

## Changes

- `31b51d2`: shared config watches (engine, core, styler) and the recursive engine theme watch
  now use `glaze-fswatch`. Deleted `desktop::dir_watch`. Kept the config wrapper's filename
  matching, atomic replacement/removal handling, and theme recursion. Added creation-time
  notifications to the shared watcher to retain the old native filter.
- The same commit fixes watch recovery: close failed I/O, reopen with a cancellable 25 ms–1 s
  backoff, and emit `Rescan` after re-arming. Missing paths sleep on the nearest surviving
  ancestor's recursive directory watch, armed before rechecking existence. There is no timed
  existence polling. Stop cancels both normal I/O and parent-watch recovery.
- `4af2bac`: a game hold reveals the installed Windows theme still immediately. It remains
  visible across the ten-second resource release; hidden drawing is skipped. Resume presents
  a fresh frame before revealing the windows, including short holds that retain swapchains.
  Snapshot completion posts one wake, so an already-held engine notices a newly installed
  still without a readiness timer. Disabling the snapshot restores the normal held drawing.

Public watcher signatures are unchanged: `Watch::files`, `Watch::directory`, `Event::{Paths, Rescan}`.
The config wrapper still exposes `watch_config`, `watch_file`, and `ConfigWatcher`.

## Checks

| Crate | Tests passed | Ignored |
|---|---:|---:|
| fswatch | 8 | 0 |
| desktop | 43 | 1 |
| core | 16 | 4 |
| wallpaper | 108 | 11 |
| app-styler | 12 | 0 |

The tweaks suite also passed. Ignored tests are existing interactive/dev tools. The engine and
styler transcript is [engine-styler-tests.log](engine-styler-tests.log). The new delete/recreate
test failed before the fix because the callback channel disconnected while `Watch` remained alive.
Afterward it receives the recovery rescan, later writes, and stops promptly while the directory is
absent. A second test removes/recreates multiple parent levels and checks named-file filtering.

`cargo check` passed for fswatch, desktop, wallpaper, core, and app-styler. `rustfmt --check` and
the owned-path whitespace check passed. `cargo tree -i notify` reports only `glaze-studio`;
no `notify` watcher import or `dir_watch` user remains elsewhere. The root workspace declaration
and lockfile still describe Studio's dependency.

## Deployment and live reload

One `cargo build --release --workspace` completed using `C:\glaze-targets\main` (6m 17s including
waiting for another build). One `scripts/deploy-local.ps1` deployment followed. The script left the
already-open Studio running and put its new executable in place, as intended.

Atomic config replacement applied `engine.fps: 24` live, and a temporary file under
`rain-city/media` triggered recursive theme reload. Engine PID **19340** stayed the same for both.
The config's original bytes were restored and the temporary file removed. See
[reload.log](reload.log), [reload.json](reload.json), and [verify-reload.ps1](verify-reload.ps1).

## Visual proof

The fixture is the unmodified `crates/wallpaper/examples/game_primary.rs`: borderless foreground
window on primary DISPLAY1 only. Captures use per-monitor V2 DPI awareness and physical DISPLAY2
coordinates `(-2560, 9, 2560, 1440)`. The user's secondary-screen windows remain in place; the
comparison region `(460,70)..(940,350)` contains bare wallpaper above those windows, outside widgets.

Baseline: the crop was identical at hold start and nine seconds, then **123,094 / 134,400 pixels**
changed at release (largest channel change 44/255). I inspected the full captures and the
[baseline crop strip](before-comparison.png). See [pixel measurements](before-comparison.json),
[fixture output](before-fixture.log), and [engine trace](before-engine.log).

After deployment, the **entire 2560×1440 DISPLAY2 captures** at hold start and release are
byte-identical, SHA-256 `c6198f0ca591c9b332dd657d0cb400da4479e9174a940740df45c7367c2f575d`.
The bare-wallpaper crop also has **0 changed pixels**, both before and across release. I opened
and inspected all three full-resolution states and the six resume samples:

- [Hold start](after-hold-start.png), [released](after-release.png), [resumed](after-resume.png).
- [Crop strip](after-comparison.png), [pixel measurements](after-comparison.json),
  [resume samples](after-comparison-resumes.png), [timestamps](after-capture.json).
- [Fixture](after-fixture.log): primary-only fullscreen signal, normal exit code 0.
- [Engine](after-engine.log): still shown at 07:42:44Z, drawing released at 07:42:54Z
  (7.8 ms, source released too), fresh frame shown at 07:43:02Z. First presented frame was
  **91.0 ms** after resume began. The sampled transition stays on the theme, with no black or
  unrelated wallpaper visible. Resume sampling is discrete, not a frame-by-frame recording.

The supplemental short run also exercised still → fresh live frame while retaining the drawing:
[short-engine.log](short-engine.log), [captures](short-capture.json),
[resume samples](short-comparison-resumes.png). In this run foreground/game activity cleared
before the fixture's four-second exit; the log confirms no resource release. These supplemental
samples were taken at fixture exit, so the long run above supplies the timed resume sequence.

Raw full-resolution captures and capture timestamps are retained under
`C:\glaze-targets\watch-merge\evidence\`. Reproduce with [capture-game.ps1](capture-game.ps1)
and [compare-captures.py](compare-captures.py). Cargo tests/checks use `C:\glaze-targets\watch-merge`;
the release build/deployment alone uses `C:\glaze-targets\main`.

No changes were made to other agents' index, files, plugins, UI, or styling work. Outside the
implementation paths, the only dependency edit was Cargo.lock's desktop→fswatch edge; engine
notes and these evidence files document the behavior. No push was performed.
