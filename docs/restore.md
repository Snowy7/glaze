# Restoring Windows

Glaze changes Windows only in ways it can undo. Before each change it writes the original to a
journal in `%LOCALAPPDATA%\glaze\`. Undo replays that journal, so the user's own values come
back exactly, including settings that were not Windows' defaults. This page lists every change,
how it comes back in each case, and the headless command that undoes everything.

## `glaze-core --restore`

```
glaze-core.exe --restore
```

The installer, the uninstaller and "Restore Windows defaults" call this. It shows no UI.

1. **It stops a running Glaze.** It posts core the panic hotkey (Ctrl+Alt+Shift+Backspace). Core
   then stops its children and undoes what it applied, including putting the user's own Windows
   wallpaper back. A core that has not exited after 15 s is ended, and the journals cover what it
   left behind.
2. **It holds core's single-instance mutex** (`Local\glaze-core`) until it finishes. A core started
   during the restore, for example by autostart at sign-in, refuses to run.
3. **It replays every journal**, in this order: it stops a leftover shell, then restores the
   taskbar, styling, preferences, classic menu, folder routing, wallpaper and desktop. The steps
   are independent, so one failing does not skip the rest. Running it again does nothing.
4. **It prints one JSON line and exits with a status code.** Example line:
   `{"stopped":true,"restored":["taskbar","styling"],"failed":[]}`. The exit codes:
   - `0` means everything was restored, or there was nothing to undo.
   - `1` means at least one step failed. The `failed` array names each step, and the Glaze log
     has the details.
   - `2` means a running Glaze could not be stopped. Nothing was changed.

Run it as the signed-in user, because the journals live in that user's profile. It does not
change autostart. To remove autostart, run `glaze-core --autostart off`. That removes both the
`Glaze (<user>)` logon task and the HKCU `Run` value named `Glaze`. The uninstall order is:
`--autostart off`, then `--restore` (stop if it returns nonzero), then stop any remaining
`glaze-*.exe` and delete the files.

## What Glaze changes and how each comes back

The columns are the five ways a run can end:

- **Quit**: a clean quit from the tray. The panic hotkey does the same and also puts the Windows
  wallpaper back.
- **Off**: the feature is turned off in Studio or `config.json`.
- **Crash/kill**: a Glaze process crashes or is killed.
- **Uninstall/`--restore`**: the uninstaller or `glaze-core --restore` runs.
- **Reboot without Glaze**: the next sign-in after a restart, power loss or crash in which Glaze
  does not start.

Some cells share a ★ marker:

- **★ next start**: core's or the shell's next start, or `--restore`, replays the journal.
- **★ Explorer**: Explorer recreates the windows at sign-in, which clears the change.

| Change (journal) | Quit | Off | Crash/kill | Uninstall/`--restore` | Reboot without Glaze |
|---|---|---|---|---|---|
| Native taskbar hidden: `SW_HIDE` and an alpha-0 click-through layered style on Explorer's taskbar windows (`taskbar.json`) | The shell undoes it on exit | Restored live | **Shell killed**: core unveils and shows the taskbar at once, replays the rest of the journal, then restarts the shell. **Core killed**: the shell sees its parent exit and restores. **Both killed**: the taskbar stays hidden until the next start | Restored | Shown again (★ Explorer). The journal entry is stale and harmless |
| Taskbar auto-hide via `ABM_SETSTATE`, which Explorer persists (`taskbar.json`) | Restored | Restored | As above | Restored | **Still auto-hidden** until ★ next start |
| Taskbar alignment, HKCU `Explorer\Advanced\TaskbarAl` (`taskbar.json`) | Restored | Restored | As above | Restored | **Persists** until ★ next start |
| App-bar work-area reservations for the top bar and dock (`ABM_NEW`) | `ABM_REMOVE` | Removed | Explorer drops the reservation when the window dies with its process | Nothing to undo | Gone |
| Accent colour, dark mode, `ColorPrevalence` (HKCU DWM, `Explorer\Accent`, `Themes\Personalize`) (`undo.json`) | Restored | Restored | Persists until ★ next start | Restored | **Persists** until ★ next start |
| Cursor scheme, HKCU `Control Panel\Cursors` plus `SPI_SETCURSORS` (`undo.json`) | Restored | Restored | Persists until ★ next start | Restored | **Persists** until ★ next start |
| DWM attributes on allowlisted Windows apps' frames: border, caption, text, corners, backdrop, dark title bar (kept in memory; `undo.json` marks that styling was on) | Exact originals | Exact originals | Stays on those windows until they close. ★ next start resets them to defaults | Reset to defaults | Gone (window state) |
| Windows preferences: Game Mode, extensions, hidden files, separate process, power mode (`undo-system.json`) | Restored | Restored | Persists until ★ next start | Restored | **Persists** until ★ next start |
| Classic context menu, an opt-in HKCU CLSID key (`undo-shell.json`) | Kept on purpose | Restored | Kept | Restored | Kept |
| Folder and Win+E routing to Glaze Files, experimental (`undo-folders.json`) | Kept on purpose | Restored, unless another app changed it since | Kept | Restored | Kept |
| Windows wallpaper set to the theme's still (`wallpaper.json`) | Kept on purpose (what Windows shows until Glaze starts) | Restored | Kept | Restored | The still stays |
| Desktop host: Progman's WorkerW, Glaze's windows behind the icons | Our windows close; the old-style WorkerW is repainted | Not applicable | The windows die with their process | Repainted | Gone |
| Tray icon takeover: a hidden `Shell_TrayWnd` that forwards to Explorer | Hands back with a `TaskbarCreated` broadcast | Hands back | The window dies, so apps reach Explorer, which already holds their icons | Not applicable | Gone |
| App styler: XAML values inside Explorer and Settings, experimental | Its helper exits and the DLL puts them back | The helper stops | The DLL waits on the helper's process handle and restores | The helper stops with core | Gone when the app restarts |
| Autostart: logon task `Glaze (<user>)`, or the `Run` value `Glaze` | Kept | `--autostart off` | Kept | `--autostart off` (the installer runs it) | Glaze starts |

Desktop icon visibility, icon size and sort order are not in the table. Glaze changes them only
when the user picks them in the tray or the desktop menu, the same way Explorer's own View menu
does, so they persist by design.

The same applies to quick-settings toggles: night light, do not disturb, airplane mode, radios,
brightness and volume.

### What is still left behind

**Core and shell killed together** (for example, Task Manager's "End process tree"). Nothing
survives to restore the taskbar, so it stays hidden until the next start, `--restore`, or an
Explorer restart. Every journal is intact. The panic hotkey needs core, so it doesn't help here.
The Start menu (Win key) still works.

**Power loss with nothing restored.** Changes held in the registry persist: auto-hide,
alignment, accent, cursors and preferences. Autostart then starts Glaze, and its startup
replays the journals before applying anything.

## Explorer restarts

Glaze never restarts Explorer. When Explorer restarts on its own:

- **The taskbar.** The shell's taskbar watch sees the new `Shell_TrayWnd` appear and veils and
  hides it within about 50 ms. On `TaskbarCreated` it re-applies the whole mode from the journal.
  The originals are kept, so a later restore still returns the user's own settings.
- **App bars and the tray.** On `TaskbarCreated` the shell re-registers its app-bar reservations
  and moves the tray host back.
- **Core's tray icon.** Core re-adds it.
- **The wallpaper.** The engine exits with `EXIT_RESTART` and core reattaches it to the new
  desktop after 750 ms.
- **Glaze's own broadcast.** The shell ignores the `TaskbarCreated` its tray host sends.

## Window styling safety

DWM attributes are written only to windows that pass all of these checks:

- **Allowlisted process.** The process must be a Microsoft-signed Windows app on the
  `glaze_inject` allowlist: Explorer, Settings, Notepad, Terminal and similar. Games and
  third-party apps never pass.
- **Not elevated.** Elevated processes are skipped. The verdict is cached per process (PID and
  start time), so an elevated window costs one check and produces no log or retry.
- **Not anti-cheat.** Anti-cheat services and clients (Easy Anti-Cheat, BattlEye, Vanguard,
  FACEIT) and protected game builds (`*_EAC.exe`, `*_BE.exe`) are denied. This is defence in
  depth, since none of them is on the allowlist anyway.
- **Not full screen.** A window that covers its whole monitor is left alone, unless it is a
  maximized window with a caption. This catches exclusive and borderless full-screen games,
  including maximized captionless ones. A window that goes full screen while styled has its
  originals put back.

## Verification

See [the evidence log](../crates/core/evidence/restore/README.md).
