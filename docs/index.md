# Native file index (experimental)

`mypaper-index` is an optional native process, enabled with
`"experimental": { "file_index": true }` in `%APPDATA%\mypaper\config.json`.
The default is **false**. Core starts it when enabled, stops it on disable/quit/panic,
and supervises crashes with its existing backoff. A wait-only parent handle also makes
the helper exit if core disappears. Disabled means no index process, watch, thread or timer.
No code runs in Explorer or in a plugin.

With `launcher.files` enabled in `bars.json`, typing in the launcher queries the helper
on its existing worker lane. Files retain the helper's ranking; folder results show
their logical subtree size and file count. If the pipe is unavailable or times out,
the existing recent-document search remains available. Apps and Settings keep their
existing behavior. Empty-query recommendations still use recent documents.

## Store and search

- Forty-byte fixed records: parent/child/sibling IDs, original/folded name offsets,
  attributes, modified time (Unix seconds), logical size and subtree file count.
- One UTF-8 name blob; lowercase names share the original bytes when possible.
  Unicode lowercase is locale-independent, without accent removal or normalization.
- `%LOCALAPPDATA%\mypaper\index\snapshot.bin` is a read-only file mapping. Small
  live changes use a record/name overlay. A checkpoint compacts deleted records and
  superseded names into a temporary file, syncs it, atomically replaces the snapshot,
  validates a new mapping, then swaps it into the service. Queries can run during I/O.
- Substring terms are ANDed. Exact name matches rank first, then name prefix, word
  prefix, substring; newer modification times break ties. Filters: `ext:rs,toml`,
  `path:"Program Files"`, `folder:photos`, `file:report`. Quotes work within filters.
- Up to eight search workers are created on the first large query, then park between
  requests. Each searches a shard with reusable substring finders and a bounded top-k
  heap. Parent paths are cached per query rather than allocated once per file.

Search is **names and paths only**, not document contents. Sizes count every accessible
file's logical length, including each directory entry of a hard link. They do not
measure allocated/compressed disk space. Protected directories may be inaccessible;
reparse points (junctions, symlinks and cloud placeholders marked reparse) are omitted.

## Enumeration and updates

Fixed NTFS/ReFS drive-letter volumes are enumerated without elevation, through
`FindFirstFileExW(FindExInfoBasic, FIND_FIRST_EX_LARGE_FETCH)` and `FindNextFileW`,
with four background-priority directory workers. No file contents are opened.
`--bench-scan PATH` compares large fetch with basic fetch in alternating order.

A recursive, blocking `ReadDirectoryChangesExW` watch with a 1 MiB aligned buffer is
armed on each root before scanning. ReFS uses `ReadDirectoryChangesW` when the extended
API reports unsupported. Changes are deduplicated and applied as final filesystem
state; adds, resizes, removals and directory moves update ancestor sizes/counts.
New/moved-in directories are scanned as subtrees. There is no polling timer.

If a notification buffer overflows, Windows has discarded the affected names. The
affected **watched subtree** is the volume root, so that volume is reconciled; other
volumes are left alone. The non-admin API cannot safely identify a smaller lost subtree.
This is a limitation relative to precise journal replay.

A cached snapshot answers queries immediately after restart. A background reconciliation
still runs once because changes made while the service was off cannot be recovered
reliably without a journal. Buffered notifications are replayed after scanning. An
event-driven two-second quiet deadline (or two minutes during sustained writes)
checkpoints changes; there are no checkpoint wakeups when clean.

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
}, 100)?;

let sizes = pipe::request(&Request::Sizes {
    path: r"D:\Coding".into(), limit: 50,
}, 100)?;
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
| Search, 1,000,000 files, first 50 results, <=10 ms | p50 **3.21 ms**, p95 **8.46 ms**, max **8.73 ms** (180 queries, 9 query/filter forms) | Pass |
| Snapshot restart, well under 1 second | **35.65 ms** (1,000,000 files, validated warm mapping) | Pass |
| Record store | **40 bytes/record**, 66,000,148-byte synthetic snapshot including names | File-backed; private-memory measurement pending |
| Full scan per fixed volume | In progress | Pending |
| Idle CPU over 60 s | Not yet sampled | Pending |
| Burst change cost, live search/sizes, launcher | Verification in progress | Pending |

The snapshot's mapped bytes are file-backed, not private allocation. Full scans temporarily
use owned buffers and can exceed the steady-state private-memory budget; both costs must
be reported separately. Do not enable the user's flag until live measurements are complete.

## Follow-ups

- Studio toggle in Experimental (Studio is owned by its redesign thread).
- Elevated MFT/USN path and recovery of offline changes without a background full reconciliation.
- Mounted volumes without drive letters, device hotplug and a user include/exclude UI.
- A Files window is a separate native process; it should consume this pipe rather than
  moving the index into its UI process.
