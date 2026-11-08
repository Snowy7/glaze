# Explorer, Settings, and fast file workflows

Research snapshot: **6 November 2026**, for Windows 11 24H2/25H2 and build 26xxx.
This is source research, not a live visual-tree inspection or a benchmark of this machine.
Private selectors, exports, and hooks require verification on the precise Windows build before shipping.
Numbers below are attributed vendor measurements or estimates, never measurements of glaze.

The [night plan](../../plans/night/2026-11-06.md) supersedes the older research notes' blanket
no-injection assumption: code inside another process belongs behind an off-by-default experimental
flag. The [current injection policy](../../crates/inject/src/lib.rs) permits only allowlisted,
Microsoft-signed Windows components; it forbids games, third-party applications, and following children.
The existing [Windows styling](../windows-styling.md) covers DWM attributes and per-user settings,
not arbitrary Explorer interior styling.

## Decisions this research supports

- **Keep supported DWM styling and injected interior styling separate.** A backdrop request cannot
  erase an application's opaque controls. Explorer has both WinUI and legacy surfaces, so a single
  XAML theme cannot restyle its whole interior. [DWM backdrops][dwm-backdrop], [Explorer guide][explorer-guide].
- **Build one optional native file service with an unprivileged crawler first.** Add a narrowly scoped
  elevated NTFS broker only if whole-volume indexing is worth the installation and filename-privacy
  implications. Enumeration APIs and journal APIs solve different problems. [Directory information][dir-info],
  [volume access][create-file], [Everything service][everything-service].
- **Reuse Windows copying before writing a copy engine.** Shell semantics, ordinary file copying,
  and accelerated block cloning already have public entry points. Capturing Explorer's operations
  is a separate problem. [IFileOperation][file-operation], [CopyFile2][copyfile2], [block cloning][block-clone].
- **Offer measured developer improvements and game quiescence, not an unqualified boost preset.**
  Microsoft's build results vary by workload; HAGS has no universal promised uplift. Removing our
  own work is under our control. [Build study][cow-builds], [HAGS][hags], [process exit][process-exit].

These are recommendations, not changes implemented by this document. Native services, app-specific
plugins, and experimental features follow the boundaries in the [night plan](../../plans/night/2026-11-06.md).

## 1. Explorer interior styling

### Which renderer owns each surface

The author's guide marks this boundary; its selectors are private implementation details.
[Explorer guide][explorer-guide].

| Surface | Framework / useful target | Practical styling route |
|---|---|---|
| Tabs | WinUI XAML; `TabView`, `TabViewItem` | Dependency properties, resources, templates' exposed elements. [Guide][explorer-guide] |
| Address/navigation bar and search | WinUI XAML; `FileExplorerExtensions.NavigationBarControl` | Background, size, margins, visibility, text/control properties. [Guide][explorer-guide] |
| Command bar | WinUI XAML; `CommandBar#FileExplorerCommandBar` | Buttons, separators, spacing, brushes; hiding is possible. [Guide][explorer-guide] |
| Right details pane | WinUI XAML; `Grid#DetailsViewControlRootGrid` | Brushes and child properties. This is separate from the file list's Details layout. [Guide][explorer-guide] |
| Home / Gallery | WinUI XAML; `Grid#HomeViewRootGrid`, `FileExplorerExtensions.GalleryViewControl` | XAML elements, including root backgrounds. [Guide][explorer-guide] |
| Ordinary folder items, including Details layout | Legacy shell/DirectUI items view | Theme drawing or private painting/layout hooks, not XAML selectors. [DirectUI hooks][blur-hooks] |
| Left navigation tree | Win32 tree control; `SysTreeView32` | Tree-control messages, theme classes, custom drawing inside Explorer. [Tree mod source][tree-mod] |
| Status bar and legacy surfaces | Non-XAML theme/GDI drawing | Theme resources or paint hooks; XAML-only color/font rules do not apply. [Guide][explorer-guide], [hook declarations][blur-hooks] |

For modern Explorer, the Windhawk implementation obtains `InitializeXamlDiagnosticsEx` from
**`Microsoft.Internal.FrameworkUdk.dll`**, then tries `WinUIVisualDiagConnectionN` endpoints.
Do not hardcode `Microsoft.UI.Xaml.dll` merely because the UI is described as WinUI 3.
[Explorer implementation, `InjectWindhawkTAP`][explorer-source]. ExplorerBlurMica's own README distinguishes
23H2 WinUI 3 from its older 22H2 XAML-island implementation. [README][blur-readme].

### XAML Diagnostics is injection

`InitializeXamlDiagnosticsEx` takes a target PID, diagnostics endpoint, TAP DLL path, and TAP COM
CLSID. Microsoft's documentation explicitly describes that DLL as injected into the target; the
COM object implements `IObjectWithSite` and receives an `IXamlDiagnostics` site. This is not an
out-of-process UI Automation styling API. [InitializeXamlDiagnosticsEx][xaml-init].

`IXamlDiagnostics` resolves instance handles to `IInspectable` objects and exposes the UI dispatcher.
`IVisualTreeService` supports tree-change subscriptions and property/tree operations. A TAP can
therefore observe newly created controls and change their properties on the appropriate UI thread.
[IXamlDiagnostics][xaml-diagnostics], [IVisualTreeService][visual-tree].

Windhawk matches class, name, parent chains, indices, predicates, and states, overriding properties
and resources; `:=` accepts XAML values. Whole-window translucency also hooks GDI/uxtheme and
DirectUI border painting. Only one diagnostics consumer can attach; the mod can alert/block/allow
competitors. Conditions are normally evaluated once.
[Mod implementation and embedded documentation][explorer-source].

**Proposed failure tests:** missing runtime/export/endpoint, competing consumer, selector drift,
invalid property types, UI-thread/reentrancy behavior, and repeated open/close/disable cycles.
Use small per-build rules, retain original values, remove subscriptions on disable, and fall back
to native UI if a probe fails. These are recommendations inferred from the diagnostics contracts
above, not a claim that a debugging interface guarantees safe production theming.

### DirectUI, msstyles, and uxtheme

