//! Process lifecycle, event-driven reconciliation/checkpointing, and measurement CLI.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::{
    Request, pipe, query,
    scan::{self, Background, Fetch},
    store::{DIRECTORY, NONE, REPARSE, Store},
    watch::{self, Change},
};

pub struct State {
    pub store: Store,
    pub scanning: bool,
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?)
            .join("mypaper/index"),
    )
}

/// Applies the final state after a burst. A remove/rename event is just a missing old path;
/// metadata I/O happens outside the store write lock. Access-denied never erases an entry.
fn apply(state: &RwLock<State>, path: &Path) -> Result<()> {
    let text = path.to_string_lossy();
    let metadata = scan::metadata(path);
    match metadata {
        Ok(entry) if entry.attributes & REPARSE == 0 => {
            let Some(parent) = path.parent() else {
                return Ok(());
            };
            let parent_text = parent.to_string_lossy();
            let existing = {
                let mut state = state.write().expect("index state");
                let Some(parent_id) = state.store.find(&parent_text) else {
                    return Ok(());
                };
                let existing = state.store.find(&text);
                state.store.upsert(
                    parent_id,
                    &entry.name,
                    entry.attributes,
                    entry.size,
                    entry.modified,
                );
                existing
            };
            if entry.attributes & DIRECTORY != 0 && existing.is_none() {
                reconcile(state, path)?;
            }
        }
        Ok(_) => {
            let mut state = state.write().expect("index state");
            if let Some(id) = state.store.find(&text) {
                state.store.remove(id);
            }
        }
        Err(error) => {
            let code = error
                .downcast_ref::<windows::core::Error>()
                .map(|e| e.code());
            if code.is_some_and(|c| {
                c == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND.to_hresult()
                    || c == windows::Win32::Foundation::ERROR_PATH_NOT_FOUND.to_hresult()
            }) {
                let mut state = state.write().expect("index state");
                if let Some(id) = state.store.find(&text) {
                    state.store.remove(id);
                }
            }
        }
    }
    Ok(())
}

fn reconcile(state: &RwLock<State>, root: &Path) -> Result<()> {
    let scanned = scan::scan(root, Fetch::Large)?;
    eprintln!(
        "scan {}: {} entries, {:.3}s, {} inaccessible directories",
        root.display(),
        scanned.store.len() - 1,
        scanned.seconds,
        scanned.denied
    );
    let mut state = state.write().expect("index state");
    let text = root.to_string_lossy();
    let old = state.store.find(&text);
    let parent = old
        .map(|id| state.store.record(id).parent)
        .or_else(|| {
            root.parent()
                .and_then(|p| state.store.find(&p.to_string_lossy()))
        })
        .unwrap_or(0);
    if let Some(id) = old {
        state.store.remove(id);
    }
    if scanned.store.record(0).first != NONE {
        let source = scanned.store.record(0).first;
        // A scan names its root with the whole path; an in-tree import needs its basename.
        if parent == 0 {
            state.store.import(&scanned.store, source, parent);
        } else {
            let name = root
                .file_name()
                .context("subtree basename")?
                .to_string_lossy();
            let id = state.store.upsert(parent, &name, DIRECTORY, 0, 0);
            let mut child = scanned.store.record(source).first;
            while child != NONE {
                state.store.import(&scanned.store, child, id);
                child = scanned.store.record(child).next;
            }
        }
    }
    Ok(())
}

fn checkpoint(state: &RwLock<State>, path: &Path) -> Result<()> {
    // Only this event-loop thread mutates the store. Let pipe readers continue throughout
    // disk I/O and validation, taking the exclusive lock only for the mapping swap.
    state.read().expect("index state").store.save(path)?;
    let mapped = Store::open(path)?;
    state.write().expect("index state").store = mapped;
    Ok(())
}

