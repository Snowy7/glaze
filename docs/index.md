# Native file index (experimental)

**Status:** the source includes follow-up `6ebad2c`, shipped in the later shared workspace
deployment recorded in the [night log](../plans/night/2026-11-06.md). The 10:54 build passed the
initial rebuild RAM gate but failed a broad game replay. Its correction passed an isolated replay;
post-repair production game certification remains open. See the independent-review evidence below.

`mypaper-index` is an optional native process, enabled with
`"experimental": { "file_index": true }` in `%APPDATA%\mypaper\config.json`.
The default is **false**. Core starts it when enabled, stops it on disable/quit/panic,
and supervises crashes with its existing backoff. A wait-only parent handle also makes
the helper exit if core disappears. Disabled means no index process, watch, thread or timer.
No code runs in Explorer or in a plugin.

## Deferred offline catch-up

Cached startup arms live watches **before** opening the snapshot. Search becomes available
after the usual snapshot validation/summary load; it does not wait for reconciliation.
Live changes apply during the startup delay. When a catch-up scan is active, its racing
notifications use the existing bounded queue and final-state replay.

`index.startup_rescan_after` is a number of seconds (default **21,600**, six hours).
`index.startup_rescan_delay` is also seconds (default **120**). `freshness.json` records
each volume's last completed full scan, snapshot generation, rules and direct-child directory
timestamps. Ordinary overlay saves/compactions do **not** refresh that age. Missing/corrupt
freshness, changed include/exclude rules, clock rollback, an older snapshot generation, and
missing roots all require catch-up. The first run after upgrading a legacy cache therefore
does one delayed, throttled verification. Freshness is saved only after scan completion,
notification replay and a durable checkpoint. Recovery failures do not certify freshness.

Recent volumes skip startup scanning. A one-shot deadline at their age limit eventually
verifies the offline interval even if this process stays running for days. Stale volumes
wait at least the startup delay. Deadlines stop throughout the existing game signal;
enumeration, generation writing, validation and replay also park at its work boundaries.
There is no keyboard/mouse hook or user-idle polling. Windows offers `GetLastInputInfo`
as a query, not an idle notification, so the delay and background CPU/I/O priority are the
fallback for ordinary foreground activity.

Catch-up uses **one** background worker, including replacement validation. A cooperative
thread CPU-time budget sleeps at work boundaries to target **25% of one logical core**
over time; it yields between directories. This is not a hard instantaneous process cap:
directory batches can burst, and live notifications/queries have their own work. Overflow,
returning-volume and explicit rule-change recovery retain their full-reconciliation paths.
`mypaper-index --rescan` requests an immediate full catch-up of every online volume through
the running service, bypassing freshness and delay while retaining the game gate and CPU
budget. `--status.scanning` describes active reconciliation, not a future deadline.

Before a stale-volume scan, a non-admin root enumeration compares direct-child directory
last-write timestamps with the saved markers. This detects many immediate child-list edits,
but **does not prove recursive cleanliness**: nested creation/deletion/rename need not touch
the top-level timestamp; in-place file writes/size changes need not touch even their parent;
timestamps have one-second precision here and can be restored. Therefore matching markers
do **not** justify skipping subtrees: this implementation logs changed top-level hints and
uses the conservative full-volume fallback at the throttled rate. A correct selective skip
needs a durable recursive change token such as journal replay; timestamp-only pruning would
silently leave stale results. Recent-volume skipping intentionally permits offline drift
until the age deadline or a manual rescan. Live changes after the watch starts are still caught.

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
- Full-volume and large-subtree scan scratch records/names use sparse temporary file mappings under
  `index/scan/`, instead of large private buffers. A four-byte ID map streams the fresh
  volume beside the other cached volumes into the next snapshot. Scan mappings are
  removed after use; recognizable leftovers from a killed service are removed only after
  exclusive pipe ownership. The service excludes its own cache tree from indexing.
- Scan and checkpoint views retain four resident 8 MiB windows per view. Dirty scan
  windows are flushed before range eviction; the snapshot writer uses a 1 MiB buffer
  and `FILE_FLAG_SEQUENTIAL_SCAN`. Checkpoints read an independent view so their window
  eviction does not target the live query view. Validation decodes UTF-8 in 4 MiB chunks
  and uses four workers sharing twelve resident windows (96 MiB), then releases them.
  Eviction follows the documented [VirtualUnlock behavior](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualunlock)
  for unlocked pages. Virtual addresses stay valid; physical pages become reclaimable.
  Round 2 adds a separate bounded query column (below). The 256 MiB measurement target applies to background
  scanning/replacement without concurrent full-index queries.
- Substring terms are ANDed. Exact name matches rank first, then name prefix, word
  prefix, substring; newer modification times break ties. Filters: `ext:rs,toml`,
  `path:"Program Files"`, `folder:photos`, `file:report`. Quotes work within filters.
- Up to eight search workers are created on the first large query, then park between
  requests. Each searches a shard with reusable substring finders and a bounded top-k
  heap. Parent-path filters are reused across adjacent siblings. Wide directories get
  lazy hash-to-record lookups for updates, with original-name verification on collisions.

### Round 2: compact query column and exclusions

