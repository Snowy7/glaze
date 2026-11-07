# App styler round 4 evidence (2026-11-06): Explorer's memory

Goal: style Explorer without growing its memory beyond the flag-off control. **Not reached**: the
only thing that frees what XAML Diagnostics holds is destroying it, and destroying it while
Explorer runs corrupts Explorer's XAML state. `experimental.app_styler` stays **off**.

## What holds the memory (WinUI source, `microsoft/microsoft-ui-xaml`, `dxaml/xcp/components/xamlDiagnostics`)

- `RuntimeObjectCache` (one per process) keeps `m_parentlessObjects`: **strong** references
  (`shared_ptr<RuntimeObject>`, each holding its XAML object by `ComPtr`) to every parentless
  object XAML Diagnostics has seen. Only `RuntimeObject::Close()` takes one out.
- **Tree roots.** `AdviseVisualTreeChange` (`GiveRootsToCallback`) and every new window
  (`SignalRootMutation(Add)`) put the root there. A WinUI 3 `DesktopWindowXamlSource` (Explorer's
  island hosts) adds **itself** as the root, but on close only its inner `XamlIsland` signals
  `Remove` (`XamlIsland_Partial.cpp`), so the source's root is never closed and the tree under it
  stays alive. That is the attached-no-rules growth (round 3: 127-151 MB per burst, flag off 8-17).
- **Handles we ask for.** `GetHandleFromIInspectable` / `RegisterInstance` for an object that isn't
  in the live tree (a brush, a created value, the scratch element) makes a new parentless
  `RuntimeObject`: also kept for good. Per handle, on top of the per-subscription roots.
- **What empties it:** only `~XamlDiagnostics` (`ReleaseInstance<RuntimeObjectCache>`), when the
  last reference to the XAML Diagnostics object goes. `UnadviseVisualTreeChange` only drops the
  callback. Visual Studio never lets go of it ("VS keeps this instance alive even if someone
  detaches", `XamlDiagnostics.cpp`), so this teardown is not a path Microsoft runs in a live app.

## What was tried: recycling the session (commit 0eb5c89, reverted)

After a folder window or island host closed (a destroy hook per Explorer, 2 s quiet), the TAP
handed its look over (styled elements held), let go of XAML Diagnostics and signalled; the helper
attached again and the new session took the look over from the replay. Styling held across every
recycle (log lines `handed over` / `took over N element(s), 0 of them gone`).

It **hangs Explorer**: three runs, each one's only UI thread stopped answering at burst 9
(13-14 recycles). Run 3's stack (`recycle-run3-stacks.txt`): the UI thread spinning in
`std::unordered_map<KnownPropertyIndex, TrackerPtr<IInspectable>>` lookup inside
`DirectUI::DependencyObject::GetValue`, a corrupted hash map. `~XamlDiagnostics` runs on the
thread that drops the last reference (the TAP's session thread) and releases thousands of XAML
objects belonging to UI threads; `DiagnosticsInterop` also keeps a raw `XamlDiagnostics*` that UI
threads use unlocked. In the shell Explorer every folder window has its own UI thread, so there
is no one thread that could do the teardown safely.

## Bursts (separate `explorer /separate` process, private MB)

Each burst: 2 tabs added in the anchor window (UI Automation Invoke on `AddButton`), each navigated
6 times across folders, Home and Gallery, then closed (`CloseButton`); 30 s apart (recycle: 10 s,
5 s in run 3). One fresh process per column.

| After burst | Control (nothing attached) | Attached, no rules | Recycle run 1 | run 2 | run 3 |
|---:|---:|---:|---:|---:|---:|
| start | 130.8 | 130.8 | 130.5 | 133.6 | 131.3 |
| 1 | 168.9 | 272.6 | 169.3 | 195.8 | 181.5 |
| 2 | 180.0 | 276.4 | 197.2 | 197.4 | 194.1 |
| 3 | 202.4 | 383.8 | 304.1 | 246.1 | 302.7 |
| 4 | 361.2 | 525.6 | 396.4 | 382.7 | 393.3 |
| 5 | 363.6 | | 396.2 | 382.5 | 393.9 |
| 6 | 371.3 | | 477.3 | 395.3 | 400.5 |
| 7 | 592.8 | | 818.2 | 540.3 | 672.4 |
| 8 | 667.4 | | 797.3 | 620.9 | 711.4 |
| 9 | 670.4 | | **hung** | **hung** | **hung** |
| 10 | 670.9 | | | | |

Recycling tracked the control within this workload's noise, so it does free the trees, but it
hangs. Tabs in a separate Explorer grow a lot on their own (the control too), so this workload
can't resolve the attached-no-rules difference finely; round 3's shell-Explorer windows (8-17 MB
per burst flag off) are the cleaner control. The attached-no-rules session ending gave nothing
back (525.6 -> 525.3 MB): after detaching, something still held the round-3 TAP's callback, whose
`Arc<Session>` kept XAML Diagnostics alive (probe log: `session refs 2`). That is round 3's
"ending the session doesn't reliably give it back".

## Unplanned live run (09:54-10:00)

`experimental.app_styler` was found **on** at 09:54 (not set by this round) with this round's
uncommitted build deployed by another agent. The shell Explorer (76024) was recycled 4 times and
Settings styled (`unplanned-live-*.log`): no hang there. The flag was turned off at 10:00:35; every
session put its look back and detached.

## Files

| File | What |
|---|---|
| `bursts.ps1`, `r4.ps1`, `watchdog.ps1` | The burst runner, one variant per fresh separate Explorer, and the hang watchdog (stacks via `cdb -pv`, then ends that test process). |
| `control.log`, `attached-no-rules.log`, `recycle-run*.log` | The table above. |
| `recycle-run1-tap.log` | The TAP's log of run 1: 15 sessions, then a hand-over that didn't answer, then an advise that never returned. |
| `recycle-run3-hang.txt`, `recycle-run3-stacks.txt` | Run 3's busiest threads (the UI thread 3 s of CPU in 3 s) and stacks. |
| `unplanned-live-*.log` | The live recycles above. |

## What would fix it

1. **In WinUI** (the real fix): `DesktopWindowXamlSource::Close` signalling `Remove` for the object
   it added (or `RemoveRootObjectFromLVT` mapping the inner island to its source), so closed
   windows leave the cache while a session stays attached. Worth a WinUI issue.
2. **In the TAP**: never ask for handles of objects outside the tree (`handle_of` on held brushes,
   values and the scratch; read them from the property chain or keep the creation handle), which
   removes the per-handle part; the per-root part stays until (1).
3. Not safe: any design that ends XAML Diagnostics in a live Explorer (recycling, unadvise when the
   last window closes, short sessions per window), for the reason above.
