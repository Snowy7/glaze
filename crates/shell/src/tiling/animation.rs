//! Transient DWM proxy animations. The worker and overlay die as soon as the spring settles.
use super::platform::{self, Placement};
use anyhow::Result;
use glaze_desktop::tiling_recovery::Identity;
use glaze_tiling::Rect;
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
    /// The drawing endpoint can be off-view on the left while the HWND is parked on the right.
    pub visual_to: Rect,
    /// The proxy is cropped to its owning output, independently of the global parking rectangle.
    pub clip: Rect,
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
enum Outcome {
    Finished,
    Replaced {
        positions: Vec<(Identity, Rect)>,
        overlay: Overlay,
    },
    Cancelled,
}
/// One newest pending projection, with no sleeping worker once the queue is empty.
fn projection_worker(jobs: Arc<Mutex<Jobs>>, cancel: Arc<AtomicBool>) {
    let mut positions = Vec::new();
    let mut previous = None;
    loop {
        let job = {
            let mut state = jobs.lock().unwrap();
            if cancel.load(Ordering::Acquire) {
                state.pending = None;
            }
            match state.pending.take() {
                Some(job) => job,
                None => {
                    state.running = false;
                    BUSY.store(false, Ordering::Release);
                    break;
                }
            }
        };
        let mut job = job;
        for change in &mut job.changes {
            if let Some((_, rect)) = positions.iter().find(|(id, _)| *id == change.identity) {
                change.from = *rect;
            } else if let Some(rect) = platform::frame(change.identity.window())
                && intersects(rect, change.clip)
            {
                change.from = rect;
            }
        }
        let result = if job.animate {
            run(&job.changes, &cancel, &jobs, previous.take())
        } else {
            let result = commit(&job.changes).map(|()| Outcome::Finished);
            drop(previous.take());
            result
        };
        match result {
            Ok(Outcome::Replaced {
                positions: current,
                overlay,
            }) => {
                positions = current;
                previous = Some(overlay);
            }
            Ok(Outcome::Finished) => positions.clear(),
            Ok(Outcome::Cancelled) => {
                positions.clear();
            }
            Err(e) => {
                log::warn!("tiling projection: {e:#}");
                if !cancel.load(Ordering::Acquire) && jobs.lock().unwrap().pending.is_none() {
                    let _ = commit(&job.changes);
                }
                positions.clear();
            }
        }
    }
    drop(previous);
    crate::inbox::post(crate::inbox::WM_CONFIG + 23, 0);
}
impl Drop for Animator {
    fn drop(&mut self) {
        self.cancel();
    }
}
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

/// Closed-form critically damped spring, stiffness 800 and unit mass (niri's movement default).
pub fn progress(seconds: f64) -> f64 {
    let frequency = 800.0_f64.sqrt();
    1.0 - (1.0 + frequency * seconds) * (-frequency * seconds).exp()
}
fn lerp(a: Rect, b: Rect, t: f64) -> Rect {
    Rect {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        w: a.w + (b.w - a.w) * t,
        h: a.h + (b.h - a.h) * t,
    }
}
fn intersects(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}
/// Clip the destination and source together, preserving scale instead of squeezing the image.
fn crop_projection(destination: Rect, clip: Rect, source: Rect) -> Option<(Rect, Rect)> {
    if destination.w <= 0.0 || destination.h <= 0.0 || source.w <= 0.0 || source.h <= 0.0 {
        return None;
    }
    let x = destination.x.max(clip.x);
    let y = destination.y.max(clip.y);
    let right = (destination.x + destination.w).min(clip.x + clip.w);
    let bottom = (destination.y + destination.h).min(clip.y + clip.h);
    if right <= x || bottom <= y {
        return None;
    }
    Some((
        Rect {
            x,
            y,
            w: right - x,
            h: bottom - y,
        },
        Rect {
            x: source.x + (x - destination.x) / destination.w * source.w,
            y: source.y + (y - destination.y) / destination.h * source.h,
            w: (right - x) / destination.w * source.w,
            h: (bottom - y) / destination.h * source.h,
        },
    ))
}
fn native(r: Rect, origin: Rect) -> RECT {
    RECT {
        left: (r.x - origin.x).round() as i32,
        top: (r.y - origin.y).round() as i32,
        right: (r.x + r.w - origin.x).round() as i32,
        bottom: (r.y + r.h - origin.y).round() as i32,
    }
}

