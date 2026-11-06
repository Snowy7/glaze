# Native file index (experimental)

`mypaper-index` is an optional native process, enabled with
`"experimental": { "file_index": true }` in `%APPDATA%\mypaper\config.json`.
The default is **false**. Core starts it when enabled, stops it on disable/quit/panic,
and supervises crashes with its existing backoff. A wait-only parent handle also makes
the helper exit if core disappears. Disabled means no index process, watch, thread or timer.
No code runs in Explorer or in a plugin.

With `launcher.files` enabled in `bars.json`, typing in the launcher queries the helper
on a dedicated panel-owned worker lane. Superseded queued queries are skipped, and
replies for old text are ignored. Files retain the helper's ranking; folder results show
their logical subtree size and file count. If the pipe is unavailable or times out,
the existing recent-document search remains available. Apps and Settings keep their
existing behavior. Empty-query recommendations still use recent documents.
The bounded async pipe deadline is 250 ms: warm queries normally finish much sooner,
but an 11M-entry query after resident-page trimming was measured above 100 ms.

## Store and search

- Forty-byte fixed records: parent/child/sibling IDs, original/folded name offsets,
  attributes, modified time (Unix seconds), logical size and subtree file count.
- One UTF-8 name blob; lowercase names share the original bytes when possible.
  Unicode lowercase is locale-independent, without accent removal or normalization.
- `%LOCALAPPDATA%\mypaper\index\snapshot.bin` is a read-only file mapping. Small
  live changes use a bounded record/name overlay, atomically persisted to `changes.json`
  with a snapshot generation. Ordinary changes do not rewrite the roughly 900 MB base.
  Above 50,000 overlay records or 4 MiB new names, a checkpoint compacts tombstones and
  superseded names, syncs and replaces the snapshot, validates its mapping, and swaps it
  under a short exclusive lock. Queries continue during disk I/O and validation.
- Full-volume scan scratch records/names use sparse temporary file mappings under
  `index/scan/`, instead of large private buffers. A four-byte ID map streams the fresh
  volume beside the other cached volumes into the next snapshot. Scan mappings are
  removed after use; recognizable leftovers from a killed service are removed only after
  exclusive pipe ownership. The service excludes its own cache tree from indexing.
- Substring terms are ANDed. Exact name matches rank first, then name prefix, word
  prefix, substring; newer modification times break ties. Filters: `ext:rs,toml`,
  `path:"Program Files"`, `folder:photos`, `file:report`. Quotes work within filters.
- Up to eight search workers are created on the first large query, then park between
  requests. Each searches a shard with reusable substring finders and a bounded top-k
  heap. Parent-path filters are reused across adjacent siblings. Wide directories get
  lazy hash-to-record lookups for updates, with original-name verification on collisions.

Search is **names and paths only**, not document contents. Sizes count every accessible
file's logical length, including each directory entry of a hard link. They do not
measure allocated/compressed disk space. Protected directories may be inaccessible;
reparse points (junctions, symlinks and cloud placeholders marked reparse) are omitted.

## Enumeration and updates

Fixed NTFS/ReFS drive-letter volumes are enumerated without elevation, through
`FindFirstFileExW(FindExInfoBasic)` and `FindNextFileW`,
with four background-priority directory workers. No file contents are opened.
`--bench-scan PATH` compares `FIND_FIRST_EX_LARGE_FETCH` with basic fetch in alternating
order. On this machine the best warm registry-tree sample was **0.471 s Basic** versus
**0.565 s Large** for 136,889 entries; Basic is selected. Concurrent disk/build activity
made wall times variable, so both modes remain available for remeasurement.

A recursive, blocking `ReadDirectoryChangesExW` watch with a 2 MiB aligned buffer is
armed on each root before scanning. ReFS uses `ReadDirectoryChangesW` when the extended
API reports unsupported. Changes are deduplicated and applied as final filesystem
state; adds, resizes, removals and directory moves update ancestor sizes/counts.
New/moved-in directories are scanned as subtrees. Readers run at normal CPU priority
so notification draining is not starved by a build. A separate background delivery thread
coalesces paths in a bounded set (128,000 unique paths/root) while metadata work or the
service queue is busy. Backpressure alone never drops notifications. Both threads block
between changes; there is no polling timer.

If a notification buffer overflows, Windows has discarded the affected names. The
affected **watched subtree** is the volume root, so that volume is reconciled; other
volumes are left alone. The non-admin API cannot safely identify a smaller lost subtree.
This is a limitation relative to precise journal replay.

A cached snapshot answers queries immediately after restart. A background reconciliation
still runs once because changes made while the service was off cannot be recovered
reliably without a journal. Buffered notifications are replayed after scanning. An
event-driven two-second quiet deadline (or two minutes during sustained writes)
checkpoints changes; there are no checkpoint wakeups when clean.