New snapshots append a separate folded-name column, streamed with the generation rather
than built in a private full-index buffer. Each 32-name block stores a 1024-bit trigram
union plus IDs, parents, modification times, directory/excluded bits and folded UTF-8.
A query combines its name terms' trigrams, rejects impossible blocks, and checks every
surviving name exactly. One- and two-byte terms bypass the prefilter. The filter has false
positives but no false negatives; filters on paths are applied after name matching.
The block filters, file/directory summaries and hashed extension unions live in a compact
query directory. Queries inspect it before touching mapped names. Blocks with more than
512 bits set gain a 4096-bit query signature so long names cannot saturate the prefilter.
Directory trigram summaries, including cross-separator trigrams, reject impossible path filters before
reconstructing paths. Any changed mapped directory path disables that shortcut until the
next checkpoint. All survivors still use exact name and path checks.
Keeping filters interleaved only on disk was insufficient: a 5.84M-entry live run stayed
under the RAM target but needed p95 239.9 ms because every filter check still faulted name
pages. Extension summaries also avoid reconstructing paths for unrelated file types.
Search-relevant overlay records own their folded names; size/count-only changes continue
using the immutable query column. Live overlays are searched separately and suppress their original entries.
A bounded top-100 prefix cache handles initial keystrokes and common prefixes through six
UTF-8 bytes. A temporary 4 MiB count-min sketch selects common longer prefixes; it is
discarded after loading. The cache has at most 8,192 keys. Live overrides are merged, and
cached results are used only when their rank bounds prove no unseen row can beat them.
Otherwise the exact scan remains available. Ranking and Unicode folding are unchanged.
The final private-memory follow-up writes immutable block/signature arrays, directory
summaries and ranked-prefix rows to four ordinary files and maps them read-only. The
construction vectors are freed before the replacements are mapped, avoiding two resident
copies. Windows can reclaim those file pages without retaining the arrays as private
commit. Only the small prefix-key/range directory remains private. Files use delete-on-close
handles, so a killed helper leaves no auxiliary files behind. These are runtime summaries:
they are still rebuilt at startup rather than persisted as a versioned startup cache.
Each of eight search shards retains two 1 MiB name windows, avoiding cross-shard page
eviction. Record/path reads retain four 2 MiB windows and a 2 MiB path cache. Searches do
not read every 40-byte record on every query.
Existing snapshots remain readable and gain the column when reconciled/checkpointed.

Built-in exclusions prune `node_modules`, `.git`, `__pycache__`, `.venv`, `venv`, `.gradle`,
`$Recycle.Bin`, `System Volume Information`, drive-root `Windows/WinSxS` and
`Windows/Installer`, `AppData/Local/Temp`, `AppData/Local/**/Cache*`, `npm-cache`,
`pnpm-store`, `.cargo/registry`, `.rustup/toolchains`, and `C:/mypaper-targets`.
`target` is excluded only when it contains `CACHEDIR.TAG` or `.rustc_info.json`;
`bin` and `obj` only when their parent contains a `.csproj` file. Scans decide those
markers once per directory and pass the decision to their children; `.csproj` presence
comes from the parent's existing enumeration. Notification batches reuse a bounded
directory-decision cache. A plain folder called
`target` or `bin` remains searchable. Reparse traversal remains disabled.

User rules layer onto these defaults in `%APPDATA%/mypaper/config.json`:

```json
{
  "index": {
    "exclude": ["D:/private/**", "**/*.tmp"],
    "include": ["D:/project/node_modules/my-package/**"]
  }
}
```

Globs are case-insensitive and component-aware; use `/` separators (`\\` also works).
`*` stays within a component; `**` crosses directories. A matched path excludes/includes
its whole subtree. Includes override both built-in and user exclusions. The scanner
retains ancestors needed to reach included descendants. The service's own cache is
always excluded. Atomic config changes reload valid rules and reconcile the watched
roots; invalid globs are logged and the last valid rules remain in use.

Excluded subtrees are **not walked just to calculate their size**. They remain as one
aggregate with `excluded: true`, `size_unknown: true`, and zero known bytes. Ancestors
inherit `size_unknown`; their byte/file totals are lower bounds, never exact totals.
Inaccessible directories are marked the same way. Excluded entries do not appear in
search, but the sizes API lists their aggregates. Removing the last unknown subtree
restores a known parent size. The launcher displays “Size unknown” for incomplete sizes.
Notification readers discard changes below static exclusions while preserving boundary
create/delete events; Rust/.NET marker changes re-evaluate the affected subtree.

The helper also consumes `mypaper_tweaks::game_activity::Snapshot`: scan workers,
generation streams and cache validators block at work boundaries while `pause_background`
is true. Native notification readers keep draining and rearming their 2 MiB buffers during
the game. They decode names into a bounded set of dirty **parent directories**, without
rule/store lookups, metadata I/O or store writes. Delivery and the service coalesce those
directories; resume scans only those subtrees, pruning descendants of an already-replayed
parent. A directory move dirties both its old and new parent. Queries remain available.
This does perform event-driven CPU work during background disk writes; a quiet watch
still blocks indefinitely. It does not promise zero CPU during a synthetic write burst.
A genuine native-buffer overflow, or exceeding 8,192 distinct dirty directories, requests
reconciliation of the affected volume(s). A dirty directory that is itself a drive root
also requires a root replay. Huge/distributed bursts can therefore still cause full scans.

On the pause transition, the path cache is freed and a one-shot working-set trim pages
out mapped data and query summaries after workers park. The path-cache allocator may
retain freed capacity; resident RAM and private commit are reported separately.
Reconciliation/checkpoint/trim deadlines use a clock stopped for the entire pause. A
directory notification wakes the game reader, and the checked publisher process handle
joins its blocking wait set; missing, corrupt, disabled or dead publication resumes work.
Index scheduling adds no polling timer. Initial-scan replay retains at most 128,000 exact
paths; paused replay retains at most 8,192 dirty directories, falling back to affected
volume reconciliation instead of retaining an unbounded list.

