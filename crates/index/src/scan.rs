//! Non-admin parallel directory enumeration. No file contents, short names, reparse traversal,
//! elevation, MFT reads or USN handles. All worker threads run in Windows background mode.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

use anyhow::Result;
use windows::Win32::Foundation::{ERROR_NO_MORE_FILES, HANDLE};
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN, THREAD_MODE_BACKGROUND_END,
};
use windows::core::PCWSTR;

use crate::rules::{Decision, Rules};
use crate::store::{DIRECTORY, EXCLUDED, REPARSE, SIZE_UNKNOWN, Store};

pub fn wide(path: &str) -> Vec<u16> {
    path.encode_utf16().chain([0]).collect()
}
pub fn long(path: &Path) -> Vec<u16> {
    let path = path.to_string_lossy().replace('/', "\\");
    wide(&if path.starts_with(r"\\?\") {
        path
    } else {
        format!(r"\\?\{path}")
    })
}

pub struct Background(bool);
impl Background {
    pub fn enter() -> Self {
        // SAFETY: background mode applies only to this worker thread and is reverted on drop.
        let entered =
            unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
        if let Err(error) = &entered {
            eprintln!("index background CPU/I/O priority unavailable: {error}");
        }
        Self(entered.is_ok())
    }
}
impl Drop for Background {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: this is the same worker thread that entered background mode.
            let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_END) };
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Fetch {
    Basic,
    Large,
}

#[derive(Debug)]
pub struct Entry {
    pub name: String,
    pub attributes: u32,
    pub size: u64,
    pub modified: u32,
}

pub fn unix_seconds(time: windows::Win32::Foundation::FILETIME) -> u32 {
    let ticks = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    (ticks / 10_000_000)
        .saturating_sub(11_644_473_600)
        .min(u64::from(u32::MAX)) as u32
}

struct Find(HANDLE);
impl Drop for Find {
    fn drop(&mut self) {
        // SAFETY: owns the successful FindFirstFileExW result.
        let _ = unsafe { FindClose(self.0) };
    }
}

fn entry(data: &WIN32_FIND_DATAW) -> Entry {
    let end = data
        .cFileName
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(data.cFileName.len());
    Entry {
        name: String::from_utf16_lossy(&data.cFileName[..end]),
        attributes: data.dwFileAttributes,
        size: (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow),
        modified: unix_seconds(data.ftLastWriteTime),
    }
}

pub fn metadata(path: &Path) -> Result<Entry> {
    let name = long(path);
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: name is NUL terminated; data is a valid output structure.
    let handle = unsafe {
        FindFirstFileExW(
            PCWSTR(name.as_ptr()),
            FindExInfoBasic,
            (&mut data as *mut WIN32_FIND_DATAW).cast(),
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_FLAGS(0),
        )?
    };
    let _handle = Find(handle);
    Ok(entry(&data))
}

pub fn enumerate(path: &Path, fetch: Fetch) -> Result<Vec<Entry>> {
    enumerate_quiet(path, fetch, None)
}

fn enumerate_quiet(
    path: &Path,
    fetch: Fetch,
    quiet: Option<&crate::quiet::Quiet>,
) -> Result<Vec<Entry>> {
    let pattern = long(&path.join("*"));
    let mut data = WIN32_FIND_DATAW::default();
    let flags = match fetch {
        Fetch::Basic => FIND_FIRST_EX_FLAGS(0),
        Fetch::Large => FIND_FIRST_EX_LARGE_FETCH,
    };
    // SAFETY: NUL-terminated absolute pattern and writable WIN32_FIND_DATAW.
    let handle = unsafe {
        FindFirstFileExW(
            PCWSTR(pattern.as_ptr()),
            FindExInfoBasic,
            (&mut data as *mut WIN32_FIND_DATAW).cast(),
            FindExSearchNameMatch,
            None,
            flags,
        )?
    };
    let _handle = Find(handle);
    let mut entries = Vec::new();
    loop {
        if entries.len() % 256 == 0
            && let Some(quiet) = quiet
        {
            quiet.wait();
        }
        let item = entry(&data);
        if item.name != "." && item.name != ".." && item.attributes & REPARSE == 0 {
            entries.push(item);
        }
        // SAFETY: live enumeration handle and valid output.
        if let Err(error) = unsafe { FindNextFileW(handle, &mut data) } {
            if error.code() == ERROR_NO_MORE_FILES.to_hresult() {
                break;
            }
            return Err(error.into());
        }
    }
    Ok(entries)
}

