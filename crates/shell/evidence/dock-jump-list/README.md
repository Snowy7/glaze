# Native dock menu verification — 9 November 2026

The release `dock_menu_evidence` fixture uses production menu layout/view, resolved popup
tokens and the shell's installed backdrop. Every window is no-activate. It reads the real
Explorer/Helium section structure, replaces private destination names with example labels,
and displays over its own opaque grayscale window. No input is sent to any application.

- [Explorer](explorer/dock-menu-live.png): Pinned (7), Frequent (12 shown).
- [Helium](helium/dock-menu-live.png): two custom categories (5 and 3), Recent (12 shown), Tasks (2).
- UIA/PrintWindow reports in each folder check the owning PID, menu role and no-activate flag.
  PrintWindow cannot read DirectComposition, so those raw bitmaps are not the blur proof.
  The final screenshots crop the fixture-owned rectangle after checking ownership across it.
  Sharp background text/checkers outside the menus become frost beneath their rounded bodies;
  broad luminance changes remain visible, without a square blur halo or a glossy rim.
- Six generated parser/native-link tests pass, including DestList v6 property-store payloads,
  custom categories/tasks, truncation, AppID hashing and serialized argument/workdir retention.
- All bars tests: 129 passed, 2 existing live/desktop tests ignored.
- Release workspace and bundled plugin builds passed. `deploy-local.ps1 -From
  C:\mypaper-targets\main\release` passed its freshness guard and deployed live. Core, shell and
  wallpaper hashes match the release artifacts. Open Studio was left running by the deploy script.
- Live `bars.json` and `config.json` are byte-identical to the pre-test backups.

Taskbar pins are imported only when `dock.pinned` is absent. The explicit live list contains
Aside, File Explorer, Grok Bot, Unity Hub and Helium; Discord is not in that list. It was not
added. A real identity bug was fixed: different explicit Chromium AppIDs no longer merge by
executable. Model tests cover running/stopped pins and separate browser/PWA items.

The fixture does not execute private destinations or task commands. Windows receives the
original serialized shell link when the user chooses a command, preserving its target,
arguments, working directory and shell properties.