`ItemsView` here means a **legacy theme class**, not the modern WinUI `ItemsView` control.
The current styler probes `DarkMode::ItemsView`, `DarkMode::ExplorerNavPane`,
`DarkMode::ExplorerStatusBar`, and `DarkMode::ProperTree` background parts. These private names and
part IDs illustrate why the host/build must be probed. [Explorer source][explorer-source]. The SDK
defines standard `LISTVIEW` and `TREEVIEW` classes. The spelling `Explorer::TreeView` expresses a
theme-subclass binding; it does not establish the current nav tree's actual theme class.
[SDK `vsstyle.h`][sdk-vsstyle], [theme subclasses][theme-subclasses].

`SetWindowTheme(hwnd, L"Explorer", nullptr)` associates a control with an existing theme subclass;
Microsoft uses a list view as its example. It does not install a new msstyles file, supply arbitrary
colors, or reach XAML. Passing null associations restores the normal binding. Microsoft discourages
depending on private theme subclasses because they can change. The documented application/control
API is not a promised external Explorer skinning API; for our foreign controls, plan to apply it
inside the allowlisted host and verify the result. [SetWindowTheme][set-window-theme],
[theme subclasses][theme-subclasses].

Supported system theme selection changes theme-aware controls without a per-Explorer DLL. Arbitrary
unsigned msstyles introduce a different mechanism: SecureUxTheme explicitly removes style-signature
verification with an in-memory patch, and says per-program themes are impractical. Do not present
that as ordinary per-window DWM styling or adopt its system-wide mechanism under our small allowlist.
[SecureUxTheme README][secure-uxtheme].

ExplorerBlurMica is an **in-process COM extension**, registered with `regsvr32`; its README requests
administrator installation/uninstallation and provides an Escape-held recovery path. Its module
declares a `Drive\shellex\FolderExtensions` registration; published headers describe DirectUI
background/paint detours, text/theme drawing hooks, DWM interception, and a XAML tree watcher.
Full implementation sources stopped being published from 2.0, so headers establish the approach
without proving every current binary detail. [README][blur-readme], [module][blur-module],
[hooks][blur-hooks], [release notes][blur-releases].

StartAllBack's official page promises translucent ribbon/command bars, a bottom details pane, and
more dark Win32 dialogs. Its published page does **not** establish its loading mechanism or provide
source for the rendering approach. Treat it as a closed-source product reference; whether a
particular version injects cannot be certified from that page. It is not evidence for a supported,
external API that Glaze can copy. [StartAllBack][startallback].

### What DWM can change without injection

MicaForEveryone supplies a concrete external-HWND precedent: its `DesktopWindowManager` wraps
`DwmSetWindowAttribute` and frame extension, and its README distinguishes requesting an effect from
rendering it. These calls do not require our code in Explorer. [MFE source][mfe-dwm], [README][mfe-readme].

Documented attributes include border/caption/text colors, corner preference, and dark frame
behavior from build 22000, and system backdrop type from 22621. `MAINWINDOW`, `TRANSIENTWINDOW`,
and `TABBEDWINDOW` map to Mica, Desktop Acrylic, and Mica Alt on current Windows 11. `AUTO` can
choose no backdrop. Opaque client painting can cover the requested material: a successful HRESULT
does not mean the folder contents became transparent. [Attributes][dwm-attributes],
[backdrop semantics][dwm-backdrop], [MFE limitations][mfe-readme].

Use the normal user's same-integrity Explorer windows; report failed HRESULTs and application
overrides. No general elevation requirement is documented for these DWM calls, but this research
does not establish a right to modify protected/higher-integrity windows. [DWM API][dwm-set].
The existing document's assertion that Mica always covers a title bar should be read with the
documented `AUTO` heuristics and application rendering limitations above.

## 2. Settings: the runtime distinction matters

For `SystemSettings.exe`, the current Settings Styler uses **platform `Windows.UI.Xaml.dll`**, the
`Windows.UI.Xaml` object model, and `VisualDiagConnectionN` endpoints. It also references
`Microsoft.UI.Xaml.Controls` controls. This establishes a UWP/platform-XAML path with WinUI controls,
not Explorer's internal WinUI 3 diagnostics path. Namespace `Microsoft.UI.Xaml.Controls` alone
does not prove WinUI 3; WinUI 2 supplies controls for UWP/XAML Islands. [Settings implementation][settings-source],
[WinUI 2 documentation][winui2].

**Yes, diagnostics reaches the Settings tree:** the mod subscribes through `IVisualTreeService3`
and styles its elements. Its present source contains workarounds for composition-diagnostics
concurrency and diagnostics-held element references. A build-26200 crash report is useful evidence
of failure, but the reporter's proposed root cause is a hypothesis, not a Microsoft diagnosis.
[Implementation][settings-source], [original crash report][settings-crash]. This should remain a
separate experimental target/runtime with a conservative property allowlist and full rollback,
not an assumption that every 26xxx page has the same tree.

## 3. A file manager we own, and default-folder routing

Files calls its default-file-manager feature **experimental**: Windows has no ordinary supported
default-file-manager selector; Files uses a registry workaround for folder launches and Win+E.
It does not replace the desktop shell merely by becoming a folder handler. [Files documentation][files-docs].

The pinned registration file writes these **per-user** locations:

- `HKCU\Software\Classes\Folder\shell\open\command`
- `HKCU\Software\Classes\Folder\shell\explore\command`
- `HKCU\Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell\opennewwindow\command`

The folder commands launch `"%LOCALAPPDATA%\Files\Files.App.Launcher.exe" "%1"` and clear
`DelegateExecute`; the CLSID command launches the same app for Win+E. Older
`Directory\shell\openinfiles` entries remain commented history, not the active technique.
[Files registration source][files-set].

Files' actual UI still launches `regedit.exe` with `Verb = "runas"`, so **that implementation prompts
for elevation even though its enable keys are HKCU**. Disabling imports an undo file which restores
Explorer commands and the native DelegateExecute GUID through HKCR, removes the old user verb, and
deletes its user Win+E CLSID override. It is a predefined restoration, not a complete backup of
arbitrary preexisting customizations. [UI implementation][files-advanced], [undo registration][files-unset].