pub fn volumes() -> Vec<PathBuf> {
    let mut buffer = [0u16; 1024];
    // SAFETY: valid stack output buffer.
    let len = unsafe { GetLogicalDriveStringsW(Some(&mut buffer)) } as usize;
    if len >= buffer.len() {
        return Vec::new();
    }
    buffer[..len]
        .split(|c| *c == 0)
        .filter(|s| !s.is_empty())
        .filter_map(|root| {
            let name = String::from_utf16_lossy(root);
            let wide = wide(&name);
            let mut fs = [0; 64];
            // SAFETY: terminated root, writable file-system-name buffer; no volume mutation.
            unsafe {
                if GetDriveTypeW(PCWSTR(wide.as_ptr())) != 3 {
                    return None;
                }
                GetVolumeInformationW(PCWSTR(wide.as_ptr()), None, None, None, None, Some(&mut fs))
                    .ok()?;
            }
            let end = fs.iter().position(|c| *c == 0).unwrap_or(fs.len());
            matches!(
                String::from_utf16_lossy(&fs[..end]).as_str(),
                "NTFS" | "ReFS"
            )
            .then(|| PathBuf::from(name))
        })
        .collect()
}

pub struct Scanned {
    pub store: Store,
    pub seconds: f64,
    pub denied: usize,
}

#[derive(Debug)]
pub(crate) struct LargeSubtree;
impl std::fmt::Display for LargeSubtree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("subtree requires bounded scan scratch")
    }
}
impl std::error::Error for LargeSubtree {}

/// Live-created subtrees are usually a few entries. Enumerate on the already-background
/// service thread instead of starting four OS threads for every build/cache directory.
pub(crate) fn subtree(
    root: &Path,
    fetch: Fetch,
    rules: &Rules,
    quiet: &std::sync::Arc<crate::quiet::Quiet>,
) -> Result<Scanned> {
    let started = Instant::now();
    let mut store = Store::new();
    store.set_quiet(quiet.clone());
    let info = metadata(root).ok();
    let conditional = crate::rules::Markers::default().directory(root);
    let root_decision = rules.decide_known(root, true, conditional);
    let id = store.insert(
        0,
        root.to_string_lossy().trim_end_matches('\\'),
        info.as_ref().map_or(DIRECTORY, |entry| entry.attributes)
            | if root_decision != Decision::Index {
                EXCLUDED | SIZE_UNKNOWN
            } else {
                0
            },
        0,
        info.as_ref().map_or(0, |entry| entry.modified),
    );
    let mut pending = if root_decision == Decision::Excluded {
        vec![]
    } else {
        vec![(id, root.to_owned(), conditional)]
    };
    let mut denied = 0;
    while let Some((parent, path, conditional)) = pending.pop() {
        quiet.wait();
        match enumerate_quiet(&path, fetch, Some(quiet)) {
            Ok(entries) => {
                let has_project = entries.iter().any(|entry| {
                    entry.attributes & DIRECTORY == 0
                        && Path::new(&entry.name)
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("csproj"))
                });
                for entry in entries {
                    let path = path.join(&entry.name);
                    let child_condition = Rules::child_condition(
                        &path,
                        entry.attributes & DIRECTORY != 0,
                        conditional,
                        has_project,
                    );
                    let decision = rules.decide_known(
                        &path,
                        entry.attributes & DIRECTORY != 0,
                        child_condition,
                    );
                    let child = store.insert(
                        parent,
                        &entry.name,
                        entry.attributes
                            | if decision != Decision::Index {
                                EXCLUDED | SIZE_UNKNOWN
                            } else {
                                0
                            },
                        if decision == Decision::Excluded {
                            0
                        } else {
                            entry.size
                        },
                        entry.modified,
                    );
                    if store.len() > 25_000 || store.scan_bytes() > 8 * 1024 * 1024 {
                        return Err(LargeSubtree.into());
                    }
                    if entry.attributes & DIRECTORY != 0 && decision != Decision::Excluded {
                        pending.push((child, path, child_condition));
                    }
                }
            }
            Err(_) => {
                denied += 1;
                let mut record = store.record(parent);
                record.attributes |= EXCLUDED | SIZE_UNKNOWN;
                store.set(parent, record);
            }
        }
    }
    store.roll_up();
    Ok(Scanned {
        store,
        seconds: started.elapsed().as_secs_f64(),
        denied,
    })
}