/// DWM host owns its regions and thumbnails. It never takes activation or pointer input.
struct Overlay {
    window: HWND,
    thumbnails: Vec<isize>,
    snapshots: Vec<Snapshot>,
    source_region: HRGN,
}
impl Drop for Overlay {
    fn drop(&mut self) {
        /* SAFETY: resources created on this thread, released once. */
        unsafe {
            for t in self.thumbnails.drain(..) {
                let _ = DwmUnregisterThumbnail(t);
            }
            let _ = DeleteObject(HGDIOBJ(self.source_region.0));
            let _ = DestroyWindow(self.window);
        }
        self.snapshots.clear();
    }
}
unsafe extern "system" fn procedure(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCHITTEST {
        return LRESULT(HTTRANSPARENT as isize);
    }
    if msg == WM_ERASEBKGND {
        return LRESULT(1);
    }
    // SAFETY: forwarding the message to the default procedure.
    unsafe { DefWindowProcW(h, msg, w, l) }
}
fn run(
    changes: &[Change],
    cancel: &AtomicBool,
    jobs: &Mutex<Jobs>,
    mut previous: Option<Overlay>,
) -> Result<Outcome> {
    let monitors = glaze_desktop::monitors();
    let Some(first) = monitors.first() else {
        return Ok(Outcome::Finished);
    };
    let mut bounds = Rect {
        x: f64::from(first.x),
        y: f64::from(first.y),
        w: f64::from(first.width),
        h: f64::from(first.height),
    };
    for m in &monitors {
        let right = (bounds.x + bounds.w).max(f64::from(m.x + m.width));
        let bottom = (bounds.y + bounds.h).max(f64::from(m.y + m.height));
        bounds.x = bounds.x.min(f64::from(m.x));
        bounds.y = bounds.y.min(f64::from(m.y));
        bounds.w = right - bounds.x;
        bounds.h = bottom - bounds.y;
    }
    // SAFETY: a static window class and a no-activate, input-transparent shell window.
    let window = unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: GetModuleHandleW(None)?.into(),
            lpszClassName: w!("glaze.tiling.animation"),
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            ..Default::default()
        };
        RegisterClassExW(&class);
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            w!("glaze.tiling.animation"),
            w!("Scrolling tiling"),
            WS_POPUP,
            bounds.x as i32,
            bounds.y as i32,
            bounds.w as i32,
            bounds.h as i32,
            None,
            None,
            None,
            None,
        )?
    };
    let mut overlay = Overlay {
        window,
        thumbnails: Vec::new(),
        snapshots: Vec::new(),
        // SAFETY: this region stays ours; per-frame copies are transferred to our host HWND.
        source_region: unsafe { CreateRectRgn(0, 0, 0, 0) },
    };
    anyhow::ensure!(
        !overlay.source_region.is_invalid(),
        "creating source clip region"
    );
    let mut sources = Vec::new();
    for change in changes {
        if !change.identity.valid() {
            continue;
        }
        let sweep = Rect {
            x: change.from.x.min(change.visual_to.x),
            y: change.from.y.min(change.visual_to.y),
            w: (change.from.x + change.from.w).max(change.visual_to.x + change.visual_to.w)
                - change.from.x.min(change.visual_to.x),
            h: (change.from.y + change.from.h).max(change.visual_to.y + change.visual_to.h)
                - change.from.y.min(change.visual_to.y),
        };
        if !intersects(sweep, change.clip) {
            continue;
        }
        let resizing = (change.from.w - change.visual_to.w).abs() > 2.0
            || (change.from.h - change.visual_to.h).abs() > 2.0;
        let source = if resizing && platform::responsive(change.identity.window()) {
            match Snapshot::capture(
                change.identity.window(),
                platform::frame(change.identity.window()).unwrap_or(change.from),
                bounds,
            ) {
                Ok(snapshot) => {
                    let h = snapshot.window;
                    overlay.snapshots.push(snapshot);
                    h
                }
                Err(_) => change.identity.window(),
            }
        } else {
            change.identity.window()
        };
        // SAFETY: source and destination are top-level, destination is our own HWND.
        if let Ok(thumbnail) = unsafe { DwmRegisterThumbnail(window, source) } {
            overlay.thumbnails.push(thumbnail);
            // The real source stays put until the final commit. Its mask is constant for this
            // projection, including retargets whose `from` is a moving proxy rather than native bounds.
            if let Some(real) = platform::frame(change.identity.window()) {
                let r = native(real, bounds);
                // SAFETY: both regions are ours; the temporary part is deleted after union.
                unsafe {
                    let part = CreateRoundRectRgn(r.left, r.top, r.right, r.bottom, 16, 16);
                    CombineRgn(
                        Some(overlay.source_region),
                        Some(overlay.source_region),
                        Some(part),
                        RGN_OR,
                    );
                    let _ = DeleteObject(HGDIOBJ(part.0));
                }
            }
            let crop = platform::frame(source)
                .zip(platform::outer(source))
                .map(|(frame, outer)| RECT {
                    left: (frame.x - outer.x).round() as i32,
                    top: (frame.y - outer.y).round() as i32,
                    right: (frame.x + frame.w - outer.x).round() as i32,
                    bottom: (frame.y + frame.h - outer.y).round() as i32,
                })
                .unwrap_or_default();
            sources.push((thumbnail, change, crop));
        }
    }
    if sources.is_empty() {
        commit(changes)?;
        return Ok(Outcome::Finished);
    }
    // SAFETY: raise within the normal band, leaving PiP, topmost applications and shell bars above it.
    unsafe {
        let _ = SetWindowPos(
            window,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    let start = Instant::now();
    let mut costs = Vec::new();
    let mut intervals = Vec::new();
    let mut last_frame = start;
    let mut timing = DWM_TIMING_INFO {
        cbSize: size_of::<DWM_TIMING_INFO>() as u32,
        ..Default::default()
    };
    // SAFETY: null HWND requests DWM's timing; the initialized struct has the required size.
    let period = unsafe { DwmGetCompositionTimingInfo(HWND::default(), &mut timing) }
        .ok()
        .and_then(|()| {
            let rate = timing.rateRefresh;
            (rate.uiNumerator > 0)
                .then(|| 1000.0 * f64::from(rate.uiDenominator) / f64::from(rate.uiNumerator))
        });
    let mut shown = false;
    loop {
        let elapsed = start.elapsed().as_secs_f64();
        let t = progress(elapsed);
        let frame = Instant::now();
        // Copy the stationary native mask, then add only the current proxy rectangles.
        // SAFETY: GDI regions are owned locally until SetWindowRgn transfers the union to USER.
        unsafe {
            let region = CreateRectRgn(0, 0, 0, 0);
            CombineRgn(Some(region), Some(overlay.source_region), None, RGN_COPY);
            for (thumbnail, c, crop) in &sources {
                let r = lerp(c.from, c.visual_to, t);
                let clipped = crop_projection(r, c.clip, platform::rect(*crop));
                let geometry = clipped
                    .map(|(dest, source)| (native(dest, bounds), native(source, Rect::default())));
                let geometry = geometry.filter(|(dest, source)| {
                    dest.right > dest.left
                        && dest.bottom > dest.top
                        && source.right > source.left
                        && source.bottom > source.top
                });
                let Some((dest, source)) = geometry else {
                    let _ = DwmUpdateThumbnailProperties(
                        *thumbnail,
                        &DWM_THUMBNAIL_PROPERTIES {
                            dwFlags: DWM_TNP_VISIBLE,
                            fVisible: false.into(),
                            ..Default::default()
                        },
                    );
                    continue;
                };
                let properties = DWM_THUMBNAIL_PROPERTIES {
                    dwFlags: DWM_TNP_VISIBLE
                        | DWM_TNP_RECTDESTINATION
                        | DWM_TNP_OPACITY
                        | DWM_TNP_SOURCECLIENTAREAONLY,
                    rcSource: source,
                    rcDestination: dest,
                    opacity: 255,
                    fVisible: true.into(),
                    fSourceClientAreaOnly: false.into(),
                    ..Default::default()
                };
                let properties = DWM_THUMBNAIL_PROPERTIES {
                    dwFlags: properties.dwFlags | DWM_TNP_RECTSOURCE,
                    ..properties
                };
                let _ = DwmUpdateThumbnailProperties(*thumbnail, &properties);
                let part = CreateRoundRectRgn(dest.left, dest.top, dest.right, dest.bottom, 16, 16);
                CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
                let _ = DeleteObject(HGDIOBJ(part.0));
            }
            if SetWindowRgn(window, Some(region), true) == 0 {
                let _ = DeleteObject(HGDIOBJ(region.0));
                anyhow::bail!("applying animation overlay clip");
            }
        }
        if !shown {
            // SAFETY: the owned overlay has a valid clip and proxy bounds before it is visible.
            unsafe {
                let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            }
            shown = true;
        }
        costs.push(frame.elapsed().as_secs_f64() * 1e6);
        // SAFETY: synchronize our worker to DWM, and dispatch only its own window messages.
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            DwmFlush()?;
        }
        drop(previous.take());
        let completed = Instant::now();
        intervals.push(completed.duration_since(last_frame).as_secs_f64() * 1000.0);
        last_frame = completed;
        if cancel.load(Ordering::Acquire) {
            return Ok(Outcome::Cancelled);
        }
        if jobs.lock().unwrap().pending.is_some() {
            return Ok(Outcome::Replaced {
                positions: changes
                    .iter()
                    .map(|c| (c.identity, lerp(c.from, c.visual_to, t)))
                    .collect(),
                overlay,
            });
        }
        if t > 0.999 || elapsed > 1.0 {
            break;
        }
    }
    commit(changes)?;
    // One composed frame covers the final commit; bounded repaint wait, never an idle timer.
    unsafe {
        let _ = DwmFlush();
    }
    costs.sort_by(f64::total_cmp);
    let mean = costs.iter().sum::<f64>() / costs.len() as f64;
    let p95 = costs[(costs.len() * 95 / 100).min(costs.len() - 1)];
    let missed = period.map(|period| intervals.iter().filter(|&&dt| dt > period * 1.5).count());
    intervals.sort_by(f64::total_cmp);
    let interval_p95 = intervals[(intervals.len() * 95 / 100).min(intervals.len() - 1)];
    log::info!(
        "tiling animation windows={} frames={} update_us mean={mean:.2} p95={p95:.2} interval_p95_ms={interval_p95:.2} missed={missed:?} refresh_ms={period:?}",
        sources.len(),
        costs.len()
    );
    if std::env::var_os("GLAZE_TILING_STATS").is_some() {
        let mut records = FRAME_COSTS.lock().unwrap();
        if records.len() >= 64 {
            records.remove(0);
        }
        records.push(FrameCost {
            windows: sources.len(),
            frames: costs.len(),
            mean_us: mean,
            p95_us: p95,
            interval_p95_ms: interval_p95,
            refresh_period_ms: period,
            missed_frames: missed,
        });
    }
    Ok(Outcome::Finished)
}

