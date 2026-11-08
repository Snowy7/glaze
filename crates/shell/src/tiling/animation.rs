//! Transient DWM proxy animations. The worker and overlay die as soon as the spring settles.
use super::platform::{self, Placement};
use anyhow::Result;
use mypaper_desktop::tiling_recovery::Identity;
use mypaper_tiling::Rect;
use std::sync::Mutex;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{Dwm::*, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};
pub struct Applied {
    pub identity: Identity,
    pub expected: Rect,
    pub actual: Rect,
}
static APPLIED: Mutex<Vec<Applied>> = Mutex::new(Vec::new());
#[derive(Debug, Clone, serde::Serialize)]
pub struct FrameCost {
    pub windows: usize,
    pub frames: usize,
    pub mean_us: f64,
    pub p95_us: f64,
    pub interval_p95_ms: f64,
    pub refresh_period_ms: Option<f64>,
    pub missed_frames: Option<usize>,
}
static FRAME_COSTS: Mutex<Vec<FrameCost>> = Mutex::new(Vec::new());
pub fn measurements() -> Vec<FrameCost> {
    std::mem::take(&mut *FRAME_COSTS.lock().unwrap())
}
static BUSY: AtomicBool = AtomicBool::new(false);
/// Native location events are reconciled once after the current projection settles.
pub fn busy() -> bool {
    BUSY.load(Ordering::Acquire)
}
pub fn results() -> Vec<Applied> {
    std::mem::take(&mut *APPLIED.lock().unwrap())
}

#[derive(Clone)]
pub struct Change {
    pub identity: Identity,
    pub from: Rect,
    pub to: Rect,
    pub border: [f64; 4],
    pub visible: bool,
    pub dpi_changed: bool,
}
#[derive(Default)]
struct Jobs {
    pending: Option<Job>,
    running: bool,
}
struct Job {
    changes: Vec<Change>,
    animate: bool,
}
#[derive(Default)]
pub struct Animator {
    worker: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    paused: bool,
    jobs: Arc<Mutex<Jobs>>,
}
impl Animator {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn paused(&self) -> bool {
        self.paused
    }
    pub fn pause(&mut self, on: bool) {
        self.paused = on;
        if on {
            self.cancel();
        }
    }
    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.jobs.lock().unwrap().pending = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
    pub fn apply(&mut self, changes: Vec<Change>, animate: bool) -> Result<()> {
        if changes.is_empty() {
            self.cancel();
            return Ok(());
        }
        let mut jobs = self.jobs.lock().unwrap();
        jobs.pending = Some(Job {
            changes,
            animate: animate && !self.paused,
        });
        if jobs.running {
            return Ok(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        jobs.running = true;
        BUSY.store(true, Ordering::Release);
        drop(jobs);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let jobs = self.jobs.clone();
        let spawned = std::thread::Builder::new()
            .name("tiling-projection".into())
            .stack_size(512 * 1024)
            .spawn(move || projection_worker(jobs, cancel));
        match spawned {
            Ok(worker) => self.worker = Some(worker),
            Err(e) => {
                let mut jobs = self.jobs.lock().unwrap();
                jobs.pending = None;
                jobs.running = false;
                BUSY.store(false, Ordering::Release);
                return Err(e.into());
            }
        }
        Ok(())
    }
}
/// Consume only the latest pending native projection, then leave no idle worker.
fn projection_worker(jobs: Arc<Mutex<Jobs>>, cancel: Arc<AtomicBool>) {
    loop {
        let job = {
            let mut state = jobs.lock().unwrap();
            if cancel.load(Ordering::Acquire) { state.pending = None; }
            match state.pending.take() {
                Some(job) => job,
                None => { state.running = false; BUSY.store(false, Ordering::Release); break; }
            }
        };
        if let Err(error) = commit(&job.changes) { log::warn!("tiling projection: {error:#}"); }
    }
    crate::inbox::post(crate::inbox::WM_CONFIG + 23, 0);
}
impl Drop for Animator { fn drop(&mut self) { self.cancel(); } }
fn commit(changes: &[Change]) -> Result<()> {
    let placements: Vec<_> = changes
        .iter()
        .filter(|c| {
            !platform::hands_off(c.identity.window())
                && platform::frame(c.identity.window()).is_none_or(|r| !super::near(r, c.to))
        })
        .map(|c| Placement {
            identity: c.identity,
            rect: c.to,
            border: c.border,
            hung: !platform::responsive(c.identity.window()),
        })
        .collect();
    platform::apply(&placements)?;
    // Per-monitor-aware clients can react to WM_DPICHANGED after the first placement.
    // A second batch is limited to actual output-scale transitions.
    let mut second = Vec::new();
    for c in changes.iter().filter(|c| c.dpi_changed) {
        if let (Some(outer), Some(frame)) = (
            platform::outer(c.identity.window()),
            platform::frame(c.identity.window()),
        ) {
            second.push(Placement {
                identity: c.identity,
                rect: c.to,
                border: [
                    outer.x - frame.x,
                    outer.y - frame.y,
                    outer.w - frame.w,
                    outer.h - frame.h,
                ],
                hung: !platform::responsive(c.identity.window()),
            });
        }
    }
    platform::apply(&second)?;
    let mut results = APPLIED.lock().unwrap();
    for p in placements.iter().filter(|p| !p.hung) {
        if let Some(actual) = platform::frame(p.identity.window()) {
            results.push(Applied {
                identity: p.identity,
                expected: p.rect,
                actual,
            });
        }
    }
    drop(results);
    crate::inbox::post(crate::inbox::WM_CONFIG + 23, 0);
    Ok(())
}