Scratch mappings close after each volume is checkpointed. Reconciliation completion and
queries arm a one-shot two-second quiet deadline; `EmptyWorkingSet` then returns resident
pages to Windows' reclaimable cache. Status requests do not arm it. There is no recurring
trim timer. A new query faults the needed pages back in, so warm-query latency and the
first query after a trim must be measured separately. A whole-index substring scan touches
much of the mapped snapshot while active; low private allocation is not low resident RAM.

**TODO:** a separately authorized elevated MFT bootstrap / USN replay path. Neither
admin access nor journal creation is attempted by this experiment. Faster offline-gap
recovery and narrower overflow repair depend on that future work.

## Pipe API

`\\.\pipe\mypaper-index` is a local, message-mode named pipe with an explicit current-user
SID DACL and remote clients rejected. First-instance creation rejects an existing pipe.
Each transaction is one UTF-8 JSON request message and one UTF-8 JSON response message,
without a newline or length prefix. Requests are limited to 8 KiB and replies to 1 MiB.
Four listeners serve concurrent clients. I/O has cancellation deadlines.

The Rust library's bounded client is the preferred integration:

```rust
use mypaper_index::{Request, pipe};

let result = pipe::request(&Request::Search {
    query: "ext:rs path:projects index".into(), limit: 50,
}, 250)?;

let sizes = pipe::request(&Request::Sizes {
    path: r"D:\Coding".into(), limit: 50,
}, 250)?;
```

Requests (serde internally tagged by `method`):

```json
{"method":"search","query":"folder:photos","limit":50}
{"method":"sizes","path":"D:\\Coding","limit":50}
{"method":"status"}
```

`Response { hits: Vec<Hit>, entries: usize, scanning: bool, elapsed_us: u64,
error: Option<String> }`. `Hit { name: String, path: String, directory: bool,
size: u64, files: u32, modified: u32 }`. Search returns up to 100 results.
`entries` counts record slots, including tombstones until compaction; query hits and
folder sizes exclude deleted records. Fresh scan and compacted snapshot counts are live.
Sizes returns the requested path **first**, followed by up to `limit` immediate children
(files and directories), biggest first, capped at 1,000 children. Missing paths give no
hits. `scanning` describes initial reconciliation; cached data can be stale during it.

CLI:

```powershell
mypaper-index --query 'ext:rs index'
mypaper-index --sizes 'D:\Coding'
mypaper-index --status
mypaper-index --bench 1000000
mypaper-index --bench-query
mypaper-index --bench-scan 'D:\some-representative-tree'
```

`--root PATH [CACHE]` runs a standalone service for an isolated fixture. Core passes
`--parent PID` in production. Only one server may own the pipe at a time.

## Measurements (2026-11-06, Ryzen 7 7800X3D, Windows)

Release mode; checks/tests use `C:\mypaper-targets\index`; the deployment build uses
`C:\mypaper-targets\main`. Initial million-entry baseline missed the query gate and
was optimized before integration. Raw evidence is in `crates/index/evidence/`.

| Gate | Measured | Result |
|---|---:|---|
| Search, 1,000,000 files, first 50 results, <=10 ms | p50 **3.38 ms**, p95 **5.98 ms**, max **8.19 ms** (180 queries, 9 query/filter forms); first query after trim **7.10 ms** | Pass; post-trim pages remained in Windows' file cache |
| Snapshot restart, well under 1 second | **16.55 ms** for 1M; final **285.26 ms** for 11,419,226 record slots; observed full-index warm starts **174–572 ms**, one **1,099.61 ms** outlier during shared deployment work | Typical cached run passes; the outlier misses the one-second gate; cold storage/page-cache state varies |
| Private allocation, <=40 MiB/million entries | **2.14 MiB** search-only; **20.18 MiB** with a million-file directory lookup; **21.96 MiB** on the live 11.26M-entry service | Pass (steady state) |
| Record store | **40 bytes/record**, 66,000,148-byte synthetic snapshot including names | Read-only file mapping |
| Final C: scan, Basic, non-admin | **4,725,399 entries, 218.912 s**, 575 inaccessible directories | Sparse scratch, concurrent desktop work |
| Final D: scan, Basic, non-admin | **6,511,657 entries, 229.906 s**, 5 inaccessible directories | Sparse scratch, concurrent desktop work |
| Final E: scan, Basic, non-admin | **167,617 entries, 6.734 s**, 2 inaccessible directories | Sparse scratch, concurrent desktop work |
| Live pipe queries over 11.40M record slots | p50 **30.20 ms**, p95 **42.21 ms**, 140 requests including IPC | Million-entry gate is separate |
| First full-index query after a two-second quiet trim | **91.88–125.36 ms** server time, four query/filter forms | File pages remained in Windows' reclaimable cache; disk-cold queries may take longer |
| Live burst on the full index, 2,000 files / 256,000 bytes | **1,631.80 ms** through size convergence; **500 ms** index CPU | Pass |
| Move/resize/rename/delete of that fixture | **303.44 ms**, final **1,999 files / 256,768 bytes**, renamed-file query **97.51 ms** | Pass |
| Idle CPU over 60 s, isolated live watched tree | **0.00% of one core**, **4 MiB private** | Pass; no filesystem changes |
| Full-volume 60 s sample, no test queries, concurrent filesystem writes | **3.65% of one core**, **18 MiB private**, **10 MiB settled working set** | Active desktop, not a zero-change idle measurement |
| Final full reconciliation, 11.41M record slots | **77.34 MiB peak private**; **1,359 MiB sampled peak working set**; settled **17.31 MiB private / 7.82 MiB working set** | Uninterrupted 566 s observation; initial scans/replay finished by about 535 s; zero scratch files afterward |
| Full-volume CPU during concurrent builds, 180 s | **2.17% of one core**, **22 MiB private**, **757 MiB working set** | Active workload, not idle; before sparse scratch change |
| Flag lifecycle | Disable: **0 processes**, **4.11 s** from config write; enable cached-ready **3.24 s**, parent verified `mypaper-core` | Enabled in the user's config after validation; can be switched off |

