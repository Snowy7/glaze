# Native file watching

`mypaper-fswatch` watches Windows directories without periodic wakeups. Each watch owns one
256 KiB-stack thread, a 64 KiB notification buffer, an overlapped completion event, and a stop
event. It waits indefinitely while idle. Dropping it signals stop, cancels and drains pending
I/O, and joins the thread before releasing its callback or handles.

```rust,no_run
let watch = mypaper_fswatch::Watch::files(&config_dir, &["config.json"], move || {
    // Post to the caller's existing event loop.
    reload.send(()).ok();
})?;
```

Keep the returned `Watch` alive. `Watch::files` watches named files nonrecursively, using
ASCII case-insensitive names. Atomic saves, modifications, removals, and renames all count;
file reads do not. `Watch::directory(dir, recursive, callback)` delivers `Event::Paths` with
absolute changed paths, including both rename names. `Event::Rescan` means the kernel lost
names, returned malformed records, or recovered from an I/O failure; reload the watched state. File filters always forward
a rescan. Callbacks run on the watcher thread and should only post a message, never drop their
own watch or block on slow work.

The first read is armed before construction returns. Interesting changes start a 60 ms
debounce, with a 250 ms maximum latency under sustained writes. Duplicate paths coalesce;
the pending set is bounded at 8,192 names, then becomes a rescan. Unrelated files never arm
a debounce deadline. Shutdown takes priority over both notifications and debounce deadlines.
An I/O failure delivers a rescan, closes the old directory handle, and reopens/re-arms the watch.
Transient errors back off from 25 ms to a maximum of 1 s, interrupted by stop. A missing directory
waits on its nearest surviving ancestor's directory notifications, without polling for existence.
The ancestor read is armed before checking the target again, so recreations cannot be missed.
A rescan after re-arming covers any publications made during recovery. Drop also cancels recovery.

Consumers: shared desktop config (core, engine, styler), engine theme, tweaks palette/game activity,
shell theme/bars/Start Menu updates, and index config. The index's bulk root watcher retains its
own implementation. Studio is the only remaining `notify` consumer.

Run `cargo test -p mypaper-fswatch` for record parsing, filtering, bounded debouncing, real
kernel overflow/recovery, create/modify/rename/delete, atomic replacement, recursive watching,
and stop/join tests. `evidence/measure-wakes.ps1` samples per-thread raw context-switch counters
and records Windows thread descriptions; pair it with the shared `measure.ps1` CPU sampler.
`cargo run -p mypaper-fswatch --example mypaper-fswatch-idle` provides a 90-second isolated
watch with no file changes, then drops it and removes its empty temporary directory.

Live results and limitations from the initial migration are in `evidence/report.md`.