/// Four workers share a directory queue; enumeration happens outside the short store lock.
/// TODO: optional elevated MFT bootstrap / USN replay, in a separate explicit admin path.
pub fn scan(root: &Path, fetch: Fetch) -> Result<Scanned> {
    scan_with_store(
        root,
        fetch,
        Store::new(),
        &Rules::new(&Default::default(), Path::new(""))?,
    )
}

pub(crate) fn scan_cached(
    root: &Path,
    fetch: Fetch,
    cache: &Path,
    rules: &Rules,
    quiet: &std::sync::Arc<crate::quiet::Quiet>,
) -> Result<Scanned> {
    let mut store = Store::temporary(&cache.join("scan"))?;
    store.set_quiet(quiet.clone());
    scan_with_store(root, fetch, store, rules)
}

fn scan_with_store(root: &Path, fetch: Fetch, mut store: Store, rules: &Rules) -> Result<Scanned> {
    let started = Instant::now();
    let conditional = crate::rules::Markers::default().directory(root);
    let root_decision = rules.decide_known(root, true, conditional);
    let root_id = store.insert(
        0,
        root.to_string_lossy().trim_end_matches('\\'),
        DIRECTORY
            | if root_decision != Decision::Index {
                EXCLUDED | SIZE_UNKNOWN
            } else {
                0
            },
        0,
        0,
    );
    struct Work {
        store: Store,
        pending: VecDeque<(u32, PathBuf, bool)>,
        active: usize,
        denied: usize,
    }
    let quiet = store.quiet();
    let shared = (
        Mutex::new(Work {
            store,
            pending: if root_decision == Decision::Excluded {
                VecDeque::new()
            } else {
                VecDeque::from([(root_id, root.to_owned(), conditional)])
            },
            active: 0,
            denied: 0,
        }),
        Condvar::new(),
    );
    let worker = || {
        let shared = &shared;
        let work_quiet = &quiet;
        loop {
            let (id, path, conditional) = {
                let mut work = shared.0.lock().expect("scan queue");
                loop {
                    if let Some(task) = work.pending.pop_front() {
                        work.active += 1;
                        break task;
                    }
                    if work.active == 0 {
                        return;
                    }
                    work = shared.1.wait(work).expect("scan queue wake");
                }
            };
            if let Some(quiet) = &work_quiet {
                quiet.wait();
            }
            let entries = enumerate_quiet(&path, fetch, work_quiet.as_deref()).map(|entries| {
                let has_project = entries.iter().any(|entry| {
                    entry.attributes & DIRECTORY == 0
                        && Path::new(&entry.name)
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("csproj"))
                });
                entries
                    .into_iter()
                    .map(|entry| {
                        let child_path = path.join(&entry.name);
                        let child_condition = Rules::child_condition(
                            &child_path,
                            entry.attributes & DIRECTORY != 0,
                            conditional,
                            has_project,
                        );
                        let decision = rules.decide_known(
                            &child_path,
                            entry.attributes & DIRECTORY != 0,
                            child_condition,
                        );
                        (entry, child_path, decision, child_condition)
                    })
                    .collect::<Vec<_>>()
            });
            let mut work = shared.0.lock().expect("scan queue");
            match entries {
                Ok(entries) => {
                    for (entry, child_path, decision, child_condition) in entries {
                        let child = work.store.insert(
                            id,
                            &entry.name,
                            entry.attributes
                                | if decision != Decision::Index {
                                    EXCLUDED | SIZE_UNKNOWN
                                } else {
                                    0
                                },
                            if decision == Decision::Excluded {
                                0
                            } else {
                                entry.size
                            },
                            entry.modified,
                        );
                        if entry.attributes & DIRECTORY != 0 && decision != Decision::Excluded {
                            work.pending.push_back((child, child_path, child_condition));
                        }
                    }
                }
                Err(_) => {
                    work.denied += 1;
                    let mut record = work.store.record(id);
                    record.attributes |= EXCLUDED | SIZE_UNKNOWN;
                    work.store.set(id, record);
                }
            }
            work.active -= 1;
            shared.1.notify_all();
        }
    };
    if crate::budget::Budget::active() {
        // Stay on the budgeted thread; spawning here would bypass its CPU accounting.
        worker();
    } else {
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let worker = &worker;
                scope.spawn(move || {
                    let _background = Background::enter();
                    worker();
                });
            }
        });
    }
    let mut work = shared.0.into_inner().expect("completed scan");
    work.store.roll_up();
    Ok(Scanned {
        store: work.store,
        seconds: started.elapsed().as_secs_f64(),
        denied: work.denied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conditional_build_trees_includes_and_unknown_sizes() {
        let root =
            std::env::temp_dir().join(format!("glaze-index-excludes-{}", std::process::id()));
        for dir in [
            "rust/target/keep",
            "ordinary/target",
            "dotnet/bin",
            "dotnet/obj",
            "node_modules/pkg",
            ".git/objects",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join("file.txt"), b"12345").unwrap();
        }
        std::fs::write(root.join("rust/target/CACHEDIR.TAG"), b"").unwrap();
        std::fs::write(root.join("dotnet/app.csproj"), b"").unwrap();
        let rules = Rules::new(
            &glaze_config::IndexConfig {
                include: vec![format!(
                    "{}/rust/target/keep/**",
                    root.to_string_lossy().replace('\\', "/")
                )],
                ..Default::default()
            },
            Path::new(""),
        )
        .unwrap();
        let quiet = std::sync::Arc::new(crate::quiet::Quiet::default());
        let scanned = subtree(&root, Fetch::Basic, &rules, &quiet).unwrap();
        let mut store = scanned.store;
        assert_eq!(store.record(1).size, 10);
        assert!(store.hit(1).size_unknown);
        assert!(
            store
                .find(&root.join("node_modules/pkg/file.txt").to_string_lossy())
                .is_none()
        );
        assert!(
            store
                .find(&root.join("rust/target/keep/file.txt").to_string_lossy())
                .is_some()
        );
        assert!(
            store
                .find(&root.join("ordinary/target/file.txt").to_string_lossy())
                .is_some()
        );
        for path in ["dotnet/bin", "dotnet/obj", "node_modules", ".git"] {
            let id = store.find(&root.join(path).to_string_lossy()).unwrap();
            assert!(store.hit(id).excluded);
            assert!(store.hit(id).size_unknown);
        }
        for path in ["dotnet/bin", "dotnet/obj", "node_modules", ".git", "rust"] {
            let id = store.find(&root.join(path).to_string_lossy()).unwrap();
            store.remove(id);
        }
        assert!(!store.hit(1).size_unknown);
        assert_eq!(store.hit(1).size, 5);
        std::fs::remove_dir_all(root).unwrap();
    }
}