For Glaze, prefer a normal launchable native file browser first. Folder routing can be a separate
opt-in HKCU experiment, with original value/type/absence journaling and compare-before-restore.
Those are design recommendations. The workaround does not provide replacement common file dialogs
or guarantee redirection of programs that directly invoke Explorer; Files' code exposes open/save
dialog replacement separately. Shell namespace objects, `/select` requests, multi-selection,
elevated launches, and uninstall while registered all need explicit compatibility checks.
[Files UI][files-advanced], [Shell invocation API][shell-execute]. A command-based shell action can
run out of process; an in-process shell extension may instead load in arbitrary applications,
which would conflict with our Windows-only injection policy. [Microsoft extension guidance][shell-extensions].

## 4. Fast filename search

### Privileged NTFS enumeration and journal replay

Everything documents metadata-based filename indexing and real-time filesystem monitoring, not a
prebuilt full-content index. Its SDK communicates with the Everything application; integrating
that existing application is an app-specific plugin, not our own native indexing backend.
[Everything FAQ][everything-faq], [SDK][everything-sdk].

The public Windows implementation route is:

1. Open a volume such as `\\.\C:` with the necessary access. Direct volume access requires
   administrative privileges; ordinary directory enumeration does not. [CreateFile][create-file].
2. Query journal identity/cursors, then enumerate current MFT records with `FSCTL_ENUM_USN_DATA`,
   starting at file reference zero and continuing with the returned cursor. This is a filesystem
   control operation, not the same thing as implementing a raw on-disk MFT parser.
   [Enumeration contract][usn-enum], [journal metadata][usn-data].
3. Track records by file/parent IDs and replay changes with `FSCTL_READ_USN_JOURNAL` from a saved
   USN. Its read controls can wait for data; no recurring whole-disk crawl is necessary.
   [Journal read][usn-read], [read parameters][usn-read-data].
4. Persist journal ID with the cursor. Rebuild/reconcile after an ID change or a cursor older
   than retained records; a journal is finite, not a permanent history. Enumeration and subsequent
   replay need overlap/race handling. [Journal data][usn-data].

Do not describe Everything's **exact private call sequence** as audited source: it is not published
here. The developer's explanation distinguishes the initial MFT read from USN-based maintenance;
the Microsoft calls above establish how Glaze can implement that architecture. This explanation
is historical, not a current binary audit. [Developer explanation][everything-mft].

The Everything service provides privileged filename/indexing access while the main application can
remain a standard user. Current docs also describe a 1.5 elevated indexing-process alternative.
They say any local user can obtain NTFS filenames through the product, access is restricted to
voidtools-signed Everything code, and the service does not grant access to file contents. A Glaze
plugin should use the public SDK, not try to impersonate that service client.
[Service boundary][everything-service], [SDK][everything-sdk].

**Sizing and speed:** voidtools' current FAQ estimates **100 MB RAM / 45 MB database for one million
files**, and about **one minute** for initial indexing; about 250,000 files on a fresh Windows 11
installation are quoted at five seconds. These are vendor examples with no matching hardware/cold-cache
methodology, not promises for our machine or our implementation. Additional indexed properties change
the footprint. [FAQ][everything-faq], [index options][everything-indexes].

Important correctness limits: `USN_RECORD_V2` has a name, parent/file references, reason flags, and
attributes, **not current file length or allocation size**. Hard-link changes also need more than
a naive single-parent map if every pathname is promised. [Record layout][usn-record],
[hard-link names API][file-names]. As design recommendations, keep fast names separate from lazy
size queries; isolate any elevated broker, authenticate its caller, respect each user's visibility,
and never expose unrestricted cross-user names as a side effect of a launcher query.

### Unprivileged alternatives

These APIs require appropriate directory/file permissions, not an elevation blanket. Opening a
directory with `FILE_FLAG_BACKUP_SEMANTICS` does not itself bypass ACLs without backup privileges.
[Directory access rules][create-file].

| API | What it buys | Limits / recommendation |
|---|---|---|
| `FindFirstFileExW`, `FindExInfoBasic`, `FIND_FIRST_EX_LARGE_FETCH` | Familiar traversal; basic info omits short-name work; larger query batches. [API][find-first], [info levels][find-info] | Use on a worker scanning entire directories. Large fetch can waste work on early exit; it is not a universal speed switch. [Microsoft guidance][large-fetch] |
| `NtQueryDirectoryFile` | Caller-sized buffers and multiple metadata records per call. [API][nt-query] | User-mode Native API; handle NTSTATUS, partial buffers, filesystem differences, and end-of-enumeration. Benchmark before adding a second backend. |
| `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)` | Batched directory records with 128-bit ID, parent-directory traversal context, logical/allocation sizes, attributes, reparse tags. [API][get-file-info], [structure][dir-info] | A good documented Win32 first choice where supported; use restart class to restart and fall back when unsupported. |
| `ReadDirectoryChangesExW` | Asynchronous subtree notifications and extended IDs/metadata; NTFS support. [API][read-dir-ex] | Not an initial index or persistent journal. Overflow/`ERROR_NOTIFY_ENUM_DIR` requires re-enumeration. Use regular `ReadDirectoryChangesW` where extended notifications are unsupported. [Notifications][dir-notifications] |

No controlled, comparable files/second benchmark for those four alternatives was found in the
reviewed primary docs. Smaller syscall count is a mechanism, not a measured throughput claim.
WinDirStat's current fallback is a useful source example of batched `NtQueryDirectoryFile`
enumeration and filesystem-capability fallback. [FinderBasic source][windirstat-basic].

Proposed crawler lifecycle: register notifications before scanning, reconcile buffered changes,
keep paths/IDs compact, bound concurrent directory work, and expose incomplete/unreadable scopes.
Close all handles and exit when disabled. These are design recommendations based on the notification
overflow and directory-enumeration contracts above, not claims of snapshot isolation.

### Windows Search: retain it for content

Windows Search indexes properties and, for supported file types, contents. Classic scope is narrower
than Enhanced; filename-only service work need not duplicate content filters or Outlook indexing.
[Microsoft support][search-settings]. Its architecture already uses NTFS journal notifications after
the initial crawl, so calling it an endless full-disk rescan would be incorrect.
[Indexer architecture][search-architecture].