The snapshot's mapped bytes are file-backed, not private allocation; the working set is
reported separately. The original private-buffer reconciliation peaked at **2.44 GiB**
on the full index, which prompted sparse scratch mappings and streamed replacement.
The sampled resident peak is not a bound. Before `74a0dd1`, the sampler observed
**1,994 MiB** and a spot observation reached **2,252 MiB** between ticks. The final build
closes scan views and trims old pages before validating the replacement; its sampled peak
was **1,359 MiB**. Active
queries over the full snapshot reached **859–878 MiB working set**, then returned to
**7–10 MiB** after the quiet deadline. Trimming makes those file pages reclaimable; it
does not eliminate the RAM and I/O needed by a later whole-index search.
The synthetic in-memory `--bench` construction peak is **88.44 MiB/million**; this CLI
fixture intentionally constructs an owned store and does not use production scan scratch.
The first full-volume watch implementation failed under concurrent builds: blocking queue
delivery overflowed notifications, forced C: rescans, and the 2,000-file fixture timed out.
Its 60-second sample (**85.73% of one core, 1,800 MiB private**) was **not idle** and fails
the gate. That result led to the coalesced handoff above; the corrected fixture passes.
Two changes removed most sustained-write overhead: coalescing watch delivery without
blocking kernel readers, and persisting bounded change overlays rather than rewriting the
whole snapshot after each quiet gap. The earlier full-cache checkpoint sample was
**28.26% of one core** during builds; the overlay sample was **2.17%** under continued
build/temp-file activity. These are workload observations, not controlled idle comparisons.

## Follow-ups

- Studio toggle in Experimental (Studio is owned by its redesign thread).
- Elevated MFT/USN path and recovery of offline changes without a background full reconciliation.
- Mounted volumes without drive letters, device hotplug and a user include/exclude UI.
- The Files window is a separate native process delivered by its own workstream. Its
  search/folder-size integration can consume this pipe; the index should remain independent.
- Scan scratch currently caps one volume at 64 million entries and 2 GiB name bytes;
  snapshots cap total name bytes at 4 GiB. No content search or full Unicode normalization.

## Validation and commits

`cargo test -p mypaper-index`: 10 passed. `cargo clippy -p mypaper-index --all-targets
-- -D warnings`: passed. Focused launcher tests: 5 passed, 1 installed-app timing test
intentionally ignored. `cargo check -p mypaper-config -p mypaper-core` and the release
workspace build passed. Existing warnings in other crates remain outside this slice.

An earlier live UI Automation capture returned the indexed demo folder (`launcher.json`).
The final guarded capture was skipped while T3 Code was foreground, so the latest launcher
binary's visual result is not certified by a final screenshot. The archived
`launcher-before-lane-fix.png` is failure evidence from before the dedicated lane and
superseded-query fixes, not a screenshot of the final result. No user application received
synthetic input during the skipped captures.

| Slice | Commits |
|---|---|
| Default-off config, workspace dependency, core supervision | `2fb402a`, `4d7121a`, `b686d0e` |
| Index/store/query/pipe initial implementation | `74325de` |
| Launcher integration, dedicated lane, stale-query cancellation | `1893252`, `f8e2b4d`, `807fb2a` |
| Coalesced live watches and incremental checkpoints | `0ddea07`, `086edab` |
| Snapshot validation and ranking correctness | `ec8463d`, `edb09a5` |
| Sparse scan scratch and streamed volume replacement; release scratch before replacement validation | `1d26ffc`, `74a0dd1` |
| Quiet resident-page trim and post-trim query measurement | `b29dc26` |
| Cold full-volume launcher deadline | `338c48b` |
| Lifecycle and measurement tools | `8108b3b`, `788c014`, `47787ef` |

Outside the brief's primary file ownership, `crates/shell/Cargo.toml` gained the index
library dependency needed by the launcher client. Shared Files routing and kit changes
belong to the other workstreams and were preserved. No push was performed.