Search is **names and paths only**, not document contents. Sizes count every accessible
file's logical length, including each directory entry of a hard link. They do not
measure allocated/compressed disk space. Protected directories may be inaccessible;
reparse points (junctions, symlinks and cloud placeholders marked reparse) are omitted.

## Enumeration and updates

Fixed NTFS/ReFS drive-letter volumes are discovered at startup and on `WM_DEVICECHANGE`
arrival/removal/device-tree broadcasts, using a hidden top-level window and no polling.
A failed watch stops only its root, immediately hides its search and Sizes hits, and keeps
other roots available. One root overlay record marks it offline; the next streamed
snapshot drops it without allocating a tombstone for every descendant. Missing cached
roots are hidden before the pipe opens. On a later device broadcast, available roots
without a healthy watch are rearmed and reconciled. A scan finishing concurrently with a
failure reapplies offline state when swapping its store, so it cannot revive stale hits.
Drive-letter aliases exercise this path in the live fixture; physical USB/VHD/BitLocker
hardware is not required for that test and is not claimed as separately verified.

Directory entries are enumerated without elevation, through
`FindFirstFileExW(FindExInfoBasic)` and `FindNextFileW`,
with four background-priority directory workers. No file contents are opened.
`THREAD_MODE_BACKGROUND_BEGIN` lowers CPU and I/O priority for enumeration and
maintenance workers; an unsuccessful priority change is logged. Existing bounded
game checkpoints also call `yield_now` after releasing their gate lock, so runnable
foreground work gets a scheduling opportunity without adding a periodic timer.
`--bench-scan PATH` compares `FIND_FIRST_EX_LARGE_FETCH` with basic fetch in alternating
order. On this machine the best warm registry-tree sample was **0.471 s Basic** versus
**0.565 s Large** for 136,889 entries; Basic is selected. Concurrent disk/build activity
made wall times variable, so both modes remain available for remeasurement.

A recursive, overlapped `ReadDirectoryChangesExW` watch with a 2 MiB aligned buffer is
armed on each root before scanning. Its reader blocks indefinitely on completion/stop
events; cancellation is drained before the watch storage is freed. ReFS uses `ReadDirectoryChangesW` when the extended
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

A cached snapshot answers queries after cached-load validation. A background reconciliation
still runs once because changes made while the service was off cannot be recovered
reliably without a journal. Buffered notifications are replayed after scanning. An
event-driven two-second quiet deadline (or two minutes during sustained writes)
checkpoints changes; there are no checkpoint wakeups when clean.

Scratch mappings close after each volume is checkpointed. Reconciliation completion and
queries arm a one-shot two-second quiet deadline; `EmptyWorkingSet` then returns resident
pages to Windows' reclaimable cache. Status requests do not arm it. There is no recurring
trim timer. A new query faults the needed pages back in, so warm-query latency and the
first query after a trim must be measured separately. Round 1's whole-record searches touched
much of the mapped snapshot while active; low private allocation is not low resident RAM.

**TODO:** a separately authorized elevated MFT bootstrap / USN replay path. Neither
admin access nor journal creation is attempted by this experiment. Faster offline-gap
recovery and narrower overflow repair depend on that future work.

## Pipe API

`\\.\pipe\mypaper-index` is a local, message-mode named pipe with an explicit current-user
SID DACL and remote clients rejected. First-instance creation rejects an existing pipe.
Each transaction is one UTF-8 JSON request message and one UTF-8 JSON response message,
without a newline or length prefix. Requests are limited to 8 KiB and replies to 1 MiB.
Four listeners serve concurrent clients. I/O has cancellation deadlines. Each transaction
is wrapped in `catch_unwind`; an unwinding build disconnects after a panic and accepts another
client. The shipped abort build exits on a panic and is restarted by core, as described below.
Sizes walks folded path components, so Unicode lowercase changes in UTF-8 byte length
(for example the Kelvin sign) never slice the original string at a folded byte offset.

