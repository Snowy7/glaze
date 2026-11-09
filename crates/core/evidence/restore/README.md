# Restore and crash-recovery evidence (2026-11-09)

All runs happened on the live desktop (two monitors, build 26300) under the desktop lock. They
killed only Glaze's own processes, sent no input and activated no window.

- `state.ps1` takes a read-only snapshot: Explorer's taskbar windows (visibility, extended style,
  veil alpha), `ABM_GETSTATE`, `TaskbarAl`, each monitor's work area, the cursor scheme, the
  accent, a sample of preferences, a hash of each journal, and DWM attributes on an owned probe
  window in a separate process.
- `kill-test.ps1` records one JSON line per snapshot.
- Before the tests, the user's Glaze had the native taskbar hidden (`SW_HIDE` plus the alpha-0
  veil), auto-hide on (the journaled original was `0`), and reserved 24 px for the top bar.

| Case (log) | Right after the kill | Recovered by | End state |
|---|---|---|---|
| Shell killed ([kill-shell.jsonl](kill-shell.jsonl)) | Core logged `restored the native taskbar from the bars' journal` about 1 s later and restarted the shell at once | Core, then the new shell re-applied | Same as before: taskbar hidden and veiled, same work areas and journals |
| Core killed ([kill-core.jsonl](kill-core.jsonl)) | Within 2 s the shell saw its parent exit and restored everything: taskbar shown and unveiled (`ex=0x88`), auto-hide `0`, native work areas, `taskbar.json` deleted | The shell's own exit path | After `runas /trustlevel:0x20000` restarted Glaze, the original state came back exactly |
| Core and shell killed together ([kill-both.jsonl](kill-both.jsonl)) | **Broken.** Explorer shows its taskbar windows again, but the veil stays on them (`visible=True`, `alpha=0`), so the taskbar is invisible and click-through. Auto-hide stays on and the work area is the full monitor | `glaze-core --restore`: taskbar shown and unveiled, auto-hide `0`, native work areas, accent back to the journaled original (`4292114432`), styling, preference and routing journals cleared | After the restart, every journal hash matches the pre-test state |
| `--restore` while Glaze runs ([kill-restore.jsonl](kill-restore.jsonl)) | `stopped:true`. Core's panic path undid what it had applied; `--restore` then undid folder routing | The command itself | Native taskbar and original accent, then Glaze restarted to the exact prior state |
| Simulated Explorer restart ([taskbarcreated.jsonl](taskbarcreated.jsonl)) | `TaskbarCreated` was posted only to Glaze's own windows (`taskbar-created.ps1`). The shell re-registered its app bars (one `ABM_NEW` was refused, then repaired by the existing retry) and restarted its tray host; core re-added its icon | The shell | Taskbar still hidden, work areas unchanged |

DWM attributes on the owned probe window were the same in every snapshot (corner `0`, unset
colours). Glaze only ever styles allowlisted Windows apps.

`dwm-live.ps1` ran the opt-in `foreign_window_corner_lifecycle` test against a
medium-integrity probe process. It passed all four corner modes for existing, new, app-reset,
release and drop. It also confirmed that the tool-window and full-monitor (borderless popup)
exclusions put the original back, and that crash recovery resets to the default. The elevated
case was skipped: this session runs at medium integrity, and an elevated probe would need a UAC
prompt.

## Wallpaper journal

Both restores reported `failed: wallpaper`, and exited `1`.

- The machine's `wallpaper.json` names its originals as files under
  `AppData\Local\mypaper\snapshot\`, a pre-rename path that no longer exists.
- `desktop_wallpaper::restore` keeps the journal whenever `SetWallpaper` fails.
- So any user whose original wallpaper file was deleted fails this step on every restore, and the
  uninstaller, which fails closed, would refuse to uninstall.
- See docs/restore.md, "What is still left behind".