Microsoft's troubleshooting guidance says problems can begin beyond **400,000 indexed items**, and
describes a **one-million-item** supported index scale. It also documents CPU/disk contention,
battery/activity backoff, and index database growth. These are troubleshooting guidelines, not a
fixed CPU/RAM budget or a comparison with a filename-only index. [Performance guidance][search-performance].

Without an elevated helper, offer links to the user's Search settings and scope controls, favor
Classic plus necessary user folders, and suggest excluding generated build/cache directories.
Users can manage scope through the Search UI; policy can override it. Do not promise every change
is non-admin: advanced file-type/rebuild/system-location controls and service maintenance may
elevate. Do not disable `WSearch` globally as the default optimization.
[Settings][search-settings], [scope rules][search-scope], [maintenance instructions][search-performance].

## 5. Folder sizes: share traversal, define the number

WizTree documents direct MFT scanning on NTFS, with traversal for other sources. TreeSize Free also
documents MFT access and a two-thread fallback without it (Professional allows up to 32). Current
WinDirStat is no longer just the historical slow crawler: its changelog added direct MFT scanning
and hard-link tracking in 2.5; its current `FinderNtfs` reads MFT extents from the volume in batches.
[WizTree][wiztree], [TreeSize][treesize], [WinDirStat history][windirstat-changelog],
[WinDirStat implementation][windirstat-ntfs].

MFT scanning needs privileged volume access. It avoids opening every directory/file but requires a
correct parser if reading raw records. Prefer reusing an already maintained native metadata index
where it has the needed fields; USN-name enumeration alone does not supply sizes.
[Volume access][create-file], [USN layout][usn-record], [MFT implementation][windirstat-ntfs].

The UI must distinguish these quantities:

- **Logical size:** file length (`EndOfFile`); useful for what the file represents.
- **Allocated size:** bytes allocated for the stream. Sparse/compressed files can differ greatly;
  directory records expose both, and `GetCompressedFileSizeW` documents actual compressed/sparse
  storage. ADS may require additional accounting. [Directory record][dir-info],
  [compressed size][compressed-size], [TreeSize NTFS accounting][treesize-ntfs].
- **Unique allocated total:** deduplicate hard links by volume + file ID. Different names can share
  the same data, so summing every pathname overcounts. WizTree explicitly counts these only once
  in allocation totals. Attribution between sibling folders needs a documented policy.
  [WizTree accounting][wiztree-changes], [TreeSize hard links][treesize-ntfs].
- **Cloud logical versus local resident data:** `CfGetPlaceholderInfo` needs only READ_ATTRIBUTES
  and returns placeholder metadata; `CF_PLACEHOLDER_STANDARD_INFO.OnDiskDataSize` distinguishes
  local bytes from full file length. A size scan should avoid content reads/hydration, skip
  unsupported providers cleanly, and label unavailable values. [API][cloud-info], [fields][cloud-size].
- **Space that deletion would free:** not equivalent to allocation sums. A remaining hard link or
  a ReFS block clone can retain the data; deduplicating file IDs alone cannot account for shared
  extents between distinct clone files. [Hard links][treesize-ntfs], [ReFS sharing][block-clone].

Recommendation: on-demand native scans, partial results, explicit reparse-point/mount policy, and
cached totals invalidated by real changes. Do not add automatic full-tree rescans or promise that
each Explorer folder-size column can be supplied externally without Explorer integration.
[Scan-policy precedent][treesize-scan], [notification contract][read-dir-ex].

## 6. File operations: execution and interception are different features

### Fast execution paths

| Workload | Starting point | Evidence / practical boundary |
|---|---|---|
| Same-volume move | Rename/move API | Avoid copy/delete when a move suffices. Cross-volume `MOVEFILE_COPY_ALLOWED` is simulated copying and deletion; directories cannot use this API to move across volumes. [MoveFileExW][move-file] |
| Normal local copy | `CopyFile2` | Progress/cancellation and Windows file semantics already exist; plain read/write loops must reproduce metadata/reparse handling themselves. [API][copyfile2], [parameters][copyfile2-params] |
| Very large streaming copy | Consider `COPY_FILE_NO_BUFFERING` | Bypasses cache; Microsoft recommends it for very large files and discourages pausing this mode. It is not a tiny-file optimization. [Microsoft source][copyfile2-params] |
| Many independent files | Bounded concurrency; robocopy `/MT:n` as a reference | `/MT` supports 1–128, default 8 when specified without a count; `/J` is unbuffered. More workers are not evidence of higher throughput. [Robocopy][robocopy] |
| Same-volume ReFS/Dev Drive copy | Let Windows clone blocks where eligible | Metadata remapping plus copy-on-write; constrained by same volume, range/alignment, and filesystem requirements. [Block cloning][block-clone], [FSCTL][duplicate-extents] |
| Specialized high-depth asynchronous I/O | Probe I/O rings | `QueryIoRingCapabilities` and per-op support checks are required. The current SDK declares read/write/flush builders even though the Learn header overview does not list every builder. This is a candidate backend, not a proven faster replacement for `CopyFile2`. [Capabilities][ioring-caps], [SDK header][sdk-ioring] |

Windows 11 **24H2 adds automatic block cloning to Windows copying on Dev Drive**, so this is not a
reason to hook Explorer. Microsoft's 2024 examples report a 10 GB copy falling from 7.964 s to
0.641 s and an 18 GB/5,815-file folder from 30.867 s to 6.306 s. These are specific published copy
benchmarks, not internet downloads, cross-volume copies, or general build speedups.
[Dev Drive availability][dev-drive], [Microsoft measurements][devdrive-benchmark].

Normal copies/moves/clones need the caller's ordinary file permissions, not admin simply because
they are fast. Backup/security privileges, protected destinations, and creating a Dev Drive are
separate elevation cases. An I/O ring does not grant extra access to its file handles.
[Copy parameters][copyfile2-params], [Dev Drive prerequisites][dev-drive], [SDK contracts][sdk-ioring].

### Capturing Explorer's copies

