# Brief: app-styler round 5 (style only processes that end)

Read `plans/native/LIVE.md`, `docs/windows-styling.md` › "App styler", and
round 4's evidence (`crates/app-styler/evidence/round4/README.md`, commits 0eb5c89, c775527, 54af732).
You own `crates/app-styler`, additive `crates/config`, and the matching docs.

## What round 4 proved

XAML Diagnostics keeps every parentless object it has seen (`RuntimeObjectCache::m_parentlessObjects`),
and Explorer's island roots are never reported as removed, so a subscribed Explorer process keeps
every closed folder window's tree until **that process exits**. Ending diagnostics inside a running
Explorer hangs it. So styling is only safe in processes that end on their own.

## Decision (made by the orchestrator for the user)

1. **Settings:** `SystemSettings.exe` exits when the user closes Settings, so the leak is bounded by
   one Settings session. Style it.
2. **Explorer:** never attach to the shell Explorer (the one that owns `Shell_TrayWnd`, the desktop and
   Progman/WorkerW). Style folder windows only when they live in Explorer's **separate folder process**
   (Windows' "Launch folder windows in a separate process", `SeparateProcess` in HKCU Advanced). The
   system-tweaks work already has a journaled `separate_process` setting in `config.json` › `system`
   (`docs/system.md`). That process exits after its last folder window closes, which frees the leak.
   Verify that on this build: how long after the last window closes it exits, and that a new window
   starts a new process the helper attaches to.
3. Per-app switches: `experimental.app_styler` stays the master flag. Add
   `look.apps.explorer.enabled` / `look.apps.settings.enabled` (or the closest fit to the existing
   rule format), with Explorer styling requiring `system.separate_process = true`. If it isn't set, log
   one clear line and skip Explorer, never the shell process.
4. Also drop the per-handle part of the leak: stop requesting handles for objects outside the tree
   (created brushes, scratch elements) where you can, as round 4 suggested.

## Prove

- Settings: open/close Settings 10 times with styling on. Its memory is per session, and nothing grows in
  any long-lived process.
- Explorer: with `separate_process` on, open, navigate and close folder-window bursts. The separate
  process's memory may grow while windows are open, but the process exits after the last window
  closes, and the shell Explorer's private bytes stay flat against a flag-off control. Never attach to
  the shell Explorer (log the PID you refuse).
- Then a 30-minute soak with the glass preset across both. If clean, turn `experimental.app_styler` on
  with `system.separate_process = true` in the user's config (both journaled and reversible). That
  changes how folder windows open (a separate process), so say so in the report. If not clean, leave
  it off.

**Hold rules:** deploy only from `C:\glaze-targets\main`; desktop lock one action at a time; your own
windows only. The user may be awake now: never steal focus from their apps. Run your windows on the
secondary monitor if possible, and keep live input to the minimum.
`CARGO_TARGET_DIR=C:\glaze-targets\app-styler`. Stop when proven, or after 2.5 hours. Report as before.