pub fn run(roots: Vec<PathBuf>, cache: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&cache)?;
    let snapshot = cache.join("snapshot.bin");
    let started = Instant::now();
    let store = Store::open(&snapshot).unwrap_or_else(|error| {
        eprintln!("no usable snapshot: {error:#}");
        Store::new()
    });
    eprintln!(
        "loaded {} entries in {:.3}ms",
        store.len() - 1,
        started.elapsed().as_secs_f64() * 1000.0
    );
    let state = Arc::new(RwLock::new(State {
        store,
        scanning: true,
    }));
    pipe::start(state.clone())?;
    let (tx, rx) = mpsc::sync_channel(256);
    for root in &roots {
        watch::start(root.clone(), tx.clone())
            .with_context(|| format!("watch {}", root.display()))?;
    }
    // Watches are armed before reconciliation so scan races are replayed afterwards. A
    // non-admin snapshot cannot know what changed while the service was off: reconcile once
    // in the background while cached queries are already available (USN replay is future work).
    let _background = Background::enter();
    for root in roots {
        if let Err(error) = reconcile(&state, &root) {
            eprintln!("scan {}: {error:#}", root.display());
        }
    }
    state.write().expect("index state").scanning = false;
    // Replay buffered events after importing scans, never before a scan can overwrite them.
    let mut dirty = Some(Instant::now());
    loop {
        let message = if dirty.is_some() {
            rx.recv_timeout(Duration::from_secs(2))
        } else {
            rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        match message {
            Ok(first) => {
                let mut paths = BTreeSet::new();
                let mut overflows = BTreeSet::new();
                for change in std::iter::once(first).chain(rx.try_iter().take(255)) {
                    match change {
                        Change::Paths {
                            root: _,
                            paths: changed,
                        } => {
                            paths.extend(changed.into_iter().filter(|p| !p.starts_with(&cache)));
                        }
                        Change::Overflow(root) => {
                            overflows.insert(root);
                        }
                    }
                }
                if !paths.is_empty() || !overflows.is_empty() {
                    dirty.get_or_insert_with(Instant::now);
                }
                for root in &overflows {
                    reconcile(&state, root)?;
                }
                // Parent paths sort before their children. New directories import complete
                // subtrees; later events are idempotent final-state updates.
                for path in paths {
                    if !overflows.iter().any(|r| path.starts_with(r)) {
                        apply(&state, &path)?;
                    }
                }
                if dirty.is_some_and(|t| t.elapsed() >= Duration::from_secs(120)) {
                    checkpoint(&state, &snapshot)?;
                    dirty = None;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                checkpoint(&state, &snapshot)?;
                dirty = None;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

pub fn cli(args: Vec<String>) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("--query" | "--sizes" | "--status") => {
            let request = match args[0].as_str() {
                "--query" => Request::Search {
                    query: args[1..].join(" "),
                    limit: 50,
                },
                "--sizes" => Request::Sizes {
                    path: args.get(1).context("--sizes PATH")?.clone(),
                    limit: 50,
                },
                _ => Request::Status,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&pipe::request(&request, 5000)?)?
            );
            Ok(())
        }
        Some("--bench") => benchmark(
            args.get(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(1_000_000),
        ),
        Some("--bench-query") => {
            let mut times = Vec::new();
            let queries = [
                "cargo",
                "readme",
                "ext:rs",
                "folder:src",
                "wallpaper",
                "path:projects ext:toml",
                "unlikely-no-match-8932",
            ];
            for _ in 0..20 {
                for query in queries {
                    let started = Instant::now();
                    let answer = pipe::request(
                        &Request::Search {
                            query: query.into(),
                            limit: 50,
                        },
                        5000,
                    )?;
                    times.push((
                        started.elapsed().as_secs_f64() * 1000.0,
                        answer.elapsed_us as f64 / 1000.0,
                    ));
                }
            }
            times.sort_by(|a, b| a.0.total_cmp(&b.0));
            println!(
                "{}",
                serde_json::json!({"queries":times.len(), "p50_ms":times[times.len()/2].0, "p95_ms":times[times.len()*95/100].0, "p50_server_ms":times[times.len()/2].1, "p95_server_ms":times[times.len()*95/100].1})
            );
            Ok(())
        }
        Some("--bench-scan") => {
            let root = PathBuf::from(args.get(1).context("--bench-scan PATH")?);
            for fetch in [Fetch::Basic, Fetch::Large, Fetch::Large, Fetch::Basic] {
                let scanned = scan::scan(&root, fetch)?;
                println!(
                    "{}",
                    serde_json::json!({"root":root,"fetch":format!("{fetch:?}"),"seconds":scanned.seconds,"entries":scanned.store.len()-1,"denied":scanned.denied})
                );
            }
            Ok(())
        }
        Some("--root") => run(
            vec![PathBuf::from(args.get(1).context("--root PATH [CACHE]")?)],
            args.get(2).map(PathBuf::from).unwrap_or(cache_dir()?),
        ),
        Some("--parent") => {
            let pid: u32 = args.get(1).context("--parent PID")?.parse()?;
            use windows::Win32::System::Threading::*;
            // SAFETY: wait-only handle; the service exits if its supervising core disappears.
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid)? };
            let raw = handle.0 as usize;
            std::thread::Builder::new()
                .name("index-parent".into())
                .spawn(move || {
                    // SAFETY: owned live process handle, closed immediately after its wait.
                    unsafe {
                        WaitForSingleObject(
                            windows::Win32::Foundation::HANDLE(raw as *mut _),
                            INFINITE,
                        );
                        let _ = windows::Win32::Foundation::CloseHandle(
                            windows::Win32::Foundation::HANDLE(raw as *mut _),
                        );
                    }
                    std::process::exit(0);
                })?;
            run(scan::volumes(), cache_dir()?)
        }
        None => run(scan::volumes(), cache_dir()?),
        _ => bail!(
            "mypaper-index [--query QUERY | --sizes PATH | --status | --bench [COUNT] | --bench-query | --bench-scan PATH | --root PATH [CACHE]]"
        ),
    }
}

fn benchmark(count: usize) -> Result<()> {
    let path = std::env::temp_dir().join(format!("mypaper-index-bench-{}.bin", std::process::id()));
    let mut store = Store::new();
    let root = store.insert(0, "X:", DIRECTORY, 0, 0);
    for i in 0..count {
        store.insert(
            root,
            &format!("project-{i:07}-report.txt"),
            128,
            i as u64,
            i as u32,
        );
    }
    store.roll_up();
    store.save(&path)?;
    drop(store);
    let started = Instant::now();
    let store = Store::open(&path)?;
    let load_ms = started.elapsed().as_secs_f64() * 1000.0;
    let queries = [
        "report",
        "000123",
        "missing",
        "project-0000500",
        "ext:txt report",
        "file:report",
        "folder:report",
        "path:X: report",
        "path:nonexistent report",
    ];
    let mut elapsed = Vec::new();
    let mut by_query = serde_json::Map::new();
    for query in queries {
        let mut samples = Vec::new();
        for _ in 0..20 {
            let at = Instant::now();
            std::hint::black_box(query::search(&store, query, 50));
            samples.push(at.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        by_query.insert(
            query.into(),
            serde_json::json!({"p50_ms":samples[10],"p95_ms":samples[19]}),
        );
        elapsed.extend(samples);
    }
    elapsed.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"entries":count,"record_bytes":size_of::<crate::store::Record>(),"snapshot_bytes":std::fs::metadata(&path)?.len(),"load_ms":load_ms,"queries":elapsed.len(),"p50_ms":elapsed[elapsed.len()/2],"p95_ms":elapsed[elapsed.len()*95/100],"max_ms":elapsed.last(),"by_query":by_query})
    );
    drop(store);
    std::fs::remove_file(&path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_final_state_changes_keep_subtree_sizes_and_moves_current() {
        let dir =
            std::env::temp_dir().join(format!("mypaper-index-changes-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::write(dir.join("a/test.txt"), b"123").unwrap();
        let scanned = scan::scan(&dir, Fetch::Large).unwrap();
        let state = RwLock::new(State {
            store: scanned.store,
            scanning: false,
        });
        std::fs::write(dir.join("a/test.txt"), b"1234567").unwrap();
        assert_eq!(scan::metadata(&dir.join("a/test.txt")).unwrap().size, 7);
        apply(&state, &dir.join("a/test.txt")).unwrap();
        assert_eq!(state.read().unwrap().store.record(1).size, 7);
        std::fs::rename(dir.join("a"), dir.join("b")).unwrap();
        apply(&state, &dir.join("a")).unwrap();
        apply(&state, &dir.join("b")).unwrap();
        assert_eq!(state.read().unwrap().store.record(1).size, 7);
        assert!(
            state
                .read()
                .unwrap()
                .store
                .find(&dir.join("b/test.txt").to_string_lossy())
                .is_some()
        );
        std::fs::remove_file(dir.join("b/test.txt")).unwrap();
        apply(&state, &dir.join("b/test.txt")).unwrap();
        assert_eq!(state.read().unwrap().store.record(1).files, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