`IFileOperation` is a public API for operations **we initiate**; its progress sink observes those
operations. It is not a global subscription or registration point for replacing Explorer's engine.
[IFileOperation][file-operation], [progress sink][file-progress].

`ICopyHook` is an in-process COM DLL for approving/vetoing folder/printer operations. It does not
perform the copy or receive completion notifications. Calling it a generic replacement-copy-engine
interface is wrong. [Microsoft copy hook documentation][copy-hook].

TeraCopy officially supports an Explorer/TeraCopy choice for drag/drop or copy/paste, a default-copy-
handler setting, context-menu entry points, and explicit CLI jobs. Its public support pages do not
disclose enough implementation detail to certify a particular `IFileOperation` hook or DLL loading
sequence. An in-process interception of Explorer's COM calls would count as injection under our
policy; it must not be inferred merely from the advertised behavior.
[Vendor workflow][teracopy-workflow], [CLI][teracopy-cli].

Recommended order: native on-demand operations from Glaze; an app-specific TeraCopy CLI plugin;
only later an off-by-default, allowlisted Explorer capture experiment. A command-based shell verb
can launch out of process; avoid registering a DLL handler that may enter file dialogs in games
or third-party applications. [Shell extension scope][shell-extensions].

## 7. Developer workloads

**Dev Drive.** ReFS volumes are intended for source repositories, caches, and build outputs.
Creation needs local admin and at least 50 GB; existing contents cannot be preserved by a conversion
in place. Use a new VHD/volume only after the user chooses it. Normal work on the created drive does
not need elevation. [Setup and intended data][dev-drive].

**Defender performance mode.** A trusted Dev Drive with real-time protection enabled can defer
scans until after file open, reducing synchronous delay. It is not an antivirus exclusion and
does not turn scanning off. Trust changes and `Set-MpPreference` configuration require admin;
organizational policy can control them. [Defender documentation][defender-mode].

**What actually sped builds in a primary study:** Microsoft reported workload-dependent reductions,
with a highest result of 43% and many repositories at 10% or more; C# dependency copying benefited
more than most C++ builds. The study used internal repositories, same-VM NTFS/Dev Drive partitions,
CoW-enabled build SDKs, multiple warm-cache iterations, and excluded package restore/tests. It did
not establish the same improvement for Rust, Unity, or this checkout. [Study and methodology][cow-builds].

**Defender exclusions.** Record the slow operation with `New-MpPerformanceRecording` and inspect
`Get-MpPerformanceReport` before changing scan policy. Recording requires administrator PowerShell.
Exclusions deliberately remove scan coverage, so a targeted, reversible change should follow an
identified cost; do not exclude all developer tools or the whole user profile by default.
[Analyzer][defender-analyzer], [exclusion guidance][defender-exclusions].

**Long paths.** `HKLM\SYSTEM\CurrentControlSet\Control\FileSystem\LongPathsEnabled=1` and the
application's `longPathAware` manifest opt-in work together; setting the machine value needs admin.
It does not repair older tools or make every Shell UI path work. Unicode extended paths are an
additional API-specific option. This is correctness/compatibility work, not a build-speed claim.
[Microsoft long-path contract][long-paths].

**Git.** Repository-local `core.fsmonitor=true` lets supported Git commands use a filesystem daemon;
`core.untrackedCache=true` reduces repeated untracked-directory scans. They need no admin for an
owned repository. Use compatible Git versions across CLI/IDE; network-mounted repositories are
experimental for fsmonitor. This accelerates status/index refresh, not compilation. Disabling our
plugin must undo only its own settings; a Git daemon it starts is a real background resource, not
zero-cost just because it is event-driven. [Git config][git-config], [daemon behavior][git-fsmonitor],
[status performance][git-status].

Recommendation: an on-demand native Dev Drive/Defender inspection page and an app-specific Git
plugin. Measure build wall time, file I/O, and scan time before assigning an optimization. No
machine changes were made during this research.

## 8. Games: documented mechanisms, not a universal boost preset

No universal current 24H2/25H2 FPS gain for these switches was established in the reviewed primary
sources. The table separates documented mechanisms from a reason to benchmark. Admin requirements
refer to the change, not merely opening its Settings page; device policy can further restrict it.

| Setting | Supported conclusion | Elevation / Glaze position |
|---|---|---|
| Game Mode | Current Windows exposes it under Gaming. Older developer docs describe resource prioritization but explicitly deprecate their Game Mode APIs from Windows 10 1809; those APIs are not a current external game detector. [Settings][windows-settings], [API status][game-mode] | User setting normally needs no admin. Offer the native page; do not promise a fixed FPS increase. |
| HAGS | Hardware/driver-dependent scheduling offload. Microsoft's architecture article expected transparent behavior rather than a significant general uplift; it is historical evidence, not a 2026 benchmark. [DirectX team][hags] | System-wide setting; plan for admin/restart if automating, and verify on target. Prefer the Settings page and per-machine A/B results. |
| MPO | GPU hardware composition can reduce latency and power. NVIDIA provides disable/restore registry files plus restart, but also documents MPO's benefits. Neither source proves blanket disabling makes games faster. [NVIDIA][mpo], [presentation explanation][mpo-benefits] | Machine registry workaround: admin/restart. Keep as a targeted experimental troubleshooting action, not a default. |
| Optimizations for windowed games | Moves compatible DX10/11 windowed/borderless games from blt to flip presentation, reducing frame latency and allowing modern display features. Supports per-app opt-out. [Microsoft][windowed-games] | User graphics settings normally need no admin; game restart. Useful supported option for its stated scope. |
| Power mode / plans | Windows offers efficiency/balanced/performance modes; actual gains depend on power limits and workload. No cited universal gaming gain from an Ultimate Performance plan. [Microsoft][power-mode] | Ordinary mode selection usually no admin; policy/custom plans may restrict it. Keep changes reversible and restore the user's mode. |
| Timer-resolution tools | `timeBeginPeriod` is not a reliable way for our process to boost another game's timer. Since 2004 its request is process-scoped; Windows 11 may revoke precision for invisible/inaudible window-owning apps. Higher resolution can increase switching and prevent power saving. [API][timer-period] | No admin for the API. Do not add a resident timer utility or permanent 1 ms request. |
| Game DVR / background capture | Historical capture is a real recording feature, distinct from simply having Game Bar installed. Disabling unwanted active capture avoids that work; no fixed FPS claim follows. Its WinRT settings API is restricted to provisioned developer accounts. [Capture API][capture-settings], [Windows UI][windows-settings] | User Captures settings normally no admin. Device policy is administrative, but its CSP documentation says enforcement is Windows 10 desktop only; do not promise it controls 24H2/25H2. Link the native page. [Policy][gamedvr-policy] |

