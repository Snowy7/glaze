# Brief: app-styler round 3 (fix the review findings)

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md`, `docs/windows-styling.md` › "App styler", and
the review below (an independent Sol review of everything through c4d2749). The styler is **on** in the
user's config with the glass preset and runs inside Explorer and Settings, so these are real-desktop
bugs. Fix every finding below, verify each one, and re-soak. You own `crates/app-styler`, plus small
additive commits in `crates/core` (the helper's parent watch) and `crates/inject`.

## Findings (P1 first)

1. **[P1] Queued work holds stale XAML element handles** (tap.rs ~866, ~926). Restyle and restore jobs
   capture bare handles; a closed window or a Settings navigation before dispatch destroys and
   recycles them, so a job can style or restore another object. Fix: retain target references or
   track element generations, and resolve fresh handles on the owning UI thread at dispatch.
2. **[P1] A core crash leaves an unsupervised styler** (core main.rs ~345, run.rs ~210). Only the index
   gets `--parent`; the restarted core's helper fails the single-instance mutex while the orphan keeps
   styling, and Quit/Panic can't stop it. Fix: pass a parent handle, exit (restoring) when core
   exits, and clean up owned children on error exits.
3. **[P1] Allowlist check doesn't pin the process** (helper/mod.rs ~70, run.rs ~303). `app_of()` closes
   the verified handle before `InitializeXamlDiagnosticsEx`; PID reuse could attach to another XAML
   app. Fix: keep the verified process handle open through attach (PID identity is pinned while a
   handle is open), re-check, and arm crash monitoring before loading the TAP. Close the CLI
   attach DLL-staging gap too.
4. **[P2] Shutdown can miss properties from pending restyle jobs** (tap.rs ~866, ~914). Cancel pending
   styling on close and collect undo state inside each UI thread's cleanup job after in-flight work.
5. **[P2] Visual-tree callback BSTRs leak** in Explorer/Settings (tap.rs ~1150): `Type`, `Name`,
   `SrcInfo.FileName`, `SrcInfo.Hash` are owned by the callback (WinUI's LiveVisualTree.cpp
   154-161). Free all four on every return path, including `closing`.
6. **[P2] Dead UI-thread state and dispatch HWNDs stay cached** (tap.rs ~473, ~1117). Retire per-thread
   state when its owned window is destroyed and give dispatch a lifetime generation.
7. **[P2] Crash guard leaves surviving Explorer processes styled** (run.rs ~386). When an app is
   disabled, restore and stop its existing sessions and stop publishing rules for it.
8. **[P2] Rule-created brush/font caches never evict** (tap.rs ~751, ~773). Evict obsolete values on the
   owning UI thread after a restyle.

## Verify

Unit tests where logic is pure. Live, with your own Explorer/Settings windows only:
- open and close windows and navigate Settings quickly during rule changes (finding 1);
- kill core (only `glaze-core`) and check the helper exits and Explorer restores; restart Glaze
  normally afterwards (finding 2);
- rapid rules change then flag off (finding 4);
- Explorer private memory and handle count over 15 minutes of open/close/navigate cycles, flat
  (findings 5, 6, 8);
- the crash-guard test from round 2 plus finding 7.
Then a 30-minute soak as in round 2. Leave the flag on if it's clean, off if not, and say which.

**No deploy-local.ps1 or core restart between 06:40 and 07:05 local** (index measurement window); check
the clock first. Killing core for finding 2 counts as a core restart: do it outside that window.
`CARGO_TARGET_DIR=C:\glaze-targets\app-styler`. Stop when all eight are fixed and verified, or after
3 hours. Report: each finding → fix → evidence, soak result, final flag state, commits.
