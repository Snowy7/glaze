# App styler round 5 evidence (2026-11-06): style only processes that exit

Round 4 proved that XAML Diagnostics keeps every tree an attached process has shown until that
process exits, and that ending it inside a live Explorer hangs Explorer. So the styler now
attaches only to processes that exit on their own:

- **Settings**: `SystemSettings.exe` exits when its window closes.
- **Explorer's separate folder process** (`system.separate_process = true`, Windows' "Launch folder
  windows in a separate process"): it exits after its last folder window closes.
- **Never the shell's Explorer**: the owner of the shell window (`GetShellWindow`), `Progman` or a
  taskbar. The helper logs the pid it refuses: `explorer (76024) is the shell's Explorer: never
  styled`.

Per-app switches: `look.apps.explorer.enabled` / `look.apps.settings.enabled` (absent: on).

All runs on the live desktop, the test windows on the secondary screen, opened without input
(Shell COM `ShellExecute`, `Navigate2`, `Quit`, `WM_CLOSE`) and only while the user had been idle
45 s. Private bytes from `Get-Process`.

## Settings: memory is per session, but one page fails fast

Each session: Settings opened on a page, a second page 5 s later, closed (`WM_CLOSE`) 5 s after
that.

| Run | Sessions | Settings private MB, two pages open | Exit after closing | Shell Explorer private MB | Helper |
|---|---:|---:|---|---|---|
| control, nothing attached | 3 | 83.8–101.5 | 0.5 s, 3 of 3 | 1,124.2 → 1,124.2 | none |
| glass (`--preview glass`) | 10 | 41.5–103.9 | 0.5–1.5 s, 10 of 10 | 1,123.6 → 1,134.0 (not attached) | 2.5 → 3.1 MB |

Every styled session ended with its process, so whatever XAML Diagnostics holds goes with it;
nothing long-lived grew (the helper stayed at 3 MB; the shell Explorer was never attached, and
it moves by about 10 MB on its own with nothing attached, see the controls). A windowless
`SystemSettings.exe` left from round 4 (73468, styled with that round's recycling build, alive
without a window since 05:36) was ended before the runs; no Settings process outlived its window
in this round.

**But sessions 7 and 8 crashed** (`0xc000027b`, a stowed exception in `Windows.UI.Xaml.dll`) about
1 s after opening Personalization › Background, and the crash guard stopped styling Settings
(`not styling settings again this session`), so sessions 9–10 ran unstyled. `bisect.ps1` and
`diag.ps1` narrowed it down (`bisect.log`, `diag-background.txt`):

- nothing attached: the page opens fine (2 of 2, `settings-background-control.log`);
- attached with **no rules** (`rules-none.json`, the TAP only follows the tree): it fails fast
  (3 of 3);
- the stowed error (cdb, `!analyze`): `E_INVALIDARG` "The parameter is incorrect" from
  `CDependencyObject::SetEffectiveValue` (`propertysystem.cpp` line 1625) while a control
  template applies a `TemplateBinding` to a `Border` during layout
  (`CControlTemplate::LoadContent` › `CFrameworkElement::ApplyTemplate` › `MeasureCore`),
  `Windows.UI.Xaml.dll` 10.0.26100.9711. No TAP code on the stack: XAML Diagnostics being
  attached changes what that property set does.

Pages attached with no rules, 8 s after opening: **Background and Lock screen fail fast**;
Themes, Start, Fonts and Colours stay up (and Display, About, Network, Bluetooth, Volume mixer,
Sound, Taskbar and Privacy did in the styled run). Both failing pages show picture pickers. So
Settings is now **off unless `look.apps.settings.enabled` is `true`**.

## Explorer

**Where folder windows live** (`folder-process.log`, nothing attached): with `SeparateProcess`
on, a new folder window opens in an `explorer.exe /factory,{75dff2b7-...} -Embedding` process,
not the shell's (76024). It exits 56.5 / 56.7 s after its last window closes, and the next
window starts a new process. Every `explorer.exe <folder>` launch got its own process: a burst's
two windows, opened a second apart, were two processes.

**Bursts** (`explorer-bursts.ps1`): two windows opened (on the secondary screen), each navigated
6 times across folders, Home and Gallery, then closed; wait for the folder processes to exit.

| Run | Folder processes | Private MB, windows open | Exit after the last window closed | Shell Explorer private MB |
|---|---:|---:|---|---|
| control, nothing attached (`explorer-control.log`) | 6 | 119–166 (first window's) | 45.3–50.1 s (each burst's first; the rest gone by the next burst) | 1,123.5 → 1,133.4 |
| glass, first build (`explorer-styled.log`) | 6 | 139–287 | **never**: 3 of 3 checked lived on without a window (30652 for 7 min); one **hung** (below) | 1,133.1 → 1,122.3 |
| glass, let go on last window (`explorer-styled2.log`) | 6 | 173–349 | **49.5–52.1 s, 6 of 6** | 1,121.8 → 1,132.9 |

