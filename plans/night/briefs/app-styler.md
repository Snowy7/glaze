# Brief: app-styler (Explorer and Settings interiors, experimental)

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md`, `docs/windows-styling.md`,
`crates/inject/src/lib.rs` (the allowlist: Explorer, Settings, Microsoft-signed only), and
`crates/tweaks/src/dwm.rs`. A research agent is writing `docs/research/explorer-and-files.md` in
parallel; read it when it lands (check `git log -- docs/research`), but don't wait for it.

## Decision already made (by the user, 2026-11-04)

Glaze is standalone: no Windhawk or other third-party tool. Restyling the **inside** of File
Explorer and Settings is our own opt-in styler: a DLL loaded only into `explorer.exe` and
`SystemSettings.exe` through the documented XAML Diagnostics API (`InitializeXamlDiagnosticsEx` +
a TAP DLL implementing `IObjectWithSite`, using `IVisualTreeService3` / `IXamlDiagnostics` to walk
the live tree and set properties), with a crash fallback. It is **experimental**: behind
`experimental.app_styler` in `config.json`, default `false`, and costs nothing when off. The file
view and nav tree (DirectUI/Win32) are a stretch goal.

## Goal

Make Explorer's and Settings' XAML parts follow the active Glaze look, and deeply customizable:

1. **Spike first (prove it, 45 min max):** attach to `SystemSettings.exe`, set one property on one
   element (e.g. the root background to transparent), see it change, detach cleanly. Then the same in
   Explorer (tabs / address bar / command bar). If XAML Diagnostics can't reach a part, write down
   exactly why.
2. **The styler:**
   - `crates/app-styler`: the TAP DLL (cdylib) and a small on-demand helper process
     `glaze-styler` that core starts only while the flag is on. The helper watches for allowlisted
     processes (`glaze_inject::evaluate`, never anything else), attaches once per process, and
     hands it the current rules.
   - **Rules are data**: target elements by type name, `x:Name`, and ancestor path; set any
     dependency property from a string value (Background, Foreground, CornerRadius, Padding, Margin,
     FontFamily, FontSize, Opacity, Visibility, BorderThickness...), plus resource overrides
     (theme brushes). Rules live in `theme.json` › `apps` with the user's `config.json` ›
     `look.apps` layered on top, field by field, like the `windows` section. Ship two or three
     presets that match our looks (calm/mono, glass with Mica showing through, compact), not
     copied from anyone.
   - **Live**: a rule change applies without restarting Explorer or Settings; new windows and
     re-created elements are styled as they appear (tree-change callbacks, no polling).
   - **Undo**: every changed value is recorded and restored on detach, theme change, flag off,
     Glaze quit and the panic hotkey.
   - **Crash guard**: if a styled process dies within 60 s of attaching twice, stop styling that
     process for the session and log it; never attach to a process the allowlist rejects; tolerate
     missing elements across Windows builds without errors.
   - Combine with the existing DWM styling (Mica/acrylic, borders, corners) so transparent XAML
     backgrounds show the backdrop.
3. **Stretch:** the DirectUI items view and nav tree (theme colours through `SetWindowTheme` /
   msstyles classes, or whatever the research shows works from inside the process).

## You own

`crates/app-styler` (new; add it to the workspace `Cargo.toml` in its own commit), additive changes
in `crates/inject`, `crates/config` (the flag and `look.apps`, defaults only, own commit),
`crates/theme` (the `apps` section: coordinate with the bars agent, who also edits `crates/theme`:
additive only, small commits), `crates/core` (start/stop the helper, one small commit), and a new
section in `docs/windows-styling.md`. Not `crates/studio`.

## Safety

- Only `explorer.exe` and `SystemSettings.exe`, only through the allowlist, only with the flag on.
- Test in Settings first. Explorer hosts the user's desktop: if it crashes it restarts, and Glaze
  recovers from Explorer restarts, but don't loop. Don't close or move the user's windows; open your
  own Explorer/Settings windows to test and close only those.
- Leave the flag **off** in the user's `config.json` at the end unless the styler survived at least
  30 minutes of live use across Explorer restarts with no crash; say which in the report.

## How

- Use `CARGO_TARGET_DIR=C:\glaze-targets\app-styler` for check/test; `C:\glaze-targets\main` for
  the deploy build. Deploy bundling: add the DLL and helper to `scripts/deploy-local.ps1` (additive).
- Look at DPI-aware captures of styled Explorer and Settings windows before calling anything done.

## Stop when

The styler works live on both apps with presets, undo and the crash guard, or after 3.5 hours.
Commit as you go.

## Report

What's styleable and what isn't (and why), the rule format, presets, captures, commits.
