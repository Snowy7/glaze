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

use crate::store::{DIRECTORY, REPARSE, Store};

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

pub struct Background;
impl Background {
    pub fn enter() -> Self {
        // SAFETY: background mode applies only to this worker thread and is reverted on drop.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
        Self
    }
}
impl Drop for Background {
    fn drop(&mut self) {
        // SAFETY: this is the same worker thread that entered background mode.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_END) };
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

/// Live-created subtrees are usually a few entries. Enumerate on the already-background
/// service thread instead of starting four OS threads for every build/cache directory.
pub fn subtree(root: &Path, fetch: Fetch) -> Result<Scanned> {
    let started = Instant::now();
    let mut store = Store::new();
    let info = metadata(root).ok();
    let id = store.insert(
        0,
        root.to_string_lossy().trim_end_matches('\\'),
        info.as_ref().map_or(DIRECTORY, |entry| entry.attributes),
        0,
        info.as_ref().map_or(0, |entry| entry.modified),
    );
    let mut pending = vec![(id, root.to_owned())];
    let mut denied = 0;
    while let Some((parent, path)) = pending.pop() {
        match enumerate(&path, fetch) {
            Ok(entries) => {
                for entry in entries {
                    let child = store.insert(
                        parent,
                        &entry.name,
                        entry.attributes,
                        entry.size,
                        entry.modified,
                    );
                    if entry.attributes & DIRECTORY != 0 {
                        pending.push((child, path.join(&entry.name)));
                    }
                }
            }
            Err(_) => denied += 1,
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
    let started = Instant::now();
    let mut store = Store::new();
    let root_id = store.insert(
        0,
        root.to_string_lossy().trim_end_matches('\\'),
        DIRECTORY,
        0,
        0,
    );
    struct Work {
        store: Store,
        pending: VecDeque<(u32, PathBuf)>,
        active: usize,
        denied: usize,
    }
    let shared = (
        Mutex::new(Work {
            store,
            pending: VecDeque::from([(root_id, root.to_owned())]),
            active: 0,
            denied: 0,
        }),
        Condvar::new(),
    );
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let shared = &shared;
            scope.spawn(move || {
                let _background = Background::enter();
                loop {
                    let (id, path) = {
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
                    let entries = enumerate(&path, fetch);
                    let mut work = shared.0.lock().expect("scan queue");
                    match entries {
                        Ok(entries) => {
                            for entry in entries {
                                let child = work.store.insert(
                                    id,
                                    &entry.name,
                                    entry.attributes,
                                    entry.size,
                                    entry.modified,
                                );
                                if entry.attributes & DIRECTORY != 0 {
                                    work.pending.push_back((child, path.join(&entry.name)));
                                }
                            }
                        }
                        Err(_) => work.denied += 1,
                    }
                    work.active -= 1;
                    shared.1.notify_all();
                }
            });
        }
    });
    let mut work = shared.0.into_inner().expect("completed scan");
    work.store.roll_up();
    Ok(Scanned {
        store: work.store,
        seconds: started.elapsed().as_secs_f64(),
        denied: work.denied,
    })
}