- **Styled folder processes didn't exit.** While a session follows the tree, XAML Diagnostics
  keeps every closed window's tree, and those keep Explorer's process alive. When the helper
  exited and the five windowless styled processes detached, all five exited within 39 s. So the
  helper now lets go of a folder process when its last `CabinetWClass` closes (a destroy hook
  per attached process, a per-process stop marker `explorer.<pid>.off`), and never attaches to
  that pid again. With that, styled processes exit like unstyled ones.
- **One styled folder process hung** (85452, `hang-85452.txt`): right after it was styled, its
  UI thread spun at a full core for 4 minutes in WinUI's
  `ItemsView::GetElementIndex` › `SharedHelpers::GetAncestorOfType<ItemContainer>` (a parent
  walk that never ends), raised from a `GettingFocus` during
  `FileExplorerFolderViewController::SetInnerFocus` › `FocusManager::TryFocusAsync`. No TAP code
  on the stack. Seen 1 of 6 styled processes in that run, 0 of 6 unstyled. It was a test
  window's process; it was ended after the stack was taken.
- **It happened again** in the soak's first burst (77068, a window on Gallery, `hang-77068.txt`,
  caught by `hang-watch.ps1`): the same `ItemsView::OnItemsViewElementGettingFocus` ›
  `ItemsView::GetElementIndex` loop (this sample inside `ItemsRepeater::GetElementIndexImpl`), on
  the UI thread the TAP had styled 18 s earlier. The soak was stopped there, the test process
  ended. Two hangs in 15 styled folder processes from the bursts, none in 13 unstyled ones.
- Trying to make it happen on purpose (`gallery-trials.ps1`, `nav-trials.ps1`, `gallery.log`,
  `nav.log`): windows opened straight on Gallery didn't hang (glass 0 of 5, attached with no
  rules 0 of 5, nothing attached 0 of 5). Navigation trials (open on a folder, then Gallery,
  Home and folders, checking the window answers after each step) were stopped after one window
  (it answered through all 8 steps): the user was at the desktop, and the test helper, which has
  no process filter, had also attached (with no rules) to a folder window that was probably
  theirs (63656, opened 11:44:43; attached at 11:44:45 and 11:47:30, detached both times, still
  answering). So whether the hang needs a rule or only XAML Diagnostics is still open.
- The shell Explorer was never attached: every helper start logged
  `explorer (76024) is the shell's Explorer: never styled`. Its private bytes move by about
  ±10 MB with nothing attached (it is at 1.1 GB from earlier rounds' tests; restarting Explorer
  reclaims that).

## Outcome

- The design holds for memory: Settings and the folder processes exit, the shell Explorer is
  never attached, nothing long-lived grows, the helper stays at 3 MB.
- It is **not clean**: Settings fails fast on two Personalization pages whenever it is attached
  (now off unless asked for), and styled folder windows hung twice. So the 30-minute soak was
  stopped at its first burst, `experimental.app_styler` stays **off**, and
  `system.separate_process` (set for these tests through the tweaks journal) was removed again:
  `config.json` is byte-for-byte what it was before the round and `SeparateProcess` is back to 0.
- The per-handle part of the leak (round 4's item 2) was not changed: the handles the TAP asks
  for outside the tree are one scratch `Border` and a few created brushes per UI thread, kilobytes
  against the trees' hundreds of MB, and they now go with the process. Every earlier change to
  handle use risked the stale-handle crash of round 1.
- Next: find out whether the ItemsView hang needs a rule (a `--only`-style process filter on the
  preview helper first, so tests never reach the user's windows), and file the Settings
  fail-fast and the `DesktopWindowXamlSource` root leak with WinUI.

## Files

| File | What |
|---|---|
| `lib.ps1` | Shared helpers: lock, idle gate, open a folder window without taking the focus, samples. |
| `folder-process.ps1`, `folder-process.log` | Where a new folder window lives with `SeparateProcess` on, and when its process exits. |
| `settings.ps1`, `settings-control.log`, `settings-styled.log`, `settings-background-control.log` | Settings sessions: open a page, a second page, close; does the process exit. |
| `bisect.ps1`, `bisect.log`, `rules-*.json` | Settings attached with one rules file, then a Personalization page: does it survive. |
| `diag.ps1`, `diag-background.txt` | cdb on Settings through the fail-fast: the stowed error and `!analyze`. |
| `explorer-bursts.ps1`, `explorer-control.log`, `explorer-styled.log`, `explorer-styled2.log` | Two folder windows opened, navigated 6 times each, closed; the folder processes' size and exit, the shell Explorer after. |
| `hang-watch.ps1`, `hang-watch.log`, `hang-85452.txt`, `hang-77068.txt` | Hung folder processes and their UI thread's stack (cdb `-pv`). |
| `gallery-trials.ps1`, `nav-trials.ps1`, `gallery-run.ps1`, `gallery.log`, `nav-mislabeled.log` | Trying to make the hang happen on purpose. |
| `run-explorer.ps1`, `soak.ps1`, `soak.log`, `shot-folder.ps1`, `explorer-glass.png` | The runner, the soak (stopped at its first burst), a styled folder window in a folder process. |
| `helper.log` | The helper's log lines from this round. |