Calling every non-helpful tweak “placebo” is stronger than the evidence. The defensible rejection is
**no demonstrated general benefit** for blanket MPO disabling, an external timer tool, arbitrary
power-plan imports, or forcing HAGS one way on all GPUs. Benchmark the affected game/configuration;
do not bundle unrelated registry changes. This is a recommendation inferred from the contracts and
evidence limits above.

### What zero overhead requires from Glaze

The [night plan](../../plans/night/2026-11-06.md) requires disabled optional services to have no
process/hook/thread/timer/memory. A blocked worker can approach zero idle CPU while retaining memory;
it is not disabled. Literal **zero product footprint during a game** requires graceful shutdown of
all Glaze processes and unloading any injected code; process exit releases its resources, but
does not automatically terminate child processes. A retained automatic-resume watcher still costs
memory and event handling. [Windows process lifecycle][process-exit].

Proposed game quiescence contract:

1. Stop wallpaper presents, simulation, video decoding, audio capture, animation, previews, and
   polling. Release render/decoder resources by stopping their owning optional processes when full
   memory recovery is requested. Restore Explorer's taskbar/desktop before stopping our shell.
2. Stop file-index watchers and indexing workers, plugin hosts, telemetry sampling, palette work,
   and optional styling hooks. Persist journal cursors and resume/reconcile after the game; do not
   force-cancel an active copy job without an explicit user policy.
3. Support an explicit game profile/launcher lifecycle and a conservative foreground heuristic.
   `SetWinEventHook(WINEVENT_OUTOFCONTEXT)` avoids injecting into the game, but still dispatches
   callbacks. `SHQueryUserNotificationState` is a notification-suppression hint and sends no
   full-screen start/stop notification; it cannot identify every borderless game.
   [WinEvent contract][winevent], [notification-state contract][notification-state].
4. State the tradeoff honestly: automatic resume needs an observer; a mode that retains core can
   promise no rendering/background jobs, not literally no memory. Full exit needs manual/external
   relaunch or an existing launcher's lifecycle integration. Never inject into a game or bypass the
   [current process allowlist](../../crates/inject/src/lib.rs).

Acceptance recommendation: compare the same reproducible game scene with Glaze off/on/quiesced,
using PresentMon frame-time capture plus process CPU/I/O/GPU activity; include tail frame times,
second-monitor behavior, and alt-tab. PresentMon provides external performance capture, so this
does not require game injection. No such measurement was performed for this report.
[PresentMon primary source][presentmon].

## Sources

Source links below point to official API documentation, vendor documentation, or project-owned
source. Key source snapshots: Windhawk mods `0103a40`, Explorer guide `340394c`, Files `11e4422`,
ExplorerBlurMica `5fd0b0b`, MFE 1.x `78e85ca`, WinDirStat `f663f68`, Windows SDK mirror `ae66c06`.
The MFE 1.x implementation is deliberately distinguished from the current 2.x rewrite README.

