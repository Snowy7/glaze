# fswatch implementation report — 6 November 2026

Completed in about 35 minutes, on shared `main`; no push. Deployed from
`C:\glaze-targets\main` at 09:49:59 after the orchestrator's all-clear. The three changed
executables' deployed hashes match the main release target (`deployed-binaries.json`).

## Callers migrated

- Tweaks: palette and game-activity file watches (therefore core, shell, index, and plugin consumers).
- Shell: `theme.json`, `bars.json`, both recursive Start Menu app-index directories, and the
  concrete game watcher field type.
- Index: the config watcher; its existing native bulk root watcher is unchanged.
- Bars: removed an unused `notify` dependency.

`cargo tree --workspace -i notify` now shows **Studio only**, which was left alone as instructed.
The existing desktop config/engine native watcher is already nonpolling and was left alone.

## Measured cost

CPU is percent of **one core**, from the shared `measure.ps1 -Seconds 30`. Wakes are raw
per-thread context-switch deltas, from `measure-wakes.ps1`, with thread names and PIDs retained.
The initial before sample ended at 09:32, ahead of the index agent's restart; it is kept
separately. The table uses a second before sample taken after that restart, on the 09:38 PIDs.
Neither before sample spans a deployment. Both after samples use the new 09:49 PIDs.

| Process | Old notify threads | New fswatch threads | Watch switches/s before | Watch switches/s after | CPU before | CPU after, two samples |
|---|---:|---:|---:|---:|---:|---:|
| shell | 5 | 6 | 46.175 | 0.592 | 0.21% | 0.10%, 0.36% |
| core | 1 | 1 | 9.597 | 0.394 | 0.00% | 0.00%, 0.00% |
| index | 2 | 2 | 18.673 | 0.282 | 1.25% | 0.52%, 1.15% |
| **Migrated watchers** | **8** | **9** | **74.445** | **1.268** | | |

That's **98.3% fewer live watcher switches**. The extra thread is the second Start Menu folder:
each directory now gets one small sleeping thread rather than a shared polling notify worker.
Config, theme, and both Start Menu watcher threads measured **0 switches/s** in the final sample.
The isolated real watcher also measured **0 switches/s for 35.444 seconds**, including its main
thread (`isolated-idle-wakes.json`). The fixture stopped and joined normally after 90 seconds.

The remaining 0.254–0.394 switches/s per local-state watcher are real directory changes:
`glaze.log` shares the palette/game directory and the engine logs a backdrop update roughly
every 10 seconds. Filename filtering suppresses callbacks and debounce timers for those writes;
ReadDirectoryChangesW still completes for changes to other files in the watched directory.
There is no periodic timer or timeout while a watch has no pending matching changes.

CPU is **not a controlled causal comparison**: the index root watcher was handling genuine
C-drive traffic, and another agent enabled/started app styling between the before and after
samples. Both after samples are retained rather than selecting the lower values. The first
before sample was shell/core/index 0.52%/0.00%/0.52%. In the final sample shell/core/index private
memory was 37/3/17 MiB; unrelated UI, engine and styling work affects whole-process measurements.

## Validation

- Six focused fswatch tests passed: safe record decoding, Unicode/rename names, malformed
  batches, file filtering/deduplication, bounded pending state/latency, real native buffer
  overflow and recovery, create/modify/atomic replace/rename/delete, recursion, and idle or
  pending drop/join. Construction arms its first read before returning.
- `cargo check` and `cargo test` passed for tweaks, bars, index, and shell, plus a core check.
- Strict `cargo clippy -p glaze-fswatch --all-targets -- -D warnings` passed, including the
  idle fixture. Broader Clippy passed with pre-existing warnings outside this change.
- Live config change switched to a copied theme and changed the Windows accent. A theme edit
  visibly made both bars/docks magenta; the restored screenshot shows the original blue glass.
- Bars rejected an injected unknown field through its real reload path and kept its valid
  surfaces. Index rejected a new malformed exclusion through its config reload path, retaining
  the current rules. A full-drive valid-rule reconciliation was deliberately avoided here;
  the index workstream had already validated that behavior.
- Palette replacement changed the core's Windows accent. Game activity replacement paused
  and resumed the shell and plugin runtime. A temporary Start Menu file triggered launcher
  reindexing, then was removed. No synthetic input or window movement was used.
- Core, shell and index PIDs survived all live probes. Config, bars, palette, and activity files
  were restored **byte for byte** (`live-results.json`). I inspected `live-theme.png` and
  `live-restored.png`. The copied theme was removed, the probe process exited, and the fswatch
  desktop lock was released. Later config changes by the styling agent were left intact.

The first live-probe attempt failed before changing state because .NET's default file sharing
conflicted with the live log writer; its cleanup also hit a sharing race. The retry used
explicit shared reads, persistent byte backups, atomic replacement, and nested cleanup that
always releases the lock. It passed every check above. The initial partial-crate workspace
interruption was fixed immediately by the registration commit.

## Commits and API

- `73eb20f` registers the crate and workspace dependency in its own commit.
- `3a3a29e` implements the native watcher and focused tests.
- `4c99b3c` migrates tweaks and the shell game watcher type.
- `c5271b0` migrates shell theme/bars/launcher watches and drops bars' dependency.
- `595ed0c` migrates index config watching and refreshes the lockfile.
- A final evidence commit contains this report, reproducible probes, raw measurements, and
  screenshots. All mutations are within the brief's paths plus the required workspace manifest
  and Cargo.lock; Studio, desktop, core, and the index bulk watcher were not edited.

API: `glaze_fswatch::Watch::directory(&Path, bool, impl FnMut(Event) + Send + 'static)` and
`Watch::files(&Path, &[&str], impl FnMut() + Send + 'static)`, each returning
`anyhow::Result<Watch>`. `Event` is `Paths(Vec<PathBuf>)` (absolute paths) or `Rescan`.
Callbacks should post to the existing event loop. Keep the watch alive; dropping it cancels,
drains and joins. File-name matching is ASCII case-insensitive. See the crate README for timing
and resource bounds.
