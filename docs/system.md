# Per-user Windows preferences and game activity

`config.json` has an optional `system` section, independent of theme styling. Every field is
optional. Absent or `null` leaves Windows alone; removing a field restores its original value.
Unknown fields/choices fail config validation. Studio controls are a follow-up; config works now.

```json
{
  "system": {
    "game_mode": true,
    "show_extensions": true,
    "show_hidden": false,
    "separate_process": true,
    "power_mode": "balanced",
    "game_activity": true
  }
}
```

These are preferences, with no preset or universal speed/FPS claim. None needs elevation under
normal user permissions; device policy or unsupported Windows APIs can reject a change. All are
unmanaged by default. The read-only game observer is enabled by default; `game_activity: false`
removes it. The example above is illustrative; it is not installed on the machine.

## Settings and evidence

Registry paths below are under `HKEY_CURRENT_USER`.

| Field | Values / implementation | Intended effect and primary source |
|---|---|---|
| `game_mode` | Boolean; `Software\Microsoft\GameBar\AutoGameModeEnabled`, DWORD 0/1 | Windows' Game Mode preference. Microsoft documents the registry mapping in its [Windows 11 settings reference](https://learn.microsoft.com/en-us/windows/apps/develop/settings/settings-windows-11#gaming-game-bar-game-mode-gaming-shortcuts). The old [Game Mode resource APIs](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/gamemode/game-mode-portal) are deprecated and are not used. Restart an already running game before comparing behavior. |
| `show_extensions` | Boolean; `Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\HideFileExt`, DWORD 0 when shown, 1 when hidden | Explorer's extension preference, corresponding to `SSF_SHOWEXTENSIONS` / `fShowExtensions`. This improves filename visibility, without a speed claim. [Shell flags](https://learn.microsoft.com/en-us/windows/win32/shell/ssf-constants), [shell state](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/ns-shlobj_core-shellstatea). Individual file classes can still override extension display; [Microsoft's explanation](https://devblogs.microsoft.com/oldnewthing/20241120-00/?p=110547). |
| `show_hidden` | Boolean; same Explorer Advanced key, `Hidden`, DWORD 1 when shown, 2 when hidden | Corresponds to `SSF_SHOWALLOBJECTS` / `fShowAllObjects`. Protected operating-system files keep their own separate preference. [Shell state contract](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/ns-shlobj_core-shellstatea). |
| `separate_process` | Boolean; same key, `SeparateProcess`, DWORD 0/1 | Folder-window process isolation (`SSF_SEPPROCESS` / `fSepProcess`), not one process per tab and not a measured speed optimization. New folder windows use the preference; existing Explorer processes are never restarted. [Documented flag](https://learn.microsoft.com/en-us/windows/win32/shell/ssf-constants), [documented behavior](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/ns-shlobj_core-shellstatea). |
| `power_mode` | `"efficiency"`, `"balanced"`, `"performance"`; Windows 11 `PowerGet/SetUserConfiguredACPowerMode` and DC counterparts | Windows' own power-mode vote for AC and battery, saving/restoring the two originals independently. No power plan is imported or replaced. [Getter](https://learn.microsoft.com/en-us/windows/win32/api/powrprof/nf-powrprof-powergetuserconfiguredacpowermode), [setter and mode GUIDs](https://learn.microsoft.com/en-us/windows/win32/api/powrprof/nf-powrprof-powersetuserconfiguredacpowermode), [DC setter](https://learn.microsoft.com/en-us/windows/win32/api/powrprof/nf-powrprof-powersetuserconfigureddcpowermode). Windows may override the effective mode; an accepted user vote is not a performance guarantee. DLL exports are looked up only on use; missing APIs produce an error and roll back the transaction. |
| `game_activity` | Boolean; default on, explicit false disables | Observes/publishes the background-pause hint below. It changes no Windows preference. |

The Explorer flag semantics are public API contracts. Their registry backing is validated by
`SHGetSetSettings(..., false)` readback on this machine; it is not presented as a permanent registry
ABI guarantee. Windows also documents these preferences in its [common settings reference](https://learn.microsoft.com/en-us/windows/apps/develop/settings/settings-common#file-explorer-classic).

## Apply and undo

`mypaper_tweaks::system::writes(&SystemConfig)` is the pure plan. `SystemTweaks::new()` first recovers
`%LOCALAPPDATA%\mypaper\undo-system.json`, then `apply(&config.system)` follows config changes.
The theme's `undo.json` and classic-menu behavior are unchanged.

The existing `Undo<Store>` engine saves originals before any write. Registry originals retain the
exact type and bytes, including absence, unusual string terminators and value types. Created keys
are removed only if empty. Power entries use a virtual `@mypaper/power-mode` namespace in the same
journal: AC/DC GUIDs are restored with the supported API, never written to a guessed registry key.
The complete plan is captured before the first write. A failed read prevents the transaction;
a failed write attempts full rollback and retains the journal if rollback fails.

Changing/removing a field restores the previous transaction before applying the remaining desired
fields. Identical config events do no work. With no managed preferences, core recovers any crash
journal once and drops the unused controller. There is no settings timer, registry watcher or worker.
Explorer receives a bounded `WM_SETTINGCHANGE` (`ShellState`) broadcast on real changes and restore.
Visibility flags read back live; process isolation applies to newly opened folder windows. Core
restores on quit and panic, and RAII covers error returns. A hard kill/abort is recovered on core's
next start, after the existing single-instance check. A second core cannot undo a live core.

The API restores preferences when mypaper exits. A power preference is not intentionally retained
after exit. A user editing an actively managed preference should first remove that field from
`system`, so mypaper relinquishes it before the manual edit.

## Settings shortcuts and deliberate omissions

The launcher's Settings vocabulary includes the following [documented Settings URIs](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-settings).
Searching the names/keywords opens the native page. Page availability varies with Windows/SKU.

| Launcher entry / keywords | URI |
|---|---|
| Graphics, GPU, HAGS, MPO, windowed game optimizations | `ms-settings:display-advancedgraphics` |
| Game Mode, gaming | `ms-settings:gaming-gamemode` |
| Captures, background capture, Game DVR, record what happened | `ms-settings:gaming-gamedvr` |
| Power & battery, power mode | `ms-settings:powersleep` |
| Dev Drive / Disks & volumes, ReFS, VHD, build cache | `ms-settings:disksandvolumes` |
| For developers, long paths | `ms-settings:developers` |
| Windows Security, Defender | `ms-settings:windowsdefender` |

Opening a page does not change anything or supply admin rights. Graphics is the relevant location
for supported GPU options; **MPO has no native Settings toggle**. For developers is contextual help,
not a promise of a long-path toggle on every Windows build. [Research sections 7–8](research/explorer-and-files.md#7-developer-workloads)
explain the tradeoffs and evidence limits.

| Not applied | Why / primary source |
|---|---|
| HAGS | Hardware/driver-dependent, system-wide configuration, potentially admin/restart. Use Graphics and benchmark that machine. [Microsoft DirectX team](https://devblogs.microsoft.com/directx/hardware-accelerated-gpu-scheduling/). |
| Long paths | `HKLM\SYSTEM\CurrentControlSet\Control\FileSystem\LongPathsEnabled` requires admin; the app also needs `longPathAware`. Compatibility, not a build-speed claim. [Microsoft contract](https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation). |
| Defender exclusions / performance configuration | Administrative configuration changes scanning coverage. Diagnose before proposing targeted changes; mypaper neither elevates nor alters protection. [Performance analyzer](https://learn.microsoft.com/en-us/defender-endpoint/tune-performance-defender-antivirus), [exclusions](https://learn.microsoft.com/en-us/defender-endpoint/microsoft-defender-antivirus-exclusions-overview). |
| Dev Drive creation/trust | Needs admin and a user-chosen new volume/VHD; not an in-place conversion of the user's files. [Dev Drive setup](https://learn.microsoft.com/en-us/windows/dev-drive/). |
| MPO disabling | An HKLM vendor troubleshooting workaround requiring admin/restart, with composition/power tradeoffs. No generic optimization is offered. [NVIDIA workaround](https://nvidia.custhelp.com/app/answers/detail/a_id/5157/), [MPO benefits](https://nvidia.custhelp.com/app/answers/detail/a_id/5159/). |
| Historical background capture | Its [WinRT settings API](https://learn.microsoft.com/en-us/uwp/api/windows.media.capture.appcapturesettings.ishistoricalcaptureenabled) is restricted to provisioned developer accounts. The old device policy is not a current per-user capture switch. Use Captures; do not confuse capture with Game Bar installation. |
| Windowed-game optimizations | Supported user feature, but the reviewed source describes the UI rather than a public external write API. Use Graphics; restart affected games. [Microsoft support](https://support.microsoft.com/en-us/windows/hardware/display-graphics/optimizations-for-windowed-games-in-windows-11). |
| Automatic folder-type discovery overrides | No supported general per-user switch or measured benefit was established in the research. No BagMRU/FolderTypes registry overrides or blanket folder-template changes. |

## Game activity API

`mypaper_tweaks::game_activity` exports:

- `classify(notification_state: Option<i32>, foreground_covers_monitor: bool, foreground_pid: u32) -> Activity`:
  pure policy, prioritizing `QUNS_RUNNING_D3D_FULL_SCREEN`, `QUNS_PRESENTATION_MODE`, `QUNS_BUSY`, then
  monitor coverage. **Busy requires actual foreground monitor coverage**; it cannot latch a
  pause after exit, minimize or focus-away. Quiet time alone does not pause anything.
- `covers_monitor(window: [i32; 4], monitor: [i32; 4]) -> bool`: physical screen rectangles
  `[left, top, right, bottom]`, including negative monitor coordinates and one-pixel tolerance.
- `Activity { pause_background: bool, reason: Reason, foreground_pid: u32 }`, with
  `Reason::{Idle, Fullscreen, Direct3d, Busy, Presentation}`.
- `GameWatcher::start() -> Result<GameWatcher>`: one session publisher, on a thread that pumps
  Win32 messages. It is bound to that thread. Drop removes every hook, hidden window, file and
  publisher lease. `system.game_activity: false` creates none of these resources; absent/null enables it.
- `path() -> Result<PathBuf>`: `%LOCALAPPDATA%\mypaper\game-activity.json`.
- `read() -> Option<(Snapshot, Owner)>`, where `Snapshot { publisher_pid, publisher_started,
  activity }` identifies the publisher by PID **and process creation time**. Missing/corrupt data,
  dead publisher or reused PID returns `None`. `Snapshot::open_owner()` also validates identity.
- `watch(changed) -> Result<notify::RecommendedWatcher>`: directory notifications for atomic
  replacement/removal, released on drop. Callback should post to the consumer's loop.
- `Owner::handle() -> HANDLE`: add this process handle to the consumer's wait set while holding
  `Owner`. A signaled handle invalidates the signal immediately after a crash. Re-read/replace the
  owner on file notifications. Missing/dead publisher means resume/no activity override.

The watcher has out-of-context foreground, foreground location, minimize and desktop-switch
WinEvent hooks, plus a hidden top-level window for presentation/display settings broadcasts. It
queries `SHQueryUserNotificationState` when those events occur. It publishes only when the sampled
activity changes, by atomic file replacement. It creates no worker thread and injects nothing.
Idle has no timer. While paused, one **1 Hz** timer on the existing hidden window rechecks the
hint/geometry in case Windows misses an event; resume/drop removes it. Consumers use event-driven
filesystem watches, with no polling timer.
[WinEvent contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook),
[notification-state limitations](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shqueryusernotificationstate).

This is a conservative background-pause **hint**, not identification of a running game. Fullscreen
video, editors and presentations can match. Windowed games may not match. Notification state has
no fullscreen start/stop notification of its own; foreground/geometry events are the trigger, so
idle-to-active internal changes without one can remain unseen until the next relevant event.
An existing pause is rechecked once a second so missed exit events cannot strand it. The foreground
geometry check excludes Explorer's desktop/taskbars and the publishing process. It uses a scoped
per-monitor-DPI-aware thread context for physical coordinates. The observer retains memory while
enabled: zero rendering/jobs and literal zero product footprint are different policies.

## Games

Core enables the event-driven observer unless `system.game_activity` is explicitly false. Absent
or null inherits on. Windows preferences remain opt-in. The original observer measurement was
0% of one core over 30 s (1.35 MiB for the entire debug probe, one thread, no timer/worker).

The shared **core → child control message** is the existing atomic `game-activity.json`
`Snapshot { publisher_pid, publisher_started, activity: Activity { pause_background, reason,
foreground_pid } }`. Start watching before the initial read, re-read on `watch` notifications,
retain `Owner`, and include `Owner::handle()` in the normal blocking wait. Missing/corrupt/dead
publisher means resume. Never add a polling timer. Index consumers pause background scans and
reconciliation, keep a single pending reconciliation, and resume after the hint clears. Explicit
file operations are not cancelled by this hint. The index agent owns that implementation.

The shell drops game-time clock/stats/custom-command deadlines and provider subscriptions on
all screens. Covered screens keep their existing visibility behavior; visible secondary bars
keep their cached content until resume. Widgets hold fetchers, stats, media/audio and motion.
The game hold is independent of tray pause, battery, lock and display-off holds.
The shared shell WinEvent source, including its 25 ms event-batching sleep, is released during
the hold. Resume recreates it and takes fresh bar/widget window snapshots before reconciliation.
The desktop menu's mouse hook and the tray-host worker also leave during games, returning
notification-area ownership to Explorer until resume. This avoids handling a game's mouse
input or third-party tray updates while the shell is quiescent.

The shell relays `mypaper_plugin_api::wire::HostMsg::GameActivity(bool)` to its helper (appended
to the wire enum, preserving existing discriminants). It precedes opens after a helper restart.
While true, the helper has **no background deadline**: timers, retries, move flushes and idle
unloads stop. Plugin code receives no background I/O/system events; connections stay open.
Queued state coalesces to the latest per system source/window and per websocket connection;
open/close and distinct request completions retain their identity. Coalesced websocket data
returns read credit. Raw pipe chunks retain order under the existing 64-event read-credit cap
(16 KiB chunks); coalescing a byte stream would corrupt its framing. Queued connection writes
wait too. WinHTTP transport keepalives remain: the documented default is 30 s, with
[a 15 s minimum and no documented disable value](https://learn.microsoft.com/en-us/windows/win32/winhttp/option-flags#winhttp_option_web_socket_keepalive_interval).
Live probing rejected changing this option on an already upgraded socket with
`ERROR_WINHTTP_INCORRECT_HANDLE_STATE`. Preserving the connection therefore does not promise
zero transport timer wakes. Incoming peer traffic can also wake its reader.
On false, retained events resume and an overdue periodic timer fires once,
then schedules from now. There is no catch-up timer burst. State streams must tolerate losing
intermediate updates during the hold; transport-level incoming traffic still wakes its reader.

This is background quiescence, not process exit. Processes retain private memory, incoming OS/
network events still run, and the wallpaper's existing covered policy retains the GPU device
(see [Hidden = free](performance.md#hidden--free-6-november-2026-round-2)). It therefore cannot
promise literal zero memory or zero driver wakes. No injection, game setting, priority, timer
resolution, active copy job or other application's process is changed.

## Verification (2026-11-06)

Checks use `CARGO_TARGET_DIR=C:\mypaper-targets\system-tweaks`.

- Config tests cover defaults, absent/null, strict names/enum choices and round trips.
- Tweaks tests cover each setting's desired writes, boolean polarity, exact original type/bytes,
  absence, changed/removed fields, idempotence, drop, crash recovery and partial-write rollback.
- `cargo check -p mypaper-core -p mypaper-shell` passed. Strict Clippy for config/tweaks passed;
  broader strict core Clippy initially encountered an unrelated bars unsafe-comment warning.
- The real-registry/API probe applied both boolean values for each of the four registry settings
  and all three power modes, then removed each setting. Each pass compared **all six** originals
  (four registry values plus AC/DC power) byte-for-byte; all passed. `reg query` output and shell
  flag readbacks are in [the live log](../crates/tweaks/evidence/system-live.txt).
- `SHGetSetSettings(..., false)` confirmed all three Explorer flags live. This verifies the
  preference, without claiming that existing Explorer windows migrate between processes.

Verified visually: native Settings showed [Game Mode Off](../crates/tweaks/evidence/game-mode-off.png)
and [On](../crates/tweaks/evidence/game-mode-on.png), plus [Best Power Efficiency](../crates/tweaks/evidence/power-efficiency.png),
[Balanced](../crates/tweaks/evidence/power-balanced.png) and [Best Performance](../crates/tweaks/evidence/power-performance.png).
Every captured case restored the same six originals afterwards. Captures used the existing
DPI-aware PrintWindow helper under the desktop lock. Computer Use was unavailable in this runtime.

The [hard-kill test](../crates/tweaks/evidence/crash-recovery.txt) killed only the two owned probe
processes. It verified the persisted [system journal](../crates/tweaks/evidence/system-crash-journal.json),
then restarted the controller and compared its recovered registry bytes/types/absence and AC/DC
power modes with the exact pre-crash baseline. The signal reader rejected a killed publisher,
and a fresh watcher acquired the abandoned lease and removed the stale publication on drop.
[Signal lifetime evidence](../crates/tweaks/evidence/game-crash-read.txt).

| Check / plan requirement | Measured result |
|---|---|
| Boolean settings and all power modes restore exactly | 11/11 live cases; all six original values checked after each |
| Optional observer idle CPU, target 0% | **0% of one core** over 30.004 seconds |
| Enabled observer footprint (whole debug probe, not a core memory delta) | **1.35 MiB private, one thread**; no worker/timer |
| Disable/drop releases the observer | Publication removed; same-thread start/drop again passed |
| Disabled preferences | No retained controller allocation, hook, timer or worker; crash recovery still runs at startup |

The [idle measurement](../crates/tweaks/evidence/game-activity-idle.json) excludes startup. The
[initial lifecycle probe](../crates/tweaks/evidence/game-activity-live.txt) received a Busy hint,
but Windows did not grant its child foreground focus. The later
[owned game lifecycle](../crates/core/evidence/game-lifecycle.json) now verifies focus-away,
minimize and exit. [Consumer assertions](../crates/core/evidence/game-lifecycle-consumers.json)
verify three shell/helper resumes with unchanged PIDs. There was no game/PresentMon frame-time benchmark.
No performance improvement is inferred from preference readback.

Config tests: 20 passed. Tweaks tests: 41 passed. Core: 16 passed, four ignored. Shell: 140 passed,
10 ignored. The release workspace build passed after unrelated in-progress files/shell edits
settled. The native release was deployed through `scripts/deploy-local.ps1`; the user's open
Studio window was preserved. User config was never changed by these probes. All preference
originals were restored; no probe process or scratch recovery journal was left active.

All implementation edits are in the assigned tweaks/config/core/launcher paths. The shared
`Value::Raw` addition was included in the folder-routing agent's `e4d8770` journal commit; the
system preference reader/writer and its example compatibility arm are in `234ddbc`. The
already-committed folder-routing module's additive export was included alongside the new exports.
No theme styling behavior or Studio implementation was changed by this slice.
The shared, already-dirty `Cargo.lock` contains the new tweaks → config dependency alongside other
agents' package changes; it is left for the orchestrator to commit together.

## Studio follow-ups

Add optional controls for the five managed preferences, with an explicit “Leave Windows alone”
choice, and a default-on activity-observer switch. Link unsupported/admin actions to their native
pages with the limits above. Do not add a “boost” preset. No `crates/studio` files were edited.