**Panics:** the workspace builds with `panic = "abort"`, and the index ships with the normal
workspace build (no special build step). Under abort, `catch_unwind` doesn't contain a
transaction panic: the helper exits and core restarts it from its cached snapshot, which is
the intended fallback for a bug we haven't seen. Known panic sources (the folded-offset slice)
are fixed at the source. `crates/index/tools/build-release.ps1` still builds an unwinding
index for experiments, and the injected-panic regression runs with
`cargo test --release -p mypaper-index` (Windows' Cranelift debug backend cannot unwind).

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
size: u64, files: u32, modified: u32, size_unknown: bool, excluded: bool }`.
The two booleans default to false when deserializing older replies. Search returns up to 100 results.
`entries` counts record slots, including tombstones until compaction; query hits and
folder sizes exclude deleted records and offline roots. Fresh scan and compacted snapshot counts are live.
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

## Independent-review repairs (2026-11-06, 10:54 deployment)

The repair series is `6abcf67` (orphan/Unicode paths), `f42ac5a` (directory marker
contexts), `3a70176` and `816d1cc` (offline roots, device events, paused-directory replay
and shadowed cached roots), `3428d22` (listener containment/unwinding release), and
`f6d0899` (infrequent lifecycle/recovery log). Only `crates/index` and this document
were changed. `service.rs` already had a formatting-only edit, which was retained.

**Deployment record at the end of this review:** `6ebad2c` adds bounded large-subtree replay, a bounded offline
visibility cache and two diagnostic CLI benchmarks after live stress exposed additional
failures. It was verified in the private target after that turn's single 10:54 deployment,
so the measurements below still describe the older helper's broad-subtree memory failure.
The later shared deployment included the correction, using the normal workspace build and
`deploy-local.ps1`; it needs no unwinding override. A new production resume/rebuild observation
is still needed to certify the corrected live path.

| Finding | Repair and focused regression |
|---|---|
| Failed/vanished volumes exit or retain stale hits | One offline-root overlay record hides descendants; streamed checkpoints prune that root. Device broadcasts re-enumerate/rearm roots. Native watch deletion leaves a second watch delivering; startup missing/shadowed roots and racing scan swaps stay hidden; cached-prefix/ordinary searches, Sizes, overlay reload and next-generation pruning are covered. |
| Marker below an unindexed parent imports a phantom root | `merge` returns when neither an old root nor its parent exists. Real marker reconciliation below `node_modules/project/target` preserves the original root chain and record count. |
| Folded Unicode byte offsets panic in Sizes and lose a listener | Walk folded components. Kelvin-sign drive/component Sizes returns correct bytes. A release-test transaction panic drops its read guard and the same listener path accepts the next transaction (unwinding builds only; the shipped abort build restarts the helper instead, see "Panics"). |
| Per-entry marker filesystem probes | Scans pass inherited marker decisions through the directory queue, taking `.csproj` presence from enumeration. Notification batches cache directory decisions. A regression removes marker files after resolving the directory and verifies 2,000 children reuse the decision; a fresh batch observes removal. Existing include/unknown-size scan coverage remains. |
| Game buffer can overflow | Native reads continue during pause; only bounded parent-directory sets are retained. A real paused watch delivers a dirty directory before resume and cancels cleanly while paused; 200,000 same-directory paths remain one directory, and 8,193 directories request bounded overflow recovery. The offline 20,000-file regression also verifies unchanged paused Sizes and exact resumed search/bytes/counts after one subtree replay. |

Validation at that review: **32 debug tests**, **33 LLVM release tests**, and index Clippy on all
targets with warnings denied. The review also tried an index-only unwinding release; this is an
experimental test configuration, not a deployment step. The injected-panic test
is release-only because this workspace's Windows Cranelift debug backend cannot unwind.

The same E: volume was measured before and after with the alternating
Basic/Large/Large/Basic `--bench-scan` sequence:

| Sample | Before (364,157 entries) | After (364,160 entries) |
|---|---:|---:|
| Basic, first sample | 25.432 s | 21.644 s |
| Large, second | 15.334 s | 3.058 s |
| Large, third | 12.130 s | 1.888 s |
| Basic, final warm sample | 12.918 s | 1.909 s |

Both runs had zero inaccessible directories. Three new entries and concurrent agent/build
activity mean this is a live before/after observation, not an isolated causal speedup
estimate. Evidence: `review-scan-before.jsonl` and `review-scan-after.jsonl`.

Before repair, a real fullscreen signal remained true throughout a **30.36-second**
paused sample while the fixture created **20,000 files** (2,560,000 logical bytes).
Index CPU/context switches remained zero because its readers were parked. Immediately
after resume, fresh scan scratch named **C:** appeared, although initial reconciliation
had completed before the fixture. This establishes the pre-fix volume fallback after
the native buffer lost names. Another agent deployed during catch-up and replaced that
index process, so no baseline catch-up latency is certified. The enlarged fixture's
search probe was also corrected to query a unique filename instead of the shared suffix.
Evidence: `review-game-before*.json`, PNGs and the interrupted-run log. The fullscreen
and restored captures were inspected; the fixture covered the primary (right) display.

The single deployment completed at **10:54 local** via `scripts/deploy-local.ps1`, from
`C:/mypaper-targets/main/release`. The core-owned helper, PID **78592**, logged
`start; panic=unwind`; its deployed SHA-256 matched the build evidence:
`4E70DA4E6E80E8A529C7FDEF63BA98830E5CB4BEBF271D8CF940FCDD37BC513F`.
It completed all C:/D:/E: generations and buffered replay in **142.95 s**, without
restarting. Its OS lifetime working-set peak during that rebuild was **137.00 MiB**,
including spikes between the one-second samples, below the 256 MiB target. Private
high-water was **141.93 MiB**. Subsequent live query/volume fixtures raised the same
process's lifetime working-set peak to **199.85 MiB**. A later broad resume raised it
again to **724.15 MiB**; the overall live RAM goal consequently **failed**, despite the
initial rebuild passing. The peak was not reset or hidden.

| Final production enumeration | Entries | Seconds | Inaccessible directories |
|---|---:|---:|---:|
| C: | 2,533,074 | 40.526 | 566 |
| D: | 3,090,806 | 61.016 | 2 |
| E: | 364,194 | 4.623 | 0 |

The live 2,000-file fixture converged in **1,010.40 ms** (812.50 ms index CPU).
Move/resize/rename/delete converged in **22.48 ms**; final Sizes returned **1,999 files /
256,768 bytes**, and renamed-file search returned the correct path. Evidence:
`review-final-live.json`.

Two temporary fixed-drive aliases exercised discovery and failure/recovery without
mounting, unlocking or changing a physical device. The K: fixture took **9.60 s** to
import its initial generation and **23.26 s** to replace it after return. Deleting its
owned backing directory produced a real native watch failure (or a racing removal
broadcast), hid K: search/Sizes in **80.65 ms**, and left C: answering in the same process.
A recreated K: returned the new **456-byte** file instead of its old **123-byte** record.
Removing the alias and delivering its volume-removal notification hid it in **1.79 ms**.
The earlier Z: test's log explicitly recorded `watch Z: offline: Access is denied` and
hid it in **65.98 ms**. These are alias/device-message tests, not physical USB/VHD or
BitLocker certification. Evidence: `review-final-volumes.json`,
`review-final-volumes-z.json` and `review-final-service.log`.

On that live K: alias, **20 crafted Sizes requests using `?:\`** returned correct
sizes. Four transactions held open concurrently then received valid responses, proving
all **four listeners** still served clients. No process restart occurred.

The user resumed using the machine before the final game check. The foreground guard
found **T3 Code (Nightly)** and skipped fullscreen launch/input; see
`review-game-after-skipped.json`. No post-repair production fullscreen-game latency,
CPU or wake-rate result is claimed. Instead, the release regression used the real native
watcher and the same Quiet/store/reconcile code in an isolated fixture. While its gate
was paused, **20,000 file creations / 2,560,000 bytes** took **6,832.87 ms**. Two dirty
directories (including a directory last-write event's parent) coalesced to **one subtree
replay in 67.55 ms**, with **zero root reconciliations**. Sizes stayed at zero files while
paused, then search and Sizes returned all 20,000 files and exactly 2,560,000 bytes.
This is an offline pause/resume proof, not a live foreground-game result. Evidence:
`review-paused-offline.json` and `.log`. The fixture lives under the excluded private
Cargo target and cleans up its own tree. The final native burst regression and foreground
safeguards were added after deployment. The later bounded-replay/visibility correction
below was added afterward; its isolated proof does not replace production certification.

### Late stress failures and bounded-replay correction (verified offline)

A larger **30,000-file** background fixture overlapped an independently triggered real
game pause after the user resumed using the machine. We did not launch a fullscreen
fixture or take their foreground. The fixture timed out waiting for Sizes after 60 s
and cleaned up; it is **not** a successful 30,000-file convergence result.
On resume, the deployed recovery log included broad parents such as
`C:/Users/you/AppData/Local`. The then-unbounded in-memory subtree scan/import raised
PID 78592's OS lifetime working-set high-water to **759,324,672 bytes / 724.15 MiB**.
The main 256 MiB goal fails for this deployed broad-replay case. Evidence is the final
lifetime record, the failed-fixture record and `review-final-service.log`.

`6ebad2c` now abandons a small subtree's private scan above **25,000 records or 8 MiB
of record/name capacity**, then re-enumerates that subtree into sparse scan mappings.
It streams the new subtree beside unchanged records in a new generation, using the
same bounded writer/validator as volume replacement. It preserves the basename,
parent/sibling links, ancestor bytes/counts and unknown-size state, with old query views
remaining readable throughout. It does not enumerate other volumes. The 26,000-file
regression verifies this becomes a mapped generation with zero retained overlay records;
a separate streamed-subtree test covers replacements, new siblings, unknown-size
propagation/clearing, unchanged volumes, old readers and orphan rejection.

A read-only private process then loaded the full production snapshot and reconciled
that same **AppData/Local** subtree into its own scratch cache: **474,248 enumerated
records in 9.427 s**, **24.61 s** total through generation replacement/validation.
Its OS working-set peak was **140,189,696 bytes / 133.70 MiB**, private high-water
**127.55 MiB**, and final private commit **8.06 MiB**. Sizes returned 439,179 known files
and `size_unknown: true` for excluded aggregates. The live helper, user files and
production snapshot were not modified by this isolated replay. Scratch was removed.
This verifies the corrected broad-subtree path under 256 MiB **offline**, and does not
certify a post-repair production game replay. Evidence:
`review-bounded-replay.json`.

The live broad-query sample taken while an offline root was retained had **p95
1,031.47 ms**, p50 4.52 ms, across 220 full-pipe requests. That is a material latency
regression; search correctness alone did not reveal it. `6ebad2c` checks volume visibility
only after top-k rejection, reuses adjacent siblings' status and retains at most 8,192
cached directory visibility decisions, invalidated when roots/parents change.
A private full-snapshot benchmark with E: marked offline then measured **p95 21.54 ms**,
p50 0.247 ms (220 direct queries); the preceding uncached follow-up measured p95
80.70 ms. These are direct-query samples with a different offline root from the live
K: fixture, so they are not a controlled IPC speedup comparison. The inherited 20 ms
latency goal still narrowly misses. After the deployed helper settled/compacted, a
separate 55-request pipe run measured p95 **22.61 ms**, p50 0.703 ms, with the same
process and its full 724.15 MiB lifetime high-water retained in the evidence.

Diagnostic commands (read-only input snapshot, isolated output scratch):

```powershell
C:/mypaper-targets/index/release/mypaper-index.exe --bench-offline-query SNAPSHOT 'E:\'
C:/mypaper-targets/index/release/mypaper-index.exe --bench-replay SNAPSHOT $env:LOCALAPPDATA C:/mypaper-targets/index/measurements
```

Evidence: `review-final-search.json`, `review-offline-query-followup*.json`,
`review-final-settled-search.json`. Timings vary with the concurrent desktop workload.
No post-repair foreground-game CPU/wake-rate result or all-scenario RAM guarantee is
claimed. Per-directory enumeration still materializes a directory's entries, so an
arbitrarily enormous single directory is a remaining separate bound.

## Final Round 2 result (2026-11-06, 09:38 build)

The final deployed index combines file-backed query summaries (`262ec03`) with exact
notification replay after games (`7062002`). One core-owned process, PID 12136, completed
all C:/D:/E: generations, notification replay, queries, a real foreground fullscreen game
fixture, and post-game catch-up without restarting. **Its OS lifetime working-set peak
remained 136.40 MiB after all of those actions**, including the resume test.

| Round 2 budget | Before (Round 1) | Final measured result |
|---|---:|---:|
| Active query working set <=150 MiB | 859–878 MiB | **65.53 MiB**, sampled every 10 ms; lifetime OS peak also below 150 MiB |
| Warm full-pipe p95 <=20 ms | 42.21 ms | **4.87 ms**; p50 0.55 ms, max 6.22 ms, 1,100 requests |
| First query after quiet trim <=40 ms | 91.88–125.36 ms server time | **0.60–5.59 ms full-pipe**, eleven forms |
| Generation working-set peak <=256 MiB | 141.91 MiB bounded follow-up | **136.40 MiB**, complete uninterrupted three-volume run and later resume |

The remaining index contained **5,984,220 record slots**, versus about 11.4 million
before exclusions. Reconciliation/checkpoints/replay took **226.39 s**, followed by a
30-second settling observation. Private high-water was **140.89 MiB**; settled private
memory was **16.82 MiB**, with **7.65 MiB working set**. The preceding private-summary
design retained 108.65 MiB private after settling; the mapped arrays remove that cost.
These are live machine results with differing cache/load conditions, not controlled
estimates of the speed gain from each change.

Recorded process CPU during the active startup/reconciliation interval was **71.88%
of one core** (162.61 CPU seconds over 226.21 elapsed seconds), including enumeration,
generation replacement and notification replay. Across the full 257.25-second sample,
including settling, it averaged **63.47% of one core**. The highest approximately
one-second interval was **175.97% of one core** across the four workers. This is active
background work, not idle CPU; the game sample below measured 0.000%. Evidence is
`round2-replay-scan-cpu.json`, derived from the original per-second counters. Explicit
checkpoint yields and checked background-mode entry were added after that run, so these
CPU numbers are not a measurement of a CPU reduction from the scheduling hardening.

| Final non-admin enumeration | Entries | Seconds | Inaccessible directories |
|---|---:|---:|---:|
| C: | 2,530,021 | 77.132 | 566 |
| D: | 3,089,648 | 86.546 | 2 |
| E: | 364,157 | 6.913 | 0 |

The owned borderless topmost fixture covered the primary monitor. Win32 foreground PID
and core's real hint both identified that fixture; all 60 sampled hints stayed true.
After warming 22 pipe requests, working set fell from **64.47 MiB** to at most
**1.55 MiB** during the **60.88-second** sample. Private commit was at most **20.18 MiB**,
and CPU was **0.000% of one core**. The pause-edge path-cache release and working-set
trim page out the mapped summaries; private allocation and resident RAM are distinct.
The sample begins after a 12-second activation/settling interval, so it does not establish
an exact trim-latency bound.

One **32 KiB file created while paused** became searchable with exactly one file and
32,768 known bytes **152.61 ms after the fixture ended**. Retaining exact raw paths was
essential. The original dirty-root fallback took **207.77 s**. An intermediate
parent-directory sweep took 9.69 s but pushed the OS lifetime peak to **259.98 MiB**:
directory metadata notifications can identify very broad ancestors. That prototype fails
the budget and was replaced, not shipped as the final behavior. Its failure evidence is
`round2-backed-resume-highwater.json` and `round2-parent-sweep-game-catchup.json`.

**Wake cost in the 08:20 build:** 18.66 context switches/s came entirely from two
`notify-rs windows loop` threads (572 and 564 switches). Every index main/search/native
reader/delivery thread recorded zero switches in that game sample. The installed
`notify` 8.2.1 Windows backend unconditionally waits with
`WaitForSingleObjectEx(wakeup_sem, 100, TRUE)`; its Windows constructor ignores the
poll-interval config. This misses the handful-of-wakes target even though index work is
parked. Those shared config/game watches now use `mypaper_fswatch::Watch`, blocking indefinitely
on overlapped I/O while idle; `31b51d2` consolidated the desktop wrapper onto the same watcher.
These historical samples do not measure that later migration.

Other final checks:

- A 2,000-file / 256,000-byte burst converged in **1,035.16 ms**, using 1,046.88 ms index
  CPU. Move/resize/rename/delete converged in **19.11 ms**; the final 1,999 files and
  256,768 bytes were correct. Renamed-file query server time was 3.42 ms.
- The earlier mapped-build live exclusion fixture passed boundary, unknown-size,
  Rust/.NET marker and deleted-project checks. Exact replay uses the same tested
  final-state update path. The earlier isolated 60-second idle sample remained
  0.00% CPU; disable/on supervision was verified and the flag restored true.
- The final million-entry query-code repeat measured **p50 5.13 / p95 12.12 ms**, max
  14.37 ms, first after trim 7.72 ms. Load was 529.88 ms; private memory 2.23 MiB,
  or 20.27 MiB with the wide-directory lookup. **The inherited <=10 ms gate still
  misses**, specifically the repeated nonmatching path case.
- Full cached startup was **4,058.58 ms**, so **the inherited subsecond goal still
  misses**. Runtime summaries are rebuilt; persisting a validated summary cache is
  follow-up work. An isolated mapped load used only 4.70 MiB private on return.
- An earlier mapped warm run measured p95 23.66 ms and the synthetic run 55.56 ms
  amid concurrent Studio fixture activity. Those results are retained alongside the
  settled repeats and the final clean full-pipe run. They are not silently discarded.
- Guarded launcher capture skipped input while Windows Settings was foreground;
  full-pipe evidence is used as the brief permits. The fullscreen and restored desktop
  PNGs were inspected. No foreign window was moved, minimized, or typed into.

Final verification: **20 index tests**, **21 config tests**, index Clippy on all targets
with warnings denied, and the workspace release build passed. The helper remains enabled
and core-owned. No push was performed. Primary commits for this round are `1c63748`
(layered config), `db7efcc` (exclusions/query column), `997640e` (honest launcher sizes),
`2625876` and `0141cd2` (bounded filters/ranked caches/game parking), `262ec03`
(file-backed summaries), and `7062002` (exact resume replay). Measurement tools are
committed separately. Config fields and defaulted Hit booleans are additive API changes;
the fswatch owner separately changed the config-watcher dependency and snippet.

Final binary SHA-256:
`F8D929EE525BC5A45CF5B6525D6F7C799E3E8455838526140C315F34146D60DC`.
Final evidence is `round2-replay-{binary,reconcile,warm,cold,live,game,game-catchup,
lifetime}.json` and `round2-replay-production.log`; synthetic repeat evidence is
`round2-backed-million-repeat.json`. Earlier iterations remain under `round2-*`.

## First complete Round 2 measurements (2026-11-06, 08:20 build)

The final workspace release was deployed at 08:20 local and measured without another
deployment through all C:/D:/E: scans, generation replacements and notification replay.
The cache contained **6,005,411 record slots**, versus about 11.4 million before default
exclusions. The machine continued ordinary filesystem writes; this is a live desktop
measurement, not a controlled storage benchmark.

| Measurement | Round 1 | Round 2 final |
|---|---:|---:|
| Active query working set, target <=150 MiB | 859–878 MiB | **71.76 MiB** sampled every 10 ms |
| Full-pipe warm latency | p50 30.20 / p95 42.21 ms | **p50 0.62 / p95 4.98 ms**, maximum 7.19 ms |
| First query after quiet working-set trim, target <=40 ms | 91.88–125.36 ms server time | **0.80–6.06 ms full-pipe**, 11 query/filter forms |
| Background generation OS working-set high-water, target <=256 MiB | 141.91 MiB bounded implementation | **189.21 MiB**, complete uninterrupted three-volume run |
| Background generation private high-water | 84.22 MiB bounded run | **195.58 MiB** |
| Settled private / working set after reconciliation | 17.31 / 7.82 MiB | **108.65 / 10.06 MiB** |
| Reconciliation including checkpoints/replay | About 535 s in an earlier complete run | **333.70 s**, observed for 364.79 s including settling |
| Cached startup | Usually 174–572 ms, later 1.6–3.2 s outliers | **5,327.55 ms** live; **2,546.55 ms** separate cached load |
| Million-entry search fixture, 180 queries | p50 3.49 / p95 6.19 ms | **p50 6.23 / p95 14.26 ms**, max 14.99 ms; first after trim 7.00 ms |
| Million-entry cached load | 16.55 ms before the query column | **444.05 ms** |
| Million-entry private memory | 2.14 MiB search / 20.18 MiB with wide lookup | **7.00 / 25.04 MiB**, within 40 MiB/million |
| 2,000-file burst, 256,000 bytes | 1,631.80 ms / 500 ms index CPU | **1,116.75 ms / 718.75 ms CPU** |
| Move/resize/rename/delete convergence | 303.44 ms | **142.67 ms**; final 1,999 files / 256,768 bytes |
| Renamed-file query, server time | 97.51 ms | **1.43 ms** |
| Truly quiet watched tree, 60 seconds | 0.00% of one core / 4 MiB private | **0.00% / 4 MiB private / 1 MiB working set** |
| Flag lifecycle | Off: 0 processes in 4.11 s; on: ready in 3.24 s | **Off: 0 processes in 2.84 s; on: cached-ready in 5.29 s**, core parent verified |

Warm evidence is 100 rounds of eleven forms (1,100 requests), including initial `c`,
`ca`, `ra`, `e` keystrokes, ordinary names, extension, directory and path filters, and
an absent term. Each cold form waits 2.5 seconds for the existing quiet trim. These are
resident-cold queries with Windows' file cache available, not a disk-cold boot. The
query sample's process-lifetime OS peak includes the preceding generation work; the
71.76 MiB figure is the query interval's sampled residency. The main Round 2 query and
generation targets pass. The **old million-entry <=10 ms gate regressed** on a repeated
nonmatching path filter (14.99 ms), and **full-index startup misses the old subsecond
goal**. Retaining adaptive filters and ranked prefixes raises private commit and the
generation peak. Neither is hidden by the small settled working set.

Final non-admin enumeration used Basic fetch:

| Volume | Entries enumerated | Wall time | Inaccessible directories |
|---|---:|---:|---:|
| C: | 2,529,097 | 100.509 s | 566 |
| D: | 3,089,298 | 174.205 s | 2 |
| E: | 384,844 | 8.018 s | 0 |

Live exclusion checks verified that excluded names are absent, unknown sizes propagate
and clear, Rust/.NET markers prune and restore their build trees, and deleting a marked
project/build directory leaves no ghost unknown aggregate. The 2,000-file fixture also
verified rename, resize, move and deletion against the live full index.

The idle check temporarily disabled the production flag and ran the deployed binary
against an isolated one-file watched tree, then verified the core-owned helper's
off/on lifecycle and restored `experimental.file_index: true`. It does not claim the
busy full-volume desktop has no filesystem work. Evidence: `round2-idle.json` and
`round2-lifecycle.json`.

An alternating fetch comparison on the small 1,467–1,468-entry source tree measured
Basic **5.69–6.20 ms** versus Large **6.81–10.51 ms** (`round2-fetch.jsonl`). Output files
changed the entry count by one between samples. This is a small warm-tree check, not
evidence of a universal API speed advantage; Basic remains selected. A broader optional
comparison was stopped to release the shared live window and is explicitly marked
incomplete in `round2-fetch-desktop-aborted.json`.

Final guarded launcher capture skipped input because Windows Settings
(`ApplicationFrameHost`) was foreground (`round2-launcher-capture.txt`). The final
launcher rendering is not visually certified; full-pipe evidence is used as the brief
permits. No foreign window was moved, minimized, or typed into.

The game reader fix had already measured 0.000% CPU in the separate game-quiet run,
but retained 119 MiB resident RAM before the pause-edge trim was added. The first local
trim proof was rejected: the owned topmost primary-monitor fullscreen fixture was
verified foreground by Win32, while core's published hint remained stale and false.
`round2-game-stale-publisher-activation.json` and its fullscreen/restored screenshots preserve that
publisher failure; they are not a successful game performance sample. A real publisher
signal is required before certifying the new game RAM behavior.

Final binary SHA-256:
`AB1B89C98F64105548DF9B74A39EB6CAC3B04B5A617147D21E46A5DB7BC16B06`.
Raw final evidence is `round2-final-{binary,reconcile,warm,cold,million,offline-load,
exclusions,live}.json` and `round2-final-production.log` under `crates/index/evidence/`.
Earlier `round2-*` runs are retained as diagnosis evidence; the initial interleaved-only
filter and first resident-filter designs missed the latency target.

Validation: **19 index tests** and **21 config tests** passed; index Clippy on all
targets with warnings denied and the workspace release build passed. Relevant commits:
`1c63748` (layered config), `db7efcc` (query column, exclusions and game gate), `997640e`
(launcher unknown-size label), `2625876` (resident filters), `0141cd2` (bounded ranked
prefix/path caches, adaptive summaries, reader parking and game trim), and `af2480b`
(owned foreground fixture verification). No core, Studio or shared-kit change was made
by this round; config fields and the two defaulted Hit booleans are additive API changes.

## Resident-window follow-up (2026-11-06, 05:40–06:20 timebox)

The first implementation's generation replacement briefly reached 2,252 MiB resident RAM.
Releasing scan views earlier reduced a later sampled peak to 1,359 MiB, still too high.
The windowed implementation (`801d661`, `48a78e7`, `bc4f8c5`) instead trims maintenance
inputs as it goes and writes the output sequentially. Normal query views are unchanged.

| Measurement | Windowed result |
|---|---:|
| OS working-set high-water, target <=256 MiB | **141.91 MiB** |
| Private high-water in the same run | **84.22 MiB** |
| Work observed | **582.32 s**: startup, complete C: enumeration, complete 11.425M-record / 912,417,170-byte generation rewrite and validation, then partial D: enumeration |
| C: enumeration | **4,740,391 entries / 380.691 s**, 574 inaccessible directories |
| C: generation checkpoint | Approximately **80 s** |
| Validated cached startup in that run | **386.587 ms**, about **101.7 MiB** peak working set |
| Standalone 11.42M-entry load | **362.509 ms**, **101.60 MiB** peak working set, **6.08 MiB** on return |
| One million entries, 180 queries | p50 **3.488 ms**, p95 **6.190 ms**, max **7.158 ms**, first after trim **6.987 ms** |

The OS high-water counter catches spikes between the one-second samples. The complete
generation rewrite and validation met the 256 MiB target. **The three-volume run did not
finish:** another deployment restarted core/index during D: enumeration at about 06:11.
The sampler preserves this as `interrupted: true`, `completed: false`; its last residency
was an active scan, not a settled-idle measurement. Evidence is `windowed-reconcile.json`,
`windowed-generation-proof.json`, and `windowed-production.log` under `crates/index/evidence`.

The time cost matters: this C: scan took 381 s versus 219 s in an earlier run, followed
by about 80 s to checkpoint. Concurrent machine load varied, so that difference is not
a controlled estimate of windowing overhead. The later external deployment's cached
load took **3.212 s**, missing the one-second startup goal despite the measured run's
387 ms load. The earlier single-worker windowed prototypes also took 1.6–1.8 s; four
validators sharing a 96 MiB resident budget restored the fast warm load.

To certify the whole startup sequence, leave one index/core instance uninterrupted for
roughly **20–25 minutes** on this machine, record `PeakWorkingSet64` through all three
volume swaps and notification replay, then take the settled sample. No further code is
needed to repeat that measurement. A strict cap for arbitrary directory fanout or huge
event bursts would additionally require batching directory enumeration (currently a
per-directory `Vec<Entry>`) and bounding/spilling the initial notification backlog.
Full-index queries still touch much of the snapshot; this 256 MiB result covers background
generation, not a hard process limit during active searches.

Eleven index tests, Clippy with warnings denied, and the final workspace release build
passed. The deployed helper remains core-owned and enabled. Shared UI/Studio compile
failures delayed deployment during this timebox; those files were left to their owners.

## Earlier measurements (2026-11-06, Ryzen 7 7800X3D, Windows)

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
- Mounted volumes without drive letters and a user include/exclude UI.
- The Files window is a separate native process delivered by its own workstream. Its
  search/folder-size integration can consume this pipe; the index should remain independent.
- Scan scratch currently caps one volume at 64 million entries and 2 GiB name bytes;
  snapshots cap total name bytes at 4 GiB. No content search or full Unicode normalization.

## Validation and commits

`cargo test -p mypaper-index`: 11 passed. `cargo clippy -p mypaper-index --all-targets
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
| Bounded scan/checkpoint windows and startup validation | `801d661`, `48a78e7`, `bc4f8c5` |
| Cold full-volume launcher deadline | `338c48b` |
| Lifecycle and measurement tools | `8108b3b`, `788c014`, `47787ef` |

Outside the brief's primary file ownership, `crates/shell/Cargo.toml` gained the index
library dependency needed by the launcher client. Shared Files routing and kit changes
belong to the other workstreams and were preserved. No push was performed.