[explorer-source]: https://github.com/ramensoftware/windhawk-mods/blob/0103a40ca706a04c726b234d5147a648e4ca256a/mods/windows-11-file-explorer-styler.wh.cpp
[explorer-guide]: https://github.com/ramensoftware/windows-11-file-explorer-styling-guide/blob/340394c46dfbc8f25263691980171b587ef341ab/README.md
[settings-source]: https://github.com/ramensoftware/windhawk-mods/blob/0103a40ca706a04c726b234d5147a648e4ca256a/mods/windows-11-settings-styler.wh.cpp
[settings-crash]: https://github.com/ramensoftware/windhawk-mods/issues/4399
[tree-mod]: https://github.com/ramensoftware/windhawk-mods/blob/0103a40ca706a04c726b234d5147a648e4ca256a/mods/hide-home-gallery-explorer.wh.cpp
[xaml-init]: https://learn.microsoft.com/en-us/windows/win32/api/xamlom/nf-xamlom-initializexamldiagnosticsex
[xaml-diagnostics]: https://learn.microsoft.com/en-us/windows/win32/api/xamlom/nn-xamlom-ixamldiagnostics
[visual-tree]: https://learn.microsoft.com/en-us/windows/win32/api/xamlom/nn-xamlom-ivisualtreeservice
[winui2]: https://learn.microsoft.com/en-us/windows/uwp/get-started/winui2/
[blur-readme]: https://github.com/Maplespe/ExplorerBlurMica/blob/5fd0b0b83b3564f7d51c9be966ec190f5e70f0f9/README.md
[blur-module]: https://github.com/Maplespe/ExplorerBlurMica/blob/5fd0b0b83b3564f7d51c9be966ec190f5e70f0f9/ExplorerBlurMica/module.h
[blur-hooks]: https://github.com/Maplespe/ExplorerBlurMica/blob/5fd0b0b83b3564f7d51c9be966ec190f5e70f0f9/ExplorerBlurMica/HookDef.h
[blur-releases]: https://github.com/Maplespe/ExplorerBlurMica/releases
[startallback]: https://www.startallback.com/
[set-window-theme]: https://learn.microsoft.com/en-us/windows/win32/api/uxtheme/nf-uxtheme-setwindowtheme
[theme-subclasses]: https://learn.microsoft.com/en-us/windows/win32/controls/theme-subclasses
[sdk-vsstyle]: https://github.com/microsoft/win32metadata/blob/ae66c067ff3e9516fcc9cda09204bf72a9d57e56/generation/WinSDK/RecompiledIdlHeaders/um/vsstyle.h
[secure-uxtheme]: https://github.com/namazso/SecureUxTheme
[mfe-dwm]: https://github.com/MicaForEveryone/MicaForEveryone/blob/78e85cae0519d87800d5f0c856fe75ac229e74df/MicaForEveryone.Win32/DesktopWindowManager.cs
[mfe-readme]: https://github.com/MicaForEveryone/MicaForEveryone/blob/38c5718891e0af516e08e23c8490d7e007bdcd20/README.md
[dwm-set]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmsetwindowattribute
[dwm-attributes]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
[dwm-backdrop]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type
[files-docs]: https://files.community/docs/customize-settings/advanced
[files-set]: https://github.com/files-community/Files/blob/11e44223a4bc1a0a21e0fc39de2a46160cda95f4/src/Files.App/Assets/FilesOpenDialog/SetFilesAsDefault.reg
[files-unset]: https://github.com/files-community/Files/blob/11e44223a4bc1a0a21e0fc39de2a46160cda95f4/src/Files.App/Assets/FilesOpenDialog/UnsetFilesAsDefault.reg
[files-advanced]: https://github.com/files-community/Files/blob/11e44223a4bc1a0a21e0fc39de2a46160cda95f4/src/Files.App/ViewModels/Settings/AdvancedViewModel.cs
[shell-execute]: https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shellexecuteexw
[shell-extensions]: https://learn.microsoft.com/en-us/windows/win32/shell/shell-and-managed-code
[create-file]: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew
[usn-enum]: https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_enum_usn_data
[usn-read]: https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_read_usn_journal
[usn-read-data]: https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-read_usn_journal_data_v0
[usn-data]: https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-usn_journal_data_v0
[usn-record]: https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-usn_record_v2
[file-names]: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstfilenamew
[everything-faq]: https://www.voidtools.com/faq/
[everything-service]: https://www.voidtools.com/support/everything/everything_service/
[everything-mft]: https://www.voidtools.com/forum/viewtopic.php?t=357
[everything-sdk]: https://www.voidtools.com/support/everything/sdk/
[everything-indexes]: https://www.voidtools.com/support/everything/indexes/
[find-first]: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstfileexw
[find-info]: https://learn.microsoft.com/en-us/windows/win32/api/minwinbase/ne-minwinbase-findex_info_levels
[large-fetch]: https://devblogs.microsoft.com/oldnewthing/20131024-00/?p=2843
[nt-query]: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntquerydirectoryfile
[get-file-info]: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex
[dir-info]: https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_extd_dir_info
[read-dir-ex]: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesexw
[dir-notifications]: https://learn.microsoft.com/en-us/windows/win32/fileio/obtaining-directory-change-notifications
[search-settings]: https://support.microsoft.com/en-us/windows/experience/performance-optimization/search-indexing-in-windows
[search-performance]: https://learn.microsoft.com/en-us/troubleshoot/windows-client/shell-experience/windows-search-performance-issues
[search-architecture]: https://learn.microsoft.com/en-us/windows/win32/search/-search-indexing-process-overview
[search-scope]: https://learn.microsoft.com/en-us/windows/win32/search/-search-3x-wds-extidx-csm-scoperules
[wiztree]: https://www.diskanalyzer.com/about
[wiztree-changes]: https://www.diskanalyzer.com/whats-new
[treesize]: https://www.jam-software.com/treesize/features.shtml
[treesize-ntfs]: https://manuals.jam-software.com/treesize/EN/notesonntfs.html
[treesize-scan]: https://manuals.jam-software.com/treesize/EN/scan_options.html
[windirstat-changelog]: https://github.com/windirstat/windirstat/blob/f663f6807f23b4f03be39ea27a7866008d8f84dc/CHANGELOG.md
[windirstat-ntfs]: https://github.com/windirstat/windirstat/blob/f663f6807f23b4f03be39ea27a7866008d8f84dc/windirstat/FinderNtfs.cpp
[windirstat-basic]: https://github.com/windirstat/windirstat/blob/f663f6807f23b4f03be39ea27a7866008d8f84dc/windirstat/FinderBasic.cpp
[compressed-size]: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getcompressedfilesizew
[cloud-info]: https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfgetplaceholderinfo
[cloud-size]: https://learn.microsoft.com/en-us/windows/win32/api/cfapi/ns-cfapi-cf_placeholder_standard_info
[copyfile2]: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-copyfile2
[copyfile2-params]: https://github.com/MicrosoftDocs/sdk-api/blob/docs/sdk-api-src/content/winbase/ns-winbase-copyfile2_extended_parameters.md
[move-file]: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw
[robocopy]: https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/robocopy
[ioring-caps]: https://learn.microsoft.com/en-us/windows/win32/api/ioringapi/nf-ioringapi-queryioringcapabilities
[sdk-ioring]: https://github.com/microsoft/win32metadata/blob/ae66c067ff3e9516fcc9cda09204bf72a9d57e56/generation/WinSDK/RecompiledIdlHeaders/um/ioringapi.h
[block-clone]: https://learn.microsoft.com/en-us/windows-server/storage/refs/block-cloning
[duplicate-extents]: https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_duplicate_extents_to_file
[devdrive-benchmark]: https://blogs.windows.com/windowsdeveloper/2024/05/21/unlock-a-new-era-of-innovation-with-windows-copilot-runtime-and-copilot-pcs/
[file-operation]: https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ifileoperation
[file-progress]: https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ifileoperationprogresssink
[copy-hook]: https://learn.microsoft.com/en-us/windows/win32/shell/how-to-create-copy-hook-handlers
[teracopy-workflow]: https://support.codesector.com/en/articles/8789942-copying-and-verifying-files
[teracopy-cli]: https://support.codesector.com/en/articles/8789945-teracopy-command-line
[dev-drive]: https://learn.microsoft.com/en-us/windows/dev-drive/
[defender-mode]: https://learn.microsoft.com/en-us/defender-endpoint/microsoft-defender-endpoint-antivirus-performance-mode
[cow-builds]: https://devblogs.microsoft.com/engineering-at-microsoft/copy-on-write-performance-and-debugging/
[defender-analyzer]: https://learn.microsoft.com/en-us/defender-endpoint/tune-performance-defender-antivirus
[defender-exclusions]: https://learn.microsoft.com/en-us/defender-endpoint/microsoft-defender-antivirus-exclusions-overview
[long-paths]: https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation
[git-config]: https://git-scm.com/docs/git-config
[git-fsmonitor]: https://git-scm.com/docs/git-fsmonitor--daemon
[git-status]: https://git-scm.com/docs/git-status
[windows-settings]: https://support.microsoft.com/en-us/accessibility/windows/understand-and-explore-windows-settings
[game-mode]: https://learn.microsoft.com/en-us/previous-versions/windows/desktop/gamemode/game-mode-portal
[hags]: https://devblogs.microsoft.com/directx/hardware-accelerated-gpu-scheduling/
[mpo]: https://nvidia.custhelp.com/app/answers/detail/a_id/5157/
[mpo-benefits]: https://nvidia.custhelp.com/app/answers/detail/a_id/5159/
[windowed-games]: https://support.microsoft.com/en-us/windows/hardware/display-graphics/optimizations-for-windowed-games-in-windows-11
[power-mode]: https://support.microsoft.com/en-us/windows/change-the-power-mode-for-your-windows-pc-c2aff038-22c9-f46d-5ca0-78696fdf2de8
[timer-period]: https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timebeginperiod
[capture-settings]: https://learn.microsoft.com/en-us/uwp/api/windows.media.capture.appcapturesettings.ishistoricalcaptureenabled?view=winrt-26100
[gamedvr-policy]: https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-applicationmanagement#allowgamedvr
[winevent]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook
[notification-state]: https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shqueryusernotificationstate
[process-exit]: https://learn.microsoft.com/en-us/windows/win32/procthread/terminating-a-process
[presentmon]: https://github.com/GameTechDev/PresentMon

