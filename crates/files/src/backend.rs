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
        sizes: HashMap<String, u64>,
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
    },
    Sort {
        epoch: u64,
        entries: Arc<Vec<Entry>>,
        sort: Sort,
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
                                    && let Ok(response) = mypaper_index::pipe::request(
                                        &mypaper_index::Request::Sizes {
                                            path: location.path,
                                            limit: 4096,
                                        },
                                        100,
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
                                                .map(|h| (h.path, h.size))
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
                                ..
                            } => {
                                if active_query.load(Ordering::Relaxed) != query {
                                    return Ok(());
                                }
                                let indexed = (!text.trim().is_empty())
                                    .then(|| {
                                        mypaper_index::pipe::request(
                                            &mypaper_index::Request::Search {
                                                query: text.clone(),
                                                limit: 2000,
                                            },
                                            100,
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
                                            size: Some(hit.size),
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
                            Job::Sort { entries, sort, .. } => {
                                let mut entries = entries.as_ref().clone();
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
    pub fn search(&self, text: String, entries: Arc<Vec<Entry>>, sort: Sort) -> u64 {
        let query = self.query.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.jobs.send(Job::Search {
            epoch: self.epoch.load(Ordering::Relaxed),
            query,
            text,
            entries,
            sort,
        });
        query
    }
    pub fn sort(&self, entries: Arc<Vec<Entry>>, sort: Sort) {
        self.query.store(0, Ordering::Relaxed);
        let _ = self.jobs.send(Job::Sort {
            epoch: self.epoch.load(Ordering::Relaxed),
            entries,
            sort,
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
}
