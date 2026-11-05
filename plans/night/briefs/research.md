# Brief: research (primary sources, no code)

Read `plans/night/2026-11-06.md`, `docs/windows-styling.md`, `crates/inject/src/lib.rs` and the
memory-style notes in `docs/research/` if any. Write **one** file: `docs/research/explorer-and-files.md`,
every claim linked to its primary source (Microsoft Learn, Windows SDK headers, source code on
GitHub, official READMEs). Commit it with `git commit -m "docs(research): ..." -- docs/research/explorer-and-files.md`.
You change no code. Cap: 75 minutes.

## Questions

1. **Explorer interior styling.** Which parts of the Windows 11 (24H2/25H2, build 26xxx) File
   Explorer window are XAML (WinUI 3 / XAML islands: tabs, address bar, command bar, details pane,
   Home/Gallery) and which are DirectUI/Win32 (items view, nav tree, status bar)? What can be
   restyled, and how, for each part:
   - XAML Diagnostics (`InitializeXamlDiagnosticsEx`, the TAP DLL, `IVisualTreeService`,
     `IXamlDiagnostics`); how Windhawk's "Windows 11 File Explorer Styler" mod
     (ramensoftware/windhawk-mods) does it, what it sets, how it targets elements, its failure modes;
   - DirectUI and the items view: msstyles / `uxtheme` theme classes (`ItemsView`, `ListView`,
     `Explorer::TreeView`), `SetWindowTheme`, the ExplorerBlurMica / StartAllBack approaches;
     which need injection and which don't;
   - cross-process DWM attributes on Explorer windows (MicaForEveryone): what works without
     injection.
2. **Settings app styling** (SystemSettings.exe): which framework it uses on 24H2+, and whether XAML
   Diagnostics reaches its tree.
3. **Our own file manager** as an alternative: how apps like Files (files-community/Files) become
   the default folder handler (registry keys, the "open folders in" setting, Win+E), what breaks,
   how they undo it.
4. **Fast file search:** how Everything (voidtools) indexes: NTFS MFT enumeration
   (`FSCTL_ENUM_USN_DATA`), the USN journal (`FSCTL_READ_USN_JOURNAL`), which calls need admin, how its
   service gives non-admin users the index; memory per million files. Non-admin alternatives:
   `FindFirstFileExW` with `FIND_FIRST_EX_LARGE_FETCH` / `FindExInfoBasic`, `NtQueryDirectoryFile`,
   `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)`, `ReadDirectoryChangesExW`. Throughput
   numbers from primary sources where they exist. The Windows Search indexer: what it costs and
   what can be tuned without admin.
5. **Folder sizes:** how WizTree / WinDirStat / TreeSize get sizes fast (MFT, allocated vs logical
   size, hard links, cloud placeholders).
6. **File operations:** fastest copy/move paths (`CopyFile2` with `COPY_FILE_NO_BUFFERING`, robocopy
   `/MT`, `IORING` on Windows 11, block cloning on ReFS/Dev Drive), and how Explorer's copy engine
   is replaced (TeraCopy's approach, `IFileOperation` hooks, copy handlers).
7. **Dev work:** Dev Drive (ReFS, performance mode, the Defender async scan), Defender exclusions,
   long paths, `git` fsmonitor, what actually speeds builds, which need admin.
8. **Games:** what measurably helps (Game Mode, HAGS, MPO, windowed-game optimizations, power
   plans, timer resolution, Game DVR), what's placebo, which need admin, and what a desktop
   customizer must do to cost exactly nothing while a game runs.

End with a table: each feature mypaper could offer here, how, needs admin or not, needs injection
or not, where it belongs (native service, plugin, experimental), and a rough size.