## Closing feature table

**Estimates are engineering judgment**, not source claims or commitments: S = 1–2 engineer-days,
M = 3–5 days, L = 1–2 weeks, XL = several weeks or more, including focused verification.
Native means code in our own processes; optional workers exist only while enabled/needed.
Experimental means off by default; it is mandatory for code inside another process.
Plugin means app-specific integration, with a native host bridge if a WASM plugin needs Win32 IPC.
No row promises that an enabled index has zero memory; disabled service resources must be released.

| Feature Glaze could offer | How / evidence | Admin? | Injection? | Placement | Rough size |
|---|---|---|---|---|---|
| Explorer frame colors/corners/backdrop | Existing external DWM attributes; client opacity limits. [API][dwm-attributes] | Normally no | No | Native, existing styling | S |
| Explorer tabs/bars/details/Home/Gallery theme | WinUI TAP + selectors + reversible properties. [Source][explorer-source] | Not intrinsic for same-user host; installer may | Yes, Explorer | Experimental native styler | L |
| Explorer legacy interior theme | uxtheme/DirectUI paint and layout handling, build probes. [Hooks][blur-hooks] | Not intrinsic for same-user host | Yes, Explorer | Experimental native styler | L–XL |
| Settings interior theme | Separate platform-XAML TAP/runtime and rollback. [Source][settings-source] | Not intrinsic; packaging/integrity may | Yes, Settings | Experimental native styler | L |
| Our own file browser | Native owned UI + Shell/file APIs; launch normally first. [Operations][file-operation] | No for user data | No | Native on-demand app | XL |
| Default-folder / Win+E routing | Journaled HKCU command/CLSID overrides. Files' installer elevates. [Registration][files-set], [UI][files-advanced] | HKCU design no; Files implementation yes | No | Experimental native setup | M, beyond browser |
| Filename search without admin | Batched directory crawl + asynchronous change notifications. [APIs][dir-info] | No within ACLs | No | Optional native service | L |
| Whole-volume NTFS index | MFT enumeration + USN replay, narrow broker and recovery. [API][usn-enum] | Yes for broker/setup | No | Optional native service; experimental until verified | L, beyond crawler |
| Use existing Everything | Public SDK through bounded host IPC bridge. [SDK][everything-sdk] | Plugin no; vendor service setup yes | No | Plugin | S–M |
| Folder-size inspector/cache | Share metadata scan; logical/allocated/link/cloud accounting. [Fields][dir-info], [cloud][cloud-size] | No crawler; yes raw-MFT fast path | No | Optional native service/job | M–L, beyond crawler |
| Copy/move queue | Shell semantics or CopyFile2, bounded jobs, clone fallback. [API][copyfile2] | No except destination/privilege needs | No | On-demand native job service | L |
| I/O-ring copy experiment | Capability/op probes and measured alternative backend. [SDK][sdk-ioring] | No extra privilege | No | Experimental native backend | M–L, beyond queue |
| TeraCopy action | Explicit CLI job, no global capture promise. [CLI][teracopy-cli] | Normally no | No from Glaze | Plugin | S |
| Capture Explorer copy/paste | Separate allowlisted interception; no general ICopyHook engine replacement. [Contract][copy-hook] | Method/install dependent | Yes for proposed in-process hooks | Experimental native styler/capture | L–XL |
| Windows Search scope helper | Open native settings, user-folder scope suggestions. [Scope][search-scope] | No page/user scope; advanced actions may | No | Native on demand | S |
| Dev Drive advisor/setup | Inspect, then user-selected new volume/VHD and trust setup. [Docs][dev-drive] | Yes to create/configure | No | Native on demand | M |
| Defender performance diagnosis | Explicit recording/report and targeted reversible configuration. [Analyzer][defender-analyzer] | Yes to record/configure | No | Native on demand | S–M |
| Long-path compatibility control | Explain manifest + machine setting; journal machine value. [Docs][long-paths] | Yes to machine setting | No | Native on demand | S |
| Git status acceleration | Repo-local fsmonitor/untracked cache, compatible tool checks. [Git][git-config] | No | No | Plugin | S |
| Gaming settings shortcuts | Game Mode, windowed optimization, capture/power pages; no generic FPS promise. [Windows][windows-settings] | Usually no; HAGS/MPO automation may/does | No | Native on demand; MPO workaround experimental | S |
| Game quiescence / full exit | Stop optional processes/jobs; distinguish resume watcher from literal zero footprint. [Lifecycle][process-exit] | No | No game injection | Native supervision | M–L |
