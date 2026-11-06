//! Process lifecycle, event-driven reconciliation/checkpointing, and measurement CLI.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::{
    Request, pipe, query,
    rules::{Decision, Rules},
    scan::{self, Background, Fetch},
    store::{DIRECTORY, NONE, REPARSE, Store},
    watch::{self, Change},
};

const BENCH_QUERIES: [&str; 11] = [
    "cargo",
    "readme",
    "ext:rs",
    "folder:src",
    "wallpaper",
    "path:projects ext:toml",
    "unlikely-no-match-8932",
    "c",
    "ca",
    "ra",
    "e",
];

pub struct State {
    pub store: Store,
    pub scanning: bool,
    cache: PathBuf,
    activity: Option<mpsc::SyncSender<Change>>,
    rules: Arc<RwLock<Rules>>,
    quiet: Arc<crate::quiet::Quiet>,
}

impl State {
    pub(crate) fn queried(&self) {
        if let Some(activity) = &self.activity {
            let _ = activity.try_send(Change::Queried);
        }
    }
}

pub(crate) fn trim_working_set() {
    // Reclaim validated/scanned file pages when the service settles. The mappings remain
    // valid; future queries fault their pages back from Windows' reclaimable file cache.
    // SAFETY: only trims this optional process, after its query quiet deadline.
    if let Err(error) = unsafe {
        windows::Win32::System::ProcessStatus::EmptyWorkingSet(
            windows::Win32::System::Threading::GetCurrentProcess(),
        )
    } {
        eprintln!("trim working set: {error}");
    }
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
    state.read().expect("index state").quiet.wait();
    let rules = state
        .read()
        .expect("index state")
        .rules
        .read()
        .expect("index rules")
        .clone();
    let text = path.to_string_lossy();
    let metadata = scan::metadata(path);
    match metadata {
        Ok(entry) if entry.attributes & REPARSE == 0 => {
            let decision = rules.decide(path, entry.attributes & DIRECTORY != 0);
            if decision != Decision::Index {
                if decision == Decision::Traverse {
                    return reconcile(state, path, false);
                }
                let mut state = state.write().expect("index state");
                if let Some(id) = state.store.find(&text) {
                    state.store.remove(id);
                }
                if let Some(parent) = path
                    .parent()
                    .and_then(|p| state.store.find(&p.to_string_lossy()))
                {
                    state.store.upsert(
                        parent,
                        &entry.name,
                        entry.attributes | crate::store::EXCLUDED | crate::store::SIZE_UNKNOWN,
                        0,
                        entry.modified,
                    );
                    state.store.refresh_unknown(parent);
                }
                return Ok(());
            }
            let Some(parent) = path.parent() else {
                return Ok(());
            };
            let parent_text = parent.to_string_lossy();
            let shared = state;
            let existing = {
                let mut state = state.write().expect("index state");
                let Some(parent_id) = state.store.find(&parent_text) else {
                    return Ok(());
                };
                let existing = state.store.find(&text);
                if existing.is_some_and(|id| {
                    state.store.record(id).attributes & crate::store::EXCLUDED != 0
                }) && entry.attributes & DIRECTORY != 0
                {
                    drop(state);
                    return reconcile(shared, path, false);
                }
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
                reconcile(state, path, false)?;
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

fn reconcile(state: &RwLock<State>, root: &Path, parallel: bool) -> Result<()> {
    let quiet = state.read().expect("index state").quiet.clone();
    quiet.wait();
    if !parallel
        && !scan::metadata(root)
            .is_ok_and(|entry| entry.attributes & (DIRECTORY | REPARSE) == DIRECTORY)
    {
        return apply(state, root);
    }
    // Measured warm trees on this machine favor Basic over LARGE_FETCH; the CLI retains both
    // so the choice can be remeasured on another disk or Windows version.
    let rules = state
        .read()
        .expect("index state")
        .rules
        .read()
        .expect("index rules")
        .clone();
    let scanned = if parallel {
        let cache = state.read().expect("index state").cache.clone();
        scan::scan_cached(root, Fetch::Basic, &cache, &rules, &quiet)?
    } else {
        scan::subtree(root, Fetch::Basic, &rules, &quiet)?
    };
    if parallel {
        eprintln!(
            "scan {}: {} entries, {:.3}s, {} inaccessible directories",
            root.display(),
            scanned.store.len() - 1,
            scanned.seconds,
            scanned.denied
        );
    }
    if parallel {
        let snapshot = state
            .read()
            .expect("index state")
            .cache
            .join("snapshot.bin");
        state
            .read()
            .expect("index state")
            .store
            .save_replacing_volume(&snapshot, &scanned.store, root)?;
        // The scan is now durable. Close its views before touching the new full snapshot,
        // and release old query pages to avoid retaining both generations in resident RAM.
        drop(scanned);
        trim_working_set();
        let replacement = Store::open_quiet(&snapshot, quiet.clone())?;
        let old = {
            let mut state = state.write().expect("index state");
            std::mem::replace(&mut state.store, replacement)
        };
        drop(old);
        let _ = std::fs::remove_file(snapshot.with_file_name("changes.json"));
        return Ok(());
    }
    let mut state = state.write().expect("index state");
    if state.store.is_empty() {
        // First volume: move its buffers instead of allocating a duplicate full index.
        state.store = scanned.store;
        return Ok(());
    }
    merge(&mut state.store, &scanned.store, root)
}

fn merge(store: &mut Store, source_store: &Store, root: &Path) -> Result<()> {
    let text = root.to_string_lossy();
    let old = store.find(&text);
    let Some(parent) = old
        .map(|id| store.record(id).parent)
        .or_else(|| root.parent().and_then(|p| store.find(&p.to_string_lossy())))
    else {
        // Marker notifications can originate below a pruned ancestor. They cannot create
        // a new top-level root; only volume reconciliation is allowed to do that.
        return Ok(());
    };
    if let Some(id) = old {
        store.remove(id);
    }
    if source_store.record(0).first != NONE {
        let source = source_store.record(0).first;
        // A scan names its root with the whole path; an in-tree import needs its basename.
        if parent == 0 {
            store.import(source_store, source, parent);
        } else {
            let name = root
                .file_name()
                .context("subtree basename")?
                .to_string_lossy();
            let root_info = source_store.record(source);
            let id = store.upsert(parent, &name, root_info.attributes, 0, root_info.modified);
            let mut child = source_store.record(source).first;
            while child != NONE {
                store.import(source_store, child, id);
                child = source_store.record(child).next;
            }
        }
    }
    store.refresh_unknown(parent);
    Ok(())
}

fn checkpoint(state: &RwLock<State>, path: &Path) -> Result<()> {
    // Only this event-loop thread mutates the store. Let pipe readers continue throughout
    // disk I/O and validation, taking the exclusive lock only for the mapping swap.
    {
        let state = state.read().expect("index state");
        state.store.save_quiet(path, state.quiet.clone())?;
    }
    let quiet = state.read().expect("index state").quiet.clone();
    let mapped = Store::open_quiet(path, quiet)?;
    let old = {
        let mut state = state.write().expect("index state");
        std::mem::replace(&mut state.store, mapped)
    };
    drop(old);
    // If a crash left an older overlay behind, its generation also prevents replay.
    let _ = std::fs::remove_file(path.with_file_name("changes.json"));
    Ok(())
}

fn checkpoint_changes(state: &RwLock<State>, path: &Path) -> Result<()> {
    if state.read().expect("index state").store.should_compact() {
        return checkpoint(state, path);
    }
    state
        .read()
        .expect("index state")
        .store
        .save_changes(&path.with_file_name("changes.json"))
}

pub fn run(roots: Vec<PathBuf>, cache: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&cache)?;
    let snapshot = cache.join("snapshot.bin");
    let (tx, rx) = mpsc::sync_channel(256);
    let quiet = crate::quiet::Quiet::start(tx.clone())?;
    let started = Instant::now();
    let mut store = Store::open_quiet(&snapshot, quiet.clone()).unwrap_or_else(|error| {
        eprintln!("no usable snapshot: {error:#}");
        Store::new()
    });
    let changes = cache.join("changes.json");
    store.set_quiet(quiet.clone());
    if changes.exists()
        && let Err(error) = store.load_changes(&changes)
    {
        eprintln!("ignoring unusable change checkpoint: {error:#}");
    }
    quiet.wait();
    store.clear_quiet();
    eprintln!(
        "loaded {} entries in {:.3}ms",
        store.len() - 1,
        started.elapsed().as_secs_f64() * 1000.0
    );
    let config = mypaper_config::Config::load()?;
    let rules = Arc::new(RwLock::new(Rules::new(&config.index, &cache)?));
    let state = Arc::new(RwLock::new(State {
        store,
        scanning: true,
        cache: cache.clone(),
        activity: Some(tx.clone()),
        rules: rules.clone(),
        quiet: quiet.clone(),
    }));
    let weak = Arc::downgrade(&state);
    quiet.on_pause(move || {
        if let Some(state) = weak.upgrade()
            && let Ok(state) = state.try_read()
        {
            state.store.release_query_paths();
        }
    });
    pipe::start(state.clone())?;
    crate::spool::cleanup(&cache.join("scan"));
    for root in &roots {
        watch::start(root.clone(), tx.clone(), rules.clone(), quiet.clone())
            .with_context(|| format!("watch {}", root.display()))?;
    }
    // Watches are armed before reconciliation so scan races are replayed afterwards. A
    // non-admin snapshot cannot know what changed while the service was off: reconcile once
    // in the background while cached queries are already available (USN replay is future work).
    let initial = state.clone();
    let scan_roots = roots.clone();
    let config_tx = tx.clone();
    let _config_watch =
        mypaper_fswatch::Watch::files(&mypaper_config::dir()?, &["config.json"], move || {
            let _ = config_tx.try_send(Change::RulesChanged);
        })?;
    std::thread::Builder::new()
        .name("index-initial".into())
        .spawn(move || {
            let _background = Background::enter();
            for root in scan_roots {
                if let Err(error) = reconcile(&initial, &root, true) {
                    eprintln!("scan {}: {error:#}", root.display());
                }
            }
            let _ = tx.send(Change::InitialDone);
        })?;
    let _background = Background::enter();
    let mut initializing = true;
    let mut pending_paths = BTreeSet::new();
    let mut pending_overflows = BTreeSet::new();
    let mut dirty = None::<Duration>;
    let mut trim_at = None::<Duration>;
    let mut current_config = config.index;
    loop {
        let trim_wait = trim_at.map(|at| at.saturating_sub(quiet.now()));
        let wait = if quiet.paused() {
            None
        } else if dirty.is_some() {
            Some(trim_wait.map_or(Duration::from_secs(2), |t| t.min(Duration::from_secs(2))))
        } else {
            trim_wait
        };
        let message = if let Some(wait) = wait {
            rx.recv_timeout(wait)
        } else {
            rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        match message {
            Ok(first) => {
                let mut initial_finished = false;
                let mut paths = BTreeSet::new();
                let mut overflows = BTreeSet::new();
                for change in std::iter::once(first).chain(rx.try_iter().take(255)) {
                    match change {
                        Change::Activity => {}
                        Change::RulesChanged => {
                            if let Ok(config) = mypaper_config::Config::load()
                                && config.index != current_config
                            {
                                match Rules::new(&config.index, &cache) {
                                    Ok(next) => {
                                        *rules.write().expect("index rules") = next;
                                        current_config = config.index;
                                        overflows.extend(roots.iter().cloned());
                                    }
                                    Err(error) => eprintln!("invalid index rules: {error:#}"),
                                }
                            }
                        }
                        Change::InitialDone => {
                            initializing = false;
                            initial_finished = true;
                            dirty = Some(quiet.now());
                            trim_at = Some(quiet.now() + Duration::from_secs(2));
                        }
                        Change::Queried => {
                            if !initializing {
                                trim_at = Some(quiet.now() + Duration::from_secs(2));
                            }
                        }
                        Change::Paths {
                            root: _,
                            paths: changed,
                        } => {
                            paths.extend(changed.into_iter().filter(|p| !p.starts_with(&cache)));
                        }
                        Change::Overflow(root) => {
                            overflows.insert(root);
                        }
                        Change::Failed { root, error } => {
                            bail!("watch {} failed: {error}", root.display())
                        }
                    }
                }
                // Keep draining watch completions during initial scans. Applying them before
                // import would race the scan; blocking the reader would overflow the kernel
                // buffer. A deduplicated set retains only the final state of each changed path.
                if initializing || quiet.paused() {
                    pending_paths.extend(paths);
                    pending_overflows.extend(overflows);
                    if pending_paths.len() > 128_000 {
                        pending_paths.clear();
                        pending_overflows.extend(roots.iter().cloned());
                    }
                    continue;
                }
                paths.append(&mut pending_paths);
                overflows.append(&mut pending_overflows);
                if !paths.is_empty() || !overflows.is_empty() {
                    dirty.get_or_insert_with(|| quiet.now());
                }
                for root in &overflows {
                    reconcile(&state, root, true)?;
                    trim_at = Some(quiet.now() + Duration::from_secs(2));
                }
                // Parent paths sort before their children. New directories import complete
                // subtrees; later events are idempotent final-state updates.
                for path in paths {
                    if !overflows.iter().any(|r| path.starts_with(r)) {
                        if let Some(root) = Rules::marker_root(&path) {
                            reconcile(&state, &root, false)?;
                        } else {
                            apply(&state, &path)?;
                        }
                    }
                }
                if dirty.is_some_and(|t| quiet.now().saturating_sub(t) >= Duration::from_secs(120))
                {
                    checkpoint_changes(&state, &snapshot)?;
                    dirty = None;
                    trim_at = Some(quiet.now() + Duration::from_secs(2));
                }
                if initial_finished || state.read().expect("index state").scanning {
                    state.write().expect("index state").scanning = false;
                    eprintln!("initial reconciliation and notification replay complete");
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if dirty.take().is_some() {
                    checkpoint_changes(&state, &snapshot)?;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if !initializing && !quiet.paused() && trim_at.is_some_and(|at| quiet.now() >= at) {
            trim_working_set();
            trim_at = None;
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
        Some("--bench-load") => {
            let path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or(cache_dir()?.join("snapshot.bin"));
            let at = Instant::now();
            let store = Store::open(&path)?;
            let load_ms = at.elapsed().as_secs_f64() * 1000.0;
            let mut memory =
                windows::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS_EX::default();
            memory.cb = size_of_val(&memory) as u32;
            // SAFETY: current-process counters, correctly sized output structure.
            unsafe {
                windows::Win32::System::ProcessStatus::GetProcessMemoryInfo(
                    windows::Win32::System::Threading::GetCurrentProcess(),
                    (&mut memory
                        as *mut windows::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS_EX)
                        .cast(),
                    size_of_val(&memory) as u32,
                )?;
            }
            println!(
                "{}",
                serde_json::json!({"entries":store.len()-1,"load_ms":load_ms,"snapshot_bytes":std::fs::metadata(path)?.len(),"peak_working_set_bytes":memory.PeakWorkingSetSize,"working_set_bytes":memory.WorkingSetSize,"private_bytes":memory.PrivateUsage})
            );
            Ok(())
        }
        Some("--bench-query") => {
            let mut times = Vec::new();
            let cold = args.iter().any(|arg| arg == "--cold");
            let queries = BENCH_QUERIES;
            let rounds = if cold {
                1
            } else {
                args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20)
            };
            if !cold {
                for query in queries {
                    pipe::request(
                        &Request::Search {
                            query: query.into(),
                            limit: 50,
                        },
                        5000,
                    )?;
                }
            }
            let mut samples = Vec::new();
            for _ in 0..rounds {
                for query in queries {
                    if cold {
                        std::thread::sleep(Duration::from_millis(2500));
                    }
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
                    samples.push(serde_json::json!({"query":query,"pipe_ms":times.last().expect("sample").0,"server_ms":answer.elapsed_us as f64 / 1000.0,"hits":answer.hits.len()}));
                }
            }
            times.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut server: Vec<_> = times.iter().map(|sample| sample.1).collect();
            server.sort_by(f64::total_cmp);
            println!(
                "{}",
                serde_json::json!({"cold_after_quiet_trim":cold,"queries":times.len(), "p50_ms":times[times.len()/2].0, "p95_ms":times[times.len()*95/100].0, "max_ms":times.last().expect("samples").0,"p50_server_ms":server[server.len()/2],"p95_server_ms":server[server.len()*95/100],"samples":samples})
            );
            Ok(())
        }
        Some("--bench-snapshot") => {
            let path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or(cache_dir()?.join("snapshot.bin"));
            let mut store = Store::open(&path)?;
            let mut segments = Vec::new();
            for overlay in [false, true] {
                if overlay
                    && let Err(error) = store.load_changes(&path.with_file_name("changes.json"))
                {
                    segments.push(serde_json::json!({"overlay":true,"error":format!("{error:#}")}));
                    break;
                }
                let mut rows = Vec::new();
                for text in BENCH_QUERIES {
                    let parsed = query::Query::parse(text);
                    let filter = crate::names::Filter::new(
                        &parsed.terms,
                        &parsed.extensions,
                        parsed.directory,
                    );
                    let profile = Instant::now();
                    let (blocks, bytes) = store.candidate_blocks(&filter);
                    let filter_ms = profile.elapsed().as_secs_f64() * 1000.0;
                    let mut times = Vec::new();
                    for _ in 0..20 {
                        let at = Instant::now();
                        std::hint::black_box(query::search(&store, text, 50));
                        times.push(at.elapsed().as_secs_f64() * 1000.0);
                    }
                    times.sort_by(f64::total_cmp);
                    rows.push(
                        serde_json::json!({"query":text,"p50_ms":times[10],"p95_ms":times[19],"filter_ms":filter_ms,"possible_blocks":blocks,"possible_name_bytes":bytes,"total_blocks":store.search_blocks()}),
                    );
                }
                segments.push(serde_json::json!({"overlay":overlay,"entries":store.len()-1,"overlay_records":store.overlay_len(),"queries":rows}));
            }
            println!("{}", serde_json::to_string(&segments)?);
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
    let mut memory = windows::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS_EX::default();
    memory.cb = size_of_val(&memory) as u32;
    // SAFETY: querying this process into a correctly sized extended counter structure.
    unsafe {
        windows::Win32::System::ProcessStatus::GetProcessMemoryInfo(
            windows::Win32::System::Threading::GetCurrentProcess(),
            (&mut memory as *mut windows::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS_EX)
                .cast(),
            size_of_val(&memory) as u32,
        )?;
    }
    let lookup_started = Instant::now();
    std::hint::black_box(store.child(root, "PROJECT-0000500-REPORT.TXT"));
    let lookup_ms = lookup_started.elapsed().as_secs_f64() * 1000.0;
    let mut after_lookup =
        windows::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS_EX::default();
    after_lookup.cb = size_of_val(&after_lookup) as u32;
    // SAFETY: same current-process query and correctly sized counter output as above.
    unsafe {
        windows::Win32::System::ProcessStatus::GetProcessMemoryInfo(
            windows::Win32::System::Threading::GetCurrentProcess(),
            (&mut after_lookup
                as *mut windows::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS_EX)
                .cast(),
            size_of_val(&after_lookup) as u32,
        )?;
    }
    trim_working_set();
    let mut trimmed = windows::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS_EX::default();
    trimmed.cb = size_of_val(&trimmed) as u32;
    // SAFETY: current process, correctly sized output counter structure.
    unsafe {
        windows::Win32::System::ProcessStatus::GetProcessMemoryInfo(
            windows::Win32::System::Threading::GetCurrentProcess(),
            (&mut trimmed
                as *mut windows::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS_EX)
                .cast(),
            size_of_val(&trimmed) as u32,
        )?;
    }
    let cold = Instant::now();
    std::hint::black_box(query::search(&store, "report", 50));
    let post_trim_query_ms = cold.elapsed().as_secs_f64() * 1000.0;
    println!(
        "{}",
        serde_json::json!({"entries":count,"record_bytes":size_of::<crate::store::Record>(),"snapshot_bytes":std::fs::metadata(&path)?.len(),"load_ms":load_ms,"queries":elapsed.len(),"p50_ms":elapsed[elapsed.len()/2],"p95_ms":elapsed[elapsed.len()*95/100],"max_ms":elapsed.last(),"by_query":by_query,"private_bytes":memory.PrivateUsage,"peak_pagefile_bytes":memory.PeakPagefileUsage,"working_set_bytes":memory.WorkingSetSize,"private_with_wide_lookup_bytes":after_lookup.PrivateUsage,"wide_lookup_build_ms":lookup_ms,"trimmed_working_set_bytes":trimmed.WorkingSetSize,"first_query_after_trim_ms":post_trim_query_ms})
    );
    drop(store);
    std::fs::remove_file(&path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_below_unindexed_parent_cannot_import_an_orphan_root() {
        let dir = std::env::temp_dir().join(format!("mypaper-orphan-{}", std::process::id()));
        let target = dir.join("node_modules/project/target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("CACHEDIR.TAG"), b"").unwrap();
        let scanned = scan::scan(&dir, Fetch::Basic).unwrap();
        let state = RwLock::new(State {
            store: scanned.store,
            scanning: false,
            cache: dir.join("cache"),
            activity: None,
            rules: Arc::new(RwLock::new(Rules::new(&Default::default(), Path::new("")).unwrap())),
            quiet: Arc::new(crate::quiet::Quiet::default()),
        });
        let before = state.read().unwrap().store.len();
        reconcile(&state, &target, false).unwrap();
        let state = state.read().unwrap();
        assert_eq!(state.store.len(), before);
        assert_eq!(state.store.record(state.store.record(0).first).next, NONE);
        assert!(state.store.find(&target.to_string_lossy()).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

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
            cache: dir.join("cache"),
            activity: None,
            rules: Arc::new(RwLock::new(
                Rules::new(&Default::default(), Path::new("")).unwrap(),
            )),
            quiet: Arc::new(crate::quiet::Quiet::default()),
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
        let moved = state
            .read()
            .unwrap()
            .store
            .find(&dir.join("b").to_string_lossy())
            .unwrap();
        assert!(state.read().unwrap().store.record(moved).modified > 0);
        std::fs::remove_file(dir.join("b/test.txt")).unwrap();
        apply(&state, &dir.join("b/test.txt")).unwrap();
        assert_eq!(state.read().unwrap().store.record(1).files, 0);
        std::fs::create_dir_all(dir.join("project/bin")).unwrap();
        std::fs::write(dir.join("project/app.csproj"), b"").unwrap();
        std::fs::write(dir.join("project/bin/build.dll"), b"123").unwrap();
        apply(&state, &dir.join("project")).unwrap();
        assert!(state.read().unwrap().store.hit(1).size_unknown);
        std::fs::remove_dir_all(dir.join("project")).unwrap();
        apply(&state, &dir.join("project")).unwrap();
        // A coalesced .csproj deletion follows its already-removed parent event.
        reconcile(&state, &dir.join("project"), false).unwrap();
        assert!(
            state
                .read()
                .unwrap()
                .store
                .find(&dir.join("project").to_string_lossy())
                .is_none()
        );
        assert!(!state.read().unwrap().store.hit(1).size_unknown);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
