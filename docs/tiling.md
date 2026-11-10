# Scrolling tiling

Glaze can replace Snap with a row of independently sized columns. Enable **Settings > General > Experimental >
Scrolling tiling** in Studio. It is off by default. Turning it off restores the windows and the session's
original Snap preference. The implementation is experimental; the validation gaps below are release gates.

## Settings

`%APPDATA%\glaze\config.json`:

```json
{
  "experimental": { "tiling": true },
  "tiling": {
    "preset_widths": [0.3333333333333333, 0.5, 0.6666666666666666],
    "default_width": 0.5,
    "gaps": 16,
    "center_focused_column": "never",
    "animations": true
  }
}
```

The flag is omitted when false; the `tiling` section is omitted when every preference is default. Gaps are
logical pixels, 0-128. Widths are proportions, 0.05-1, with 1-16 presets. `center_focused_column` is `never`,
`always`, or `on-overflow`. The last compares the target column with its neighbour in the direction focus
came from: it centers only when the pair cannot fit together, as niri does. Settings apply live.

`keybindings` replaces the complete default list. A binding has a numeric Windows virtual `key`, optional
`ctrl`, `shift`, `alt` booleans, and a `command`, for example:

```json
{"key":37,"command":{"action":"focus","direction":"left"}}
```

The Windows key is implicit. Duplicate bindings, Win+L, Game Bar combinations, and Glaze's launcher/flyout
keys are rejected. Defaults intentionally replace some Windows shortcuts while tiling is enabled; turning
tiling off returns those shortcuts to Windows. Win+G and Win+Alt recording/capture shortcuts remain reserved.
See Microsoft's [Windows keyboard guide](https://support.microsoft.com/en-us/windows/hardware/input-devices/windows-keyboard-tips-and-tricks)
and [Game Bar keyboard guide](https://support.microsoft.com/en-us/accessibility/windows/use-a-screen-reader-to-record-your-screen-with-xbox-game-bar).

## Default keys

| Keys | Action |
|---|---|
| Win+arrows | Focus in that direction; portrait outputs rotate the strip axis |
| Win+Ctrl+arrows | Move in that direction |
| Win+Shift+arrows | Focus a monitor in that direction |
| Win+Ctrl+Shift+arrows | Move to a monitor in that direction |
| Win+PgUp / PgDn | Previous / next workspace |
| Win+Ctrl+PgUp / PgDn | Move to the previous / next workspace |
| Win+R / Win+Shift+R | Cycle column width / tile height |
| Win+F | Toggle full-width column, retaining gaps |
| Win+M | Toggle maximize to work-area edges |
| Win+Shift+F | Toggle full-monitor placement |
| Win+C | Center the column |
| Win+V / Win+Shift+V | Toggle floating / switch focus between layers |
| Win+W | Toggle a stack between vertical and tabbed presentation |
| Win+comma / Win+period | Consume into the preceding column / expel from a stack |
| Win+Tab | Open or close workspace overview; Escape also closes it |
| Ctrl+Alt+Shift+Backspace | Existing core panic recovery; always passes through the overview |

A Win tap keeps its launcher behavior. Win+arrows are swallowed while tiling owns the keys, including after
remapping. Keys pass through during fullscreen/game activity, when the shell is hung, or if menu masking
fails. Floating directional moves use niri's 50 logical pixels and retain a visible portion of the window.

## Layout and native boundary

`glaze-tiling` is a pure Rust crate with no Win32 or unsafe code. A monitor owns dynamic workspaces; each
workspace owns columns, stacked or tabbed tiles, a view offset, and floating windows. A trailing empty
workspace is created as needed. Landscape outputs scroll horizontally and portrait outputs vertically.

New windows open after the focused column or their owner. Opening never resizes other columns. A temporary
new focus remembers one previous view, anchored to its previous column so unrelated closes cannot corrupt
it. Explicit focus changes forget that bookmark. Minima win over maxima. Proportional widths and gaps use
logical coordinates; target rectangles use physical coordinates. Removed outputs migrate workspaces and
remember their stable display path; reconnecting returns them unless opening a new window reassigned them.
Floating positions remain relative to the work area through hotplug and DPI changes. Floating toggles
remember geometry and constraints; the first toggle uses niri's clamped 50 logical pixel offset, except
when columns are always centered. Fullscreening a float moves it into a column and restores the float on
exit. Maximize/fullscreen expel a stacked member; tabbed columns stay together, showing their active tab.

The shell subscribes to the existing `win-events` source. Cached USER state and the admitted-window set
filter events before process/token/DWM queries. Location hooks are restricted to managed PIDs. Display
broadcasts are coalesced by a one-shot debounce; a drag timer exists only during a native move/resize loop.
Eligible SHOW events whose native state is still settling get at most three one-shot 100 ms retries.
There is no tiling object, subscription, hook extension, worker, overlay, or timer while the flag is off.

The projection worker retains only the newest pending layout. A spring drives DWM thumbnails in a transient,
nonactivating, input-transparent overlay; resizing windows use a `PrintWindow` snapshot
uploaded once into an owned layered source window. It is paced by
`DwmFlush` and exits when motion ends. **Region updates use `SetWindowRgn` only on Glaze's own overlay.**
No other application's region, style, or cloaking state is changed. Proxy paths use workspace coordinates,
independent of native parking, and are cropped to their owning output. Both overlays stay below topmost
windows. Responsive real windows are committed in one `DeferWindowPos` batch; hung clients use the
`SetWindowPos` async fallback because the deferred API rejects `SWP_ASYNCWINDOWPOS`. Only a DPI
transition gets a second compensation pass. Extended frame bounds account for invisible borders. Post-placement bounds teach per-application minima and incompatible-client rules.

Off-view windows are parked just beyond the bounding box of all connected monitors. Parking preserves
native taskbar and Alt-Tab membership. Parked GDI thumbnails can show their last frame; this is an accepted
tradeoff, including Alt-Tab. Off-view activation reveals the window. Owned dialogs remain floating.

The overview represents every workspace. Dragging onto a workspace moves a window; dropping in a gap
creates a workspace. It has a finite zoom transition and no stationary frame timer. The top bar draws a
position track through its existing paint cycle. Dock clicks enqueue a reveal before native activation.

## Compatibility rules and recovery

The 188-application, 225-rule seed is data adapted from komorebi's MIT application list, not its window
manager code. See [attribution](../crates/shell/src/tiling/APPLICATIONS.md) and the adjacent license.
User additions are a JSON array in `%APPDATA%\glaze\tiling-rules.json`:

```json
[{
  "name": "My utility",
  "matches": [{"field":"exe","strategy":"equals","value":"utility.exe"}],
  "options": {"float":true}
}]
```

Matchers within a rule are ANDed; matching rules combine enabled options. Fields are `exe`, `class`, or
`title`. Strategies include `equals`, `does_not_equal`, `starts_with`, `ends_with`, `contains`, `regex`, and
legacy substring matching. `float`, `ignore`, `layered`, `force`, `tray`, and `late_title` options are
available. `force` overrides floating heuristics, not integrity, topmost, or root-window safety checks.
Learned behavior is saved only when changed, in `%APPDATA%\glaze\tiling-learned.json`.

Before moving a window or disabling Snap, a write-ahead journal is flushed at
`%LOCALAPPDATA%\glaze\tiling-recovery.json`. Entries bind the HWND to PID, process creation time, and a
window-lifetime cookie. Core restores after shell death and before respawn; startup, disable, exit and panic
also recover. Dead/recycled identities are retired. Recovery rehomes originals whose monitor disappeared.
Hung recovery attempts remain journaled for a later retry. Snap uses `SPI_SETWINARRANGING` without
`SPIF_UPDATEINIFILE`; the original session value is restored.

## Validation and limits

On 9 November 2026, medium-integrity scoped fixtures passed admission/owned/fixed/minimum-size/late-title,
tray, minimize, hung-client positioning, native fullscreen suspension/return, topmost overlay ordering,
workspace overview, destruction and exact disable restoration. A hard-crash/new-process recovery restored
four parked windows. Own-window overview and tiled captures were inspected and uploaded to PR #2.
The final run repeats tray hide/show three times. Its resized snapshot source and animated proxy were
inspected: both contain the frozen client image, after replacing an initially black off-screen GDI source.
Foreground stayed unchanged and every created process was stopped. Native stress and resize capture are
recorded in the [measurement artifact](research/scrolling-tiling/measurements-2026-11-09.json).

Latest translation costs are medians of three runs per count, `glaze-benchmark-final-20261109-003623`. Update includes DWM
thumbnail properties and **our overlay's** region updates, excluding snapshot setup, `DwmFlush` and the
final native commit. Intervals include pacing. The tested output was 200 Hz (5 ms); DWM's primary timing
reported 240 Hz (4.167 ms). The last column uses that primary period, not the fixture output's period.
These are shared-desktop measurements, not isolated DWM/GPU attribution.

| Windows | Update mean (ms) | Update p95 (ms) | Interval p95 (ms) | Intervals >1.5 primary periods |
|---|---:|---:|---:|---:|
| 4 | 1.398 | 2.356 | 9.713 | 74 / 164 |
| 10 | 1.606 | 2.592 | 10.290 | 69 / 169 |
| 30 | 2.871 | 3.948 | 13.050 | 75 / 136 |

The preceding source-mask cache A/B reduced 30-window mean update from 3.086 to 2.269 ms (8 November,
three runs each). The latest output-clipped build measured 2.871 ms. Different desktop load prevents
attributing that difference to clipping. Smooth pacing is **not established**: interval p95 exceeds the
tested output's 5 ms period. Gestures, wallpaper parallax and further visual effects were withheld while
that gate is open; their incremental costs have not been measured.

| 60-second adapter sample | CPU ms | One-core CPU | UI wakes | Private bytes |
|---|---:|---:|---:|---:|
| Flag on | 0 | 0% | 0 | 2,068,480 |
| Flag off | 15.625 | 0.02604% | 0 | 1,560,576 |

These single quiet samples include process CPU time, not just UI time; zero means no CPU-time quantum was
recorded. They prove no adapter idle message wake in these runs. They do **not** prove whole-shell flag-off
equivalence with main. The shell `wakes` instrumentation includes a tiling bucket, but the owner's running
main shell was not stopped or replaced to collect a whole-shell A/B.

The pure layout suite has 39 passing tests, including 128 generated sequences over 36 operation families
and saved counterexamples. Latest focused runs: config 25, desktop 45 (one ignored), shell tiling 5,
keyboard 8. Studio's enable/disable/undo/redo test verifies that the flag is written only while on.
Its focused settings-page render also passes; the General page was inspected with the
new toggle off. All seven touched crates pass `cargo check` and clippy (warnings remain). At the rename
checkpoint, bars had 122 passing tests; shell had 218 passing tests and a z-order test that passed in
isolation. Studio had 184 passing tests and one weather-gallery golden mismatch (missing plugin card),
reproduced in isolation. That fixture and its baselines were not changed by tiling.

The following follows every row of the study's section 4. **Live** means fixture-created HWNDs only. **Unit** means
model/rule coverage, with the stated native checks still open. **Open** means incomplete or unverified;
code inspection is not counted as a test.

| Study case | State | Evidence or remaining work |
|---|---|---|
| Which windows tile | Live | Native admission: normal, fixed, owned, topmost, tooltip, layered and late-caption cases |
| Dialogs / owned windows | Live, partial | Admission and unchanged size; parked-owner/z-order behavior needs a dedicated case |
| Menus, tooltips, popups | Live | Noncaption tool popup excluded; real application menus not exercised |
| Fixed-size windows | Live | Nonresizable fixture floats without resize |
| Min size bigger than column | Live | Native 1600 px minimum learned; siblings retain their dimensions |
| Self-movers | Open | Rate limit and persistent floating implemented; repeated native fights not exercised |
| Late title / class | Live | Child adds its caption and publishes NAMECHANGE, then is admitted |
| Maximize | Unit | Edge geometry, stack extraction and tabbed restore; native button/double-click untested |
| Fullscreen / games | Live, partial | Own borderless fullscreen suspends/returns without movement; real game transitions untested |
| Always-on-top / PiP | Live, partial | Initial exclusion and topmost-above-animation/overview assertions; real PiP transition untested |
| Splash screens | Unit | Shipped rule matching; real startup splashes untested |
| Tray / multi-window apps | Live | HIDE removes the tile and show readmits it |
| Electron transients | Open | Class exclusion/recently-hidden policy implemented; transient sequence untested |
| New window placement | Live / Unit | Sibling sizes asserted live; insertion/owner loops covered by layout tests; urgency not implemented |
| New window during overview | Open | Rebuild path implemented; arrival during overview untested |
| Closing focused column | Unit | Remembered view, preceding closes, layer fallback and selected-tab regressions |
| Minimize | Live | Nonactivating minimize/restore after display grace period |
| Arranged windows | Open | Session Snap lease implemented; actual flyout/drag-to-edge suppression untested |
| Monitor hotplug | Unit | Zero outputs, migration/reconnect, floating geometry and active focus; physical hotplug untested |
| Mixed DPI | Unit | Logical sizing/refit covered; native DPI changes untested |
| Invisible borders | Live | Native frame-bounds assertions and exact original outer-rectangle restoration |
| Elevated windows | Open | Integrity check and one-time log hint implemented; elevated-client rejection not exercised |
| Hung apps | Live | A two-second blocked own client does not block projection; placement reconciles after response |
| UWP / WinUI3 | Open | Frame/core-child resolution implemented; real frame cases untested |
| Virtual desktops | Unit, partial | Workspace model covered; public desktop adapter/switch notifications untested |
| Title-bar drag | Unit, partial | Edge/center drop model covered; native drag, threshold and hint interaction untested |
| Focus follows mouse | Off | Optional mode not implemented; no hover hook installed |
| Touchpad / touch | Open | Raw Input gestures withheld while animation pacing is open; incremental cost unmeasured |
| Win-key conflicts | Unit | Bindings and existing hook state-machine tests; actual key capture untested without input injection |
| Explorer restart | Open | Debounced TaskbarCreated receiver implemented; actual restart not performed |
| Crash / exit | Live, partial | Disable and renamed hard-crash recovery restore exact originals; actual core-death/panic paths not invoked |

Whole-shell flag-off CPU, wakes and RAM equivalence has **not** been measured: the owner's main Glaze build
must not be replaced or stopped. An isolated adapter measurement cannot prove that gate.

The adapters cover dialogs/fixed windows, tray/minimize, native maximize/fullscreen/topmost transitions,
late titles, Electron transients, public virtual-desktop identity, DPI/hotplug, self-mover rate limiting,
hung probes, UWP frames and recovery. Real applications, physical monitor changes, keys, title-bar dragging,
and overview pointer drops need additional live validation. Full-monitor placement retains client chrome.
Shadows, a wallpaper-backed overview, repaint detection beyond the final DWM frame, touchpad gestures and
wallpaper panning are not implemented yet.
`PrintWindow` is [synchronous](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow).
The 20 ms responsiveness probe handles an already-hung client; a client hanging after that probe can still
block capture/positioning and the cancellation join. Mid-call cancellation is a remaining reliability gap. Optional cloaking and an elevated companion are not included.

Run only PID-scoped, medium-integrity fixtures; never exercise the normal tiler on the user's windows:

```powershell
$env:CARGO_TARGET_DIR = 'C:\glaze-targets\tiling'   # the script's default
cargo build --release -p glaze-shell --example glaze-tiling-fixture
.\scripts\test-tiling.ps1 -Count 4 -Cases
.\scripts\test-tiling.ps1 -Count 30 -Benchmark
.\scripts\test-tiling.ps1 -Count 4 -Crash
.\scripts\test-tiling.ps1 -Count 4 -Idle
.\scripts\test-tiling.ps1 -Count 4 -Idle -Off
```

The script holds the Glaze desktop lock for at most 95 seconds, checks foreground before/after even on
failure, and terminates only that run's processes in `finally`. Test scope is `GLAZE_TILING_PIDS` (a nonempty
comma-separated PID list); the fixture also overrides `GLAZE_TILING_JOURNAL`, private `APPDATA`, and optional
`GLAZE_TILING_STATS`. Scoped runs never change global Snap. Captures use `PrintWindow` on fixture HWNDs only.
Use the existing shell `wakes` report and [performance methods](performance.md) for whole-shell sign-off.

The study was written with Claude Opus 5.5; the implementation uses GPT-6.1 Sol in Codex via T3 Code.
