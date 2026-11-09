//! Bounded background work. Workers block on channels at rest and wake the HWND after results;
//! there is no polling timer and navigation cancels obsolete directory/image requests.
use crate::{
    model::{Entry, Sort},
    shell::{self, Decoration, Location, Operation},
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    time::Instant,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

pub const WAKE: u32 = WM_APP + 171;

pub enum Reply {
    DriveSpace {
        path: String,
        space: Option<crate::model::DriveSpace>,
    },
    Changed {
        path: String,
    },
    Folder {
        epoch: u64,
        location: Location,
        entries: Arc<Vec<Entry>>,
        complete: bool,
        elapsed_ms: u128,
    },
    Search {
        epoch: u64,
        query: u64,
        entries: Arc<Vec<Entry>>,
        indexed: bool,
    },
    Sizes {
        epoch: u64,
        sizes: HashMap<String, crate::model::Size>,
    },
    Image {
        epoch: u64,
        key: (String, i32),
        decoration: Option<Decoration>,
    },
    Operation(Result<(), String>),
    Error {
        epoch: u64,
        message: String,
    },
}

enum Job {
    Folder {
        epoch: u64,
        path: String,
        base: Option<String>,
        sort: Sort,
    },
    Search {
        epoch: u64,
        query: u64,
        text: String,
        entries: Arc<Vec<Entry>>,
        sort: Sort,
        sizes: HashMap<String, crate::model::Size>,
    },
    Sort {
        epoch: u64,
        entries: Arc<Vec<Entry>>,
        sort: Sort,
        sizes: HashMap<String, crate::model::Size>,
    },
}

pub struct Backend {
    jobs: Sender<Job>,
    images: SyncSender<(u64, String, i32, bool)>,
    pub replies: Receiver<Reply>,
    epoch: Arc<AtomicU64>,
    query: Arc<AtomicU64>,
    sender: SyncSender<Reply>,
    hwnd: usize,
}

fn send(sender: &SyncSender<Reply>, hwnd: usize, reply: Reply) {
    if sender.send(reply).is_ok() {
        // SAFETY: only a wake message; a closed HWND rejects it. No pointers cross threads.
        let _ = unsafe { PostMessageW(Some(HWND(hwnd as _)), WAKE, WPARAM(0), LPARAM(0)) };
    }
}

impl Backend {
    pub fn new(hwnd: HWND) -> anyhow::Result<Self> {
        let hwnd = hwnd.0 as usize;
        let (sender, replies) = mpsc::sync_channel(12);
        let (jobs, receiver) = mpsc::channel();
        let (images, image_rx) = mpsc::sync_channel::<(u64, String, i32, bool)>(128);
        let epoch = Arc::new(AtomicU64::new(0));
        let query = Arc::new(AtomicU64::new(0));
        let (out, current, active_query) = (sender.clone(), epoch.clone(), query.clone());
        let pending_drives = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        std::thread::Builder::new()
            .name("files-folders".into())
            .spawn(move || {
                let Ok(_apartment) = shell::Apartment::new() else {
                    return;
                };
                while let Ok(job) = receiver.recv() {
                    let epoch = match &job {
                        Job::Folder { epoch, .. }
                        | Job::Search { epoch, .. }
                        | Job::Sort { epoch, .. } => *epoch,
                    };
                    if current.load(Ordering::Relaxed) != epoch {
                        continue;
                    }
                    let result: anyhow::Result<()> = (|| {
                        match job {
                            Job::Folder {
                                path, base, sort, ..
                            } => {
                                let started = Instant::now();
                                let location = shell::resolve(&path, base.as_deref())?;
                                let mut entries = shell::enumerate(
                                    &location,
                                    || current.load(Ordering::Relaxed) != epoch,
                                    |page| {
                                        let mut entries = page.to_vec();
                                        entries.sort_by(|a, b| sort.compare(a, b));
                                        send(
                                            &out,
                                            hwnd,
                                            Reply::Folder {
                                                epoch,
                                                location: location.clone(),
                                                entries: Arc::new(entries),
                                                complete: false,
                                                elapsed_ms: started.elapsed().as_millis(),
                                            },
                                        );
                                    },
                                )?;
                                if current.load(Ordering::Relaxed) != epoch {
                                    return Ok(());
                                }
                                entries.sort_by(|a, b| sort.compare(a, b));
                                // Each root gets its own one-shot worker: a stalled network or
                                // removable drive cannot hold up the listing or other drives.
                                for entry in entries.iter().filter(|e| shell::drive_root(&e.path)) {
                                    let path = entry.path.clone();
                                    if !pending_drives.lock().unwrap().insert(path.clone()) {
                                        continue;
                                    }
                                    let (out, pending) = (out.clone(), pending_drives.clone());
                                    let failed_path = path.clone();
                                    if std::thread::Builder::new()
                                        .name("files-drive-space".into())
                                        .spawn(move || {
                                            let space = shell::drive_space(&path);
                                            pending.lock().unwrap().remove(&path);
                                            send(&out, hwnd, Reply::DriveSpace { path, space });
                                        })
                                        .is_err()
                                    {
                                        pending_drives.lock().unwrap().remove(&failed_path);
                                    }
                                }
                                send(
                                    &out,
                                    hwnd,
                                    Reply::Folder {
                                        epoch,
                                        location: location.clone(),
                                        entries: Arc::new(entries),
                                        complete: true,
                                        elapsed_ms: started.elapsed().as_millis(),
                                    },
                                );
                                if location.filesystem
                                    && let Ok(response) = glaze_index::pipe::request(
                                        &glaze_index::Request::Sizes {
                                            path: location.path,
                                            limit: 4096,
                                        },
                                        250,
                                    )
                                    && response.error.is_none()
                                {
                                    send(
                                        &out,
                                        hwnd,
                                        Reply::Sizes {
                                            epoch,
                                            sizes: response
                                                .hits
                                                .into_iter()
                                                .filter(|h| h.directory)
                                                .map(|h| {
                                                    let size = crate::model::Size::indexed(
                                                        h.size,
                                                        h.size_unknown,
                                                        h.excluded,
                                                    );
                                                    (h.path, size)
                                                })
                                                .collect(),
                                        },
                                    );
                                }
                            }
                            Job::Search {
                                query,
                                text,
                                entries,
                                sort,
                                sizes,
                                ..
                            } => {
                                if active_query.load(Ordering::Relaxed) != query {
                                    return Ok(());
                                }
                                let indexed = (!text.trim().is_empty())
                                    .then(|| {
                                        glaze_index::pipe::request(
                                            &glaze_index::Request::Search {
                                                query: text.clone(),
                                                limit: 2000,
                                            },
                                            250,
                                        )
                                    })
                                    .transpose()
                                    .ok()
                                    .flatten()
                                    .filter(|r| r.error.is_none());
                                let from_index = indexed.is_some();
                                let mut filtered = if let Some(response) = indexed {
                                    response
                                        .hits
                                        .into_iter()
                                        .map(|hit| Entry {
                                            name: hit.name,
                                            path: hit.path,
                                            directory: hit.directory,
                                            size: Some(crate::model::Size::indexed(
                                                hit.size,
                                                hit.size_unknown,
                                                hit.excluded,
                                            )),
                                            modified: (u64::from(hit.modified) + 11_644_473_600)
                                                * 10_000_000,
                                            kind: if hit.directory { "Folder" } else { "File" }
                                                .into(),
                                            hidden: false,
                                        })
                                        .collect::<Vec<_>>()
                                } else {
                                    let text = text.to_lowercase();
                                    entries
                                        .iter()
                                        .filter(|entry| entry.name.to_lowercase().contains(&text))
                                        .cloned()
                                        .collect()
                                };
                                if active_query.load(Ordering::Relaxed) != query {
                                    return Ok(());
                                }
                                if !from_index {
                                    for entry in &mut filtered {
                                        if entry.directory {
                                            entry.size =
                                                sizes.get(&entry.path).copied().or(entry.size);
                                        }
                                    }
                                }
                                filtered.sort_by(|a, b| sort.compare(a, b));
                                send(
                                    &out,
                                    hwnd,
                                    Reply::Search {
                                        epoch,
                                        query,
                                        entries: Arc::new(filtered),
                                        indexed: from_index,
                                    },
                                );
                            }
                            Job::Sort {
                                entries,
                                sort,
                                sizes,
                                ..
                            } => {
                                let mut entries = entries.as_ref().clone();
                                for entry in &mut entries {
                                    if entry.directory {
                                        entry.size = sizes.get(&entry.path).copied().or(entry.size);
                                    }
                                }
                                entries.sort_by(|a, b| sort.compare(a, b));
                                if active_query.load(Ordering::Relaxed) == 0 {
                                    send(
                                        &out,
                                        hwnd,
                                        Reply::Search {
                                            epoch,
                                            query: 0,
                                            entries: Arc::new(entries),
                                            indexed: false,
                                        },
                                    );
                                }
                            }
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        send(
                            &out,
                            hwnd,
                            Reply::Error {
                                epoch,
                                message: format!("{error:#}"),
                            },
                        );
                    }
                }
            })?;
        let (out, current) = (sender.clone(), epoch.clone());
        std::thread::Builder::new()
            .name("files-images".into())
            .spawn(move || {
                let Ok(_apartment) = shell::Apartment::new() else {
                    return;
                };
                while let Ok((epoch, path, size, thumbnail)) = image_rx.recv() {
                    if current.load(Ordering::Relaxed) != epoch {
                        continue;
                    }
                    let decoration = shell::decorate(&path, size, thumbnail).ok();
                    if current.load(Ordering::Relaxed) == epoch {
                        send(
                            &out,
                            hwnd,
                            Reply::Image {
                                epoch,
                                key: (path, size),
                                decoration,
                            },
                        );
                    }
                }
            })?;
        Ok(Self {
            jobs,
            images,
            replies,
            epoch,
            query,
            sender,
            hwnd,
        })
    }

    pub fn folder(&self, path: String, base: Option<String>, sort: Sort) -> u64 {
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        self.query.store(0, Ordering::Relaxed);
        let _ = self.jobs.send(Job::Folder {
            epoch,
            path,
            base,
            sort,
        });
        epoch
    }
    pub fn search(
        &self,
        text: String,
        entries: Arc<Vec<Entry>>,
        sort: Sort,
        sizes: HashMap<String, crate::model::Size>,
    ) -> u64 {
        let query = self.query.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.jobs.send(Job::Search {
            epoch: self.epoch.load(Ordering::Relaxed),
            query,
            text,
            entries,
            sort,
            sizes,
        });
        query
    }
    pub fn sort(
        &self,
        entries: Arc<Vec<Entry>>,
        sort: Sort,
        sizes: HashMap<String, crate::model::Size>,
    ) {
        self.query.store(0, Ordering::Relaxed);
        let _ = self.jobs.send(Job::Sort {
            epoch: self.epoch.load(Ordering::Relaxed),
            entries,
            sort,
            sizes,
        });
    }
    pub fn image(&self, path: String, size: i32, thumbnail: bool) -> bool {
        self.images
            .try_send((self.epoch.load(Ordering::Relaxed), path, size, thumbnail))
            .is_ok()
    }
    pub fn operation(&self, operation: Operation) -> anyhow::Result<()> {
        let (out, hwnd) = (self.sender.clone(), self.hwnd);
        std::thread::Builder::new()
            .name("files-operation".into())
            .spawn(move || {
                let result = shell::Apartment::new()
                    .and_then(|_apartment| shell::operate(operation, HWND(hwnd as _)))
                    .map_err(|e| format!("{e:#}"));
                send(&out, hwnd, Reply::Operation(result));
            })?;
        Ok(())
    }

    pub fn watch(&self, path: String) -> anyhow::Result<FolderWatch> {
        FolderWatch::new(path, self.sender.clone(), self.hwnd)
    }
}

struct Stop(usize);
impl Drop for Stop {
    fn drop(&mut self) {
        // SAFETY: the final Arc owner closes the event after the watcher stops using it.
        let _ = unsafe {
            windows::Win32::Foundation::CloseHandle(windows::Win32::Foundation::HANDLE(self.0 as _))
        };
    }
}

/// One directory notification, cancelled on navigation or close. It does not recurse or poll.
pub struct FolderWatch {
    stop: Arc<Stop>,
}
impl FolderWatch {
    fn new(path: String, sender: SyncSender<Reply>, hwnd: usize) -> anyhow::Result<Self> {
        use windows::{
            Win32::{Foundation::*, Storage::FileSystem::*, System::Threading::*},
            core::HSTRING,
        };
        // SAFETY: an unnamed manual-reset cancellation event, shared until the worker exits.
        let stop = Arc::new(Stop(
            unsafe { CreateEventW(None, true, false, None) }?.0 as usize,
        ));
        let worker_stop = stop.clone();
        std::thread::Builder::new()
            .name("files-directory-watch".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                // SAFETY: notification creation is off the UI thread; this worker owns its handle.
                let notification = unsafe {
                    FindFirstChangeNotificationW(
                        &HSTRING::from(&path),
                        false,
                        FILE_NOTIFY_CHANGE_FILE_NAME
                            | FILE_NOTIFY_CHANGE_DIR_NAME
                            | FILE_NOTIFY_CHANGE_LAST_WRITE
                            | FILE_NOTIFY_CHANGE_SIZE,
                    )
                };
                let Ok(notification) = notification else {
                    return;
                };
                let stop = HANDLE(worker_stop.0 as _);
                loop {
                    // SAFETY: both handles remain live throughout this wait; cancellation wins.
                    if unsafe { WaitForMultipleObjects(&[stop, notification], false, INFINITE) }
                        != WAIT_EVENT(WAIT_OBJECT_0.0 + 1)
                    {
                        break;
                    }
                    // SAFETY: rearm this worker's notification before publishing the change.
                    if unsafe { FindNextChangeNotification(notification) }.is_err() {
                        break;
                    }
                    send(&sender, hwnd, Reply::Changed { path: path.clone() });
                }
                // SAFETY: this worker owns the successful directory notification.
                let _ = unsafe { FindCloseChangeNotification(notification) };
            })?;
        Ok(Self { stop })
    }
}
impl Drop for FolderWatch {
    fn drop(&mut self) {
        // SAFETY: this owner's Arc keeps the cancellation event live through SetEvent.
        let _ = unsafe {
            windows::Win32::System::Threading::SetEvent(windows::Win32::Foundation::HANDLE(
                self.stop.0 as _,
            ))
        };
    }
}