struct Snapshot {
    window: HWND,
    bitmap: HBITMAP,
}
impl Snapshot {
    fn capture(source: HWND, rect: Rect, bounds: Rect) -> Result<Self> {
        let outer = platform::outer(source)
            .ok_or_else(|| anyhow::anyhow!("snapshot source disappeared"))?;
        // SAFETY: the DC/bitmap are local GDI resources; restored and released on every path.
        unsafe {
            let dc = GetWindowDC(Some(source));
            let memory = CreateCompatibleDC(Some(dc));
            let bitmap = CreateCompatibleBitmap(dc, outer.w as i32, outer.h as i32);
            let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
            let printed = windows::Win32::Storage::Xps::PrintWindow(
                source,
                memory,
                windows::Win32::Storage::Xps::PRINT_WINDOW_FLAGS(2),
            )
            .as_bool();
            SelectObject(memory, previous);
            let _ = DeleteDC(memory);
            ReleaseDC(Some(source), dc);
            if !printed {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                anyhow::bail!("PrintWindow refused snapshot");
            }
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(snapshot_proc),
                hInstance: GetModuleHandleW(None)?.into(),
                lpszClassName: w!("glaze.tiling.snapshot"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            let window = match CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("glaze.tiling.snapshot"),
                w!("Tiling snapshot"),
                WS_POPUP,
                (bounds.x + bounds.w + 8.0) as i32,
                bounds.y as i32,
                rect.w as i32,
                rect.h as i32,
                None,
                None,
                None,
                None,
            ) {
                Ok(h) => h,
                Err(e) => {
                    let _ = DeleteObject(HGDIOBJ(bitmap.0));
                    return Err(e.into());
                }
            };
            SetWindowLongPtrW(window, GWLP_USERDATA, bitmap.0 as isize);
            let snapshot = Self { window, bitmap };
            SetPropW(
                window,
                w!("glaze.crop.x"),
                Some(windows::Win32::Foundation::HANDLE(
                    (rect.x - outer.x).round() as usize as *mut _,
                )),
            )?;
            SetPropW(
                window,
                w!("glaze.crop.y"),
                Some(windows::Win32::Foundation::HANDLE(
                    (rect.y - outer.y).round() as usize as *mut _,
                )),
            )?;
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(window);
            Ok(snapshot)
        }
    }
}
impl Drop for Snapshot {
    fn drop(&mut self) {
        /* SAFETY: our own window and bitmap, released on the creating thread. */
        unsafe {
            let _ = DestroyWindow(self.window);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
        }
    }
}
unsafe extern "system" fn snapshot_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        // SAFETY: WM_PAINT bracket; bitmap lives until the window is destroyed.
        unsafe {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(h, &mut paint);
            let memory = CreateCompatibleDC(Some(dc));
            let bitmap = HBITMAP(GetWindowLongPtrW(h, GWLP_USERDATA) as *mut _);
            let old = SelectObject(memory, HGDIOBJ(bitmap.0));
            let _ = BitBlt(
                dc,
                0,
                0,
                paint.rcPaint.right,
                paint.rcPaint.bottom,
                Some(memory),
                GetPropW(h, w!("glaze.crop.x")).0 as usize as i32,
                GetPropW(h, w!("glaze.crop.y")).0 as usize as i32,
                SRCCOPY,
            );
            SelectObject(memory, old);
            let _ = DeleteDC(memory);
            let _ = EndPaint(h, &paint);
        }
        return LRESULT(0);
    }
    // SAFETY: default handling of the original message.
    unsafe { DefWindowProcW(h, msg, w, l) }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_cropping_preserves_thumbnail_scale() {
        let destination = Rect {
            x: 300.0,
            y: 100.0,
            w: 800.0,
            h: 400.0,
        };
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            w: 600.0,
            h: 1000.0,
        };
        let source = Rect {
            x: 7.0,
            y: 10.0,
            w: 400.0,
            h: 200.0,
        };
        let (dest, source) = crop_projection(destination, clip, source).unwrap();
        assert_eq!(
            dest,
            Rect {
                x: 300.0,
                y: 100.0,
                w: 300.0,
                h: 400.0
            }
        );
        assert_eq!(
            source,
            Rect {
                x: 7.0,
                y: 10.0,
                w: 150.0,
                h: 200.0
            }
        );
        assert!(crop_projection(destination, Rect { x: 1200.0, ..clip }, source).is_none());
    }
    #[test]
    fn critically_damped_spring_is_monotonic_and_settles() {
        let mut last = 0.0;
        for step in 0..1000 {
            let value = progress(f64::from(step) / 1000.0);
            assert!(value >= last && value <= 1.0);
            last = value;
        }
        assert!(last > 0.999);
    }
}
