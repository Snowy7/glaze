//! Glass overlays (plan: "Liquid Glass everywhere" 2 and 4): Liquid Glass under shell surfaces
//! that are not over the engine's own frame — flyouts, menus and bars over app windows, and
//! widgets over web and app wallpapers.
//!
//! For each open surface the engine owns a click-through window directly under the shell's
//! window (`window.rs`) and draws Liquid Glass into it (`glass.rs`) over a crop of what is behind
//! it, captured by one Windows.Graphics.Capture session per monitor (`capture.rs`) that keeps
//! our windows and the shell's out of the picture (`abi.rs`).
//!
//! **When it draws.** Only when the pixels behind a surface change:
//! 1. On WGC's thread, frames rendered before the current exclusion list, or whose dirty rects
//!    miss every overlay's frame, are dropped (a video elsewhere costs only WGC's delivery).
//! 2. The capture reports dirty rects where an *excluded* window repaints or moves, and where an
//!    animated wallpaper changes behind an opaque window, though the pixels it captures there
//!    are unchanged. So a frame that touches an overlay is copied into that overlay's spare crop
//!    and compared with the shown one on the GPU; only a crop that differs is drawn. That is
//!    what stops the glass redrawing itself (its own presents, the popup's hover and clock
//!    repaints): one compare per such frame, no glass pass.
//! 3. A new or moved overlay takes the next frame whatever its dirty rects, and draws it
//!    without comparing (excluded windows appearing or moving always produce one).
//!
//! **Moving.** A `Place` moves the overlay window inside the request's `WM_COPYDATA`, before the
//! client moves itself, so both land in the same composition frame; the glass inside follows at
//! the next captured frame.
//!
//! **Memory and warmth.** Nothing exists until the first overlay opens. WGC's first frame takes
//! ~100-250 ms after a session starts, so the capture device, sessions and DirectComposition
//! device stay [`WARM`] after the last close (the pipelines stay for good): the next popup (a
//! submenu, the next flyout) gets its glass in a frame. A warm session with no overlay drops
//! every frame on WGC's thread. (A warm-up on pointer-down was measured and dropped: starting the capture while
//! the button is down stalled the click's delivery to the shell by ~30 ms.)
//!
//! Driving it: create [`Overlays`] on the engine's thread, pump that thread's messages (requests
//! and frame wakes arrive as window messages), call [`Overlays::tick`] after each pump, and wake
//! by [`Overlays::deadline`] (when the warm capture goes cold).

mod abi;
mod capture;
mod geometry;
mod glass;
pub mod request;
mod timing;
mod window;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use mypaper_desktop::Rect;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice3, IDCompositionDesktopDevice,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EVENT_OBJECT_DESTROY, GWLP_USERDATA,
    GetWindowLongPtrW, GetWindowThreadProcessId, HWND_MESSAGE, IsWindow, MSG, OBJID_WINDOW,
    PM_NOREMOVE, PM_QS_SENDMESSAGE, PeekMessageW,
    PostMessageW, RegisterClassExW, RegisterWindowMessageW, SetWindowDisplayAffinity,
    SetWindowLongPtrW, WDA_EXCLUDEFROMCAPTURE, WINEVENT_OUTOFCONTEXT, WM_APP, WM_COPYDATA,
    WNDCLASSEXW,
};
use windows::core::w;

pub use self::capture::Counts;
use self::capture::{CaptureDevice, CopyTimer, Session};
use self::glass::{Pipelines, Targets};
use self::request::{Layer, Material, OVERLAY_CLASS, Reply, Request};
use self::timing::Timer;
use self::window::OverlayWindow;

/// Posted to the request window by a capture thread when a frame waits.
const WM_FRAME: u32 = WM_APP + 0x4F0;
/// Posted by the window-destroy hook: a client may be gone.
const WM_PRUNE: u32 = WM_APP + 0x4F1;
/// Registered message posted to a client whose window must exclude itself from capture
/// (`WDA_EXCLUDEFROMCAPTURE`), when this Windows has no per-session exclusion list.
pub const EXCLUDE_YOURSELF: windows::core::PCWSTR = w!("mypaper.glass.exclude-yourself");
/// How long the capture stays running after the last overlay closes.
pub const WARM: Duration = Duration::from_secs(3);

/// The engine's GPU, which draws every overlay (plan, Rule 1).
#[derive(Clone)]
pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// What the overlays did, for measuring.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    /// Capture frames by fate (summed over sessions, including closed ones).
    pub frames: Counts,
    /// Frames the engine's thread processed.
    pub updates: u64,
    /// Fresh crops compared with the shown one, and those that were the same.
    pub compares: u64,
    pub unchanged: u64,
    /// Glass passes drawn (each one shown).
    pub passes: u64,
    /// GPU ms of the copies, compares and glass passes, when timing is on.
    pub gpu_ms: f64,
    pub timed: u64,
    /// `Place` requests that moved the overlay window inside the client's `SendMessage`.
    pub moved_at_once: u64,
    /// From the last `Open` to its first glass on screen.
    pub open_to_glass: Option<Duration>,
    /// Whether the overlays fall back to `WDA_EXCLUDEFROMCAPTURE`.
    pub excluding_by_affinity: bool,
}

/// Requests and wakes, filled by the request window's procedure.
#[derive(Default)]
struct Inbox {
    requests: RefCell<VecDeque<Request>>,
    frames: Cell<bool>,
    prune: Cell<bool>,
    /// Whether clients must exclude themselves (no per-session exclusion list).
    affinity: Cell<bool>,
    /// Where each overlay is, so a `Place` can move its window at once (see the module docs).
    placements: RefCell<Vec<Placement>>,
    moved_at_once: Cell<u64>,
}

/// An open overlay's window and geometry, as the request window's procedure needs them.
#[derive(Clone, Copy)]
struct Placement {
    client: HWND,
    overlay: HWND,
    frame: Rect,
    bounds: Rect,
    material: Material,
    /// The monitor's px per point.
    scale: f32,
}

pub struct Overlays {
    // Drop order: the window first (its procedure points at `inbox`).
    window: MessageWindow,
    inbox: Rc<Inbox>,
    gpu: Gpu,
    open: Vec<Overlay>,
    sessions: Vec<Session>,
    live: Option<Live>,
    hooks: Vec<(u32, Hook)>,
    /// Whether the exclusion lists must be set again (an overlay opened or closed).
    exclusion_changed: bool,
    /// Frame counts of sessions already closed.
    closed_counts: Counts,
    stats: Stats,
    /// Whether to time the GPU work (waits for the GPU after every update).
    timing: bool,
    /// Until when sessions without an overlay (and, with none open, `live`) are kept.
    warm_until: Option<Instant>,
    /// The glass and compare pipelines, built at the first open and kept (small, and their
    /// shader compile is the slowest part of a cold open).
    pipes: Option<Rc<Pipelines>>,
}

/// What exists only while an overlay is open, or the capture is warm.
struct Live {
    capture: CaptureDevice,
    dcomp: IDCompositionDesktopDevice,
    pipes: Rc<Pipelines>,
    timer: Option<Timer>,
    copy_timer: Option<CopyTimer>,
}

struct Overlay {
    client: HWND,
    /// The client surface, virtual px.
    rect: Rect,
    radius: f32,
    material: Material,
    monitor: HMONITOR,
    bounds: Rect,
    /// The monitor's px per point.
    scale: f32,
    /// The window: `rect` grown by the glass's reach, on its monitor.
    frame: Rect,
    window: OverlayWindow,
    targets: Targets,
    /// Needs the next frame drawn as is (new, or moved since its crop).
    fresh: bool,
    /// What is behind it changed at the last compare (so it probably still is).
    changing: bool,
    /// When its glass first showed (and began fading in), and whether the fade is over and its
    /// animation replaced by a plain opacity.
    shown: Option<Instant>,
    faded: bool,
    opened: Instant,
}

impl Overlays {
    /// Creates the request window (`OVERLAY_CLASS`) on this thread. Nothing else exists until
    /// an overlay opens.
    pub fn new(gpu: Gpu) -> Result<Self> {
        capture::ask_borderless();
        let inbox = Rc::new(Inbox::default());
        let window = MessageWindow::new(&inbox)?;
        Ok(Self {
            window,
            inbox,
            gpu,
            open: Vec::new(),
            sessions: Vec::new(),
            live: None,
            hooks: Vec::new(),
            exclusion_changed: false,
            closed_counts: Counts::default(),
            stats: Stats::default(),
            // Measuring only: with the device's timestamp queries (MYPAPER_GPU_TIMINGS).
            timing: std::env::var_os("MYPAPER_OVERLAY_TIMINGS").is_some(),
            warm_until: None,
            pipes: None,
        })
    }

    /// When the warm capture goes cold: the loop must call [`tick`](Self::tick) by then.
    pub fn deadline(&self) -> Option<Instant> {
        self.warm_until
    }

    /// Times the GPU work of every update (measuring only: it waits for the GPU).
    pub fn set_timing(&mut self, on: bool) {
        self.timing = on;
    }

    pub fn stats(&self) -> Stats {
        let mut stats = self.stats;
        stats.moved_at_once = self.inbox.moved_at_once.get();
        stats.frames = self
            .sessions
            .iter()
            .map(Session::counts)
            .fold(self.closed_counts, Counts::plus);
        stats
    }

    /// How many overlays are open, and on how many capture sessions.
    pub fn counts(&self) -> (usize, usize) {
        (self.open.len(), self.sessions.len())
    }

    /// What `client`'s glass currently samples: its frame (virtual px) and the shown crop as
    /// tightly packed BGRA. Waits for the GPU; for checks (is the client really excluded?).
    pub fn backdrop(&self, client: HWND) -> Option<(Rect, Vec<u8>)> {
        let overlay = &self.open[self.find(client)?];
        let pixels = overlay
            .targets
            .read_shown(&self.gpu.device, &self.gpu.queue);
        Some((overlay.frame, pixels))
    }

    /// Handles what arrived since the last call: requests, gone clients, waiting frames.
    /// Returns how many requests it handled.
    pub fn tick(&mut self) -> usize {
        // A cold open takes the shell's sent requests while it works (see 	ake_sent); those
        // run here, after it, in the order they came.
        let mut handled = 0;
        loop {
            let requests = std::mem::take(&mut *self.inbox.requests.borrow_mut());
            if requests.is_empty() {
                break;
            }
            handled += requests.len();
            for request in requests {
                if let Err(e) = self.handle(request) {
                    log::warn!("glass overlay {request:?}: {e:#}");
                }
            }
        }
        if self.inbox.prune.take() {
            let gone: Vec<HWND> = self
                .open
                .iter()
                // SAFETY: a read-only query.
                .filter(|o| !unsafe { IsWindow(Some(o.client)) }.as_bool())
                .map(|o| o.client)
                .collect();
            for client in gone {
                log::info!("glass overlay: client {client:?} is gone");
                self.close(client);
            }
        }
        if handled > 0 {
            self.sync_placements();
        }
        if self.inbox.frames.take() {
            for index in 0..self.sessions.len() {
                if let Some(frame) = self.sessions[index].take()
                    && let Err(e) = self.update(index, frame)
                {
                    log::warn!("glass overlay update: {e:#}");
                }
            }
        }
        if self.warm_until.is_some_and(|at| at <= Instant::now()) {
            self.warm_until = None;
            self.cool();
        }
        handled
    }

    /// Ends what only the warmth kept: sessions with no overlay, and with none open, the rest.
    fn cool(&mut self) {
        self.drop_idle_sessions();
        if self.open.is_empty() {
            self.hooks.clear();
            // The capture device, DirectComposition and the pipelines go with the warmth.
            self.live = None;
            log::debug!("glass overlays: capture cold");
        }
    }

    /// Keeps the capture running for [`WARM`] from now.
    fn keep_warm(&mut self) {
        self.warm_until = Some(Instant::now() + WARM);
    }

    /// Handles one request now (the window queues them for [`tick`](Self::tick)).
    pub fn handle(&mut self, request: Request) -> Result<()> {
        match request {
            Request::Open {
                client,
                layer,
                rect,
                radius,
                material,
            } => self.open(client, layer, rect, radius, material),
            Request::Place {
                client,
                rect,
                radius,
            } => {
                let Some(o) = self.find(client) else {
                    return Ok(());
                };
                let material = self.open[o].material;
                self.place(o, rect, radius, material)
            }
            Request::Material { client, material } => {
                let Some(o) = self.find(client) else {
                    return Ok(());
                };
                let (rect, radius) = (self.open[o].rect, self.open[o].radius);
                self.place(o, rect, radius, material)?;
                // The shown crop still holds what is behind: redraw at once.
                if self.open[o].shown.is_some() && !self.open[o].fresh {
                    self.draw(&[o])?;
                }
                Ok(())
            }
            Request::Close { client } => {
                self.close(client);
                Ok(())
            }
        }
    }

    fn find(&self, client: HWND) -> Option<usize> {
        self.open.iter().position(|o| o.client == client)
    }

    fn open(
        &mut self,
        client: HWND,
        layer: Layer,
        rect: Rect,
        radius: f32,
        material: Material,
    ) -> Result<()> {
        if let Some(o) = self.find(client) {
            return self.place(o, rect, radius, material);
        }
        let opened = Instant::now();
        let (monitor, bounds) = monitor_of(rect);
        if self.live.is_none() {
            let capture = CaptureDevice::new(&self.gpu.adapter, &self.gpu.device)?;
            // Capture first: its first frame takes ~300 ms, the rest is built meanwhile.
            self.start_session(&capture, monitor, bounds)?;
            // Compiled once per engine: rebuilding them made every cold open ~100 ms slower.
            let pipes = self
                .pipes
                .get_or_insert_with(|| Rc::new(Pipelines::new(&self.gpu.device)))
                .clone();
            self.live = Some(Live::new(&self.gpu, capture, pipes, self.timing)?);
            take_sent();
        }
        if !self.sessions.iter().any(|s| s.monitor == monitor) {
            let live = self.live.take().context("no overlays")?;
            let started = self.start_session(&live.capture, monitor, bounds);
            self.live = Some(live);
            started?;
        }
        let live = self.live.as_ref().context("no overlays")?;
        let scale = scale_of(bounds);
        let frame = geometry::frame(rect, geometry::reach(rect, &material, scale), bounds);
        let mut window = OverlayWindow::new(&live.dcomp, layer, frame)?;
        window.place(
            &live.dcomp,
            client,
            frame,
            geometry::local(rect, frame),
            radius,
        )?;
        let targets = Targets::new(&self.gpu.device, &live.pipes, &live.capture, size(frame))?;
        // SAFETY: a commit on a live device.
        unsafe { live.dcomp.Commit()? };
        if self.inbox.affinity.get() {
            // SAFETY: our own window.
            unsafe { SetWindowDisplayAffinity(window.hwnd(), WDA_EXCLUDEFROMCAPTURE)? };
            notify_exclude_yourself(client);
        }
        self.open.push(Overlay {
            client,
            rect,
            radius,
            material,
            monitor,
            bounds,
            scale,
            frame,
            window,
            targets,
            fresh: true,
            changing: false,
            shown: None,
            faded: false,
            opened,
        });
        self.watch_client(client);
        self.exclusion_changed = true;
        self.sync_capture(Some(monitor))
    }

    /// Starts capturing `monitor`; the first one tells whether this Windows can exclude windows
    /// per session, or overlays fall back to window affinity.
    fn start_session(
        &mut self,
        capture: &CaptureDevice,
        monitor: HMONITOR,
        bounds: Rect,
    ) -> Result<()> {
        let session = Session::start(capture, monitor, bounds, self.window.0, WM_FRAME)?;
        if !session.excludes() && !self.inbox.affinity.get() {
            log::error!(
                "glass overlays: this Windows has no per-session capture exclusion list \
                 (IDisplayGraphicsCaptureSession); falling back to WDA_EXCLUDEFROMCAPTURE, so \
                 open overlays and their popups vanish from screenshots"
            );
            self.inbox.affinity.set(true);
            self.stats.excluding_by_affinity = true;
        }
        self.sessions.push(session);
        self.exclusion_changed = true;
        Ok(())
    }

    /// Moves, resizes or re-materials overlay `o`.
    fn place(&mut self, o: usize, rect: Rect, radius: f32, material: Material) -> Result<()> {
        let (monitor, bounds) = monitor_of(rect);
        let scale = if monitor == self.open[o].monitor {
            self.open[o].scale
        } else {
            scale_of(bounds)
        };
        let frame = geometry::frame(rect, geometry::reach(rect, &material, scale), bounds);
        let overlay = &mut self.open[o];
        overlay.scale = scale;
        let moved = frame != overlay.frame || monitor != overlay.monitor;
        let reshaped = moved || rect != overlay.rect || radius != overlay.radius;
        (overlay.rect, overlay.radius, overlay.material) = (rect, radius, material);
        if !reshaped {
            return Ok(());
        }
        let live = self.live.as_ref().context("no overlays")?;
        if size(frame) != overlay.targets.size() {
            overlay.targets =
                Targets::new(&self.gpu.device, &live.pipes, &live.capture, size(frame))?;
        }
        overlay.window.place(
            &live.dcomp,
            overlay.client,
            frame,
            geometry::local(rect, frame),
            radius,
        )?;
        // SAFETY: a commit on a live device.
        unsafe { live.dcomp.Commit()? };
        overlay.frame = frame;
        if !moved {
            return Ok(());
        }
        overlay.fresh = true;
        overlay.bounds = bounds;
        let previous = std::mem::replace(&mut overlay.monitor, monitor);
        if previous != monitor {
            if !self.sessions.iter().any(|s| s.monitor == monitor) {
                let live = self.live.take().context("no overlays")?;
                let started = self.start_session(&live.capture, monitor, bounds);
                self.live = Some(live);
                started?;
            }
            self.drop_idle_sessions();
        }
        self.sync_capture(Some(monitor))
    }

    fn close(&mut self, client: HWND) {
        let Some(o) = self.find(client) else {
            return;
        };
        drop(self.open.remove(o));
        // Totals since the engine started (a harness diffs consecutive lines).
        let s = self.stats();
        let gpu = if s.timed > 0 {
            s.gpu_ms / s.timed as f64
        } else {
            0.0
        };
        log::info!(
            "glass overlay closed; totals: {} updates, {} compares ({} unchanged), {} passes, \
             {gpu:.3} ms GPU per timed update, frames {:?}",
            s.updates,
            s.compares,
            s.unchanged,
            s.passes,
            s.frames,
        );
        self.exclusion_changed = true;
        // The next popup usually follows within seconds (a submenu, the next flyout): keep the
        // capture running, so its glass is there in a frame instead of ~260 ms.
        self.keep_warm();
        if let Err(e) = self.sync_capture(None) {
            log::warn!("glass overlays: {e:#}");
        }
    }

    /// Ends the sessions no overlay uses, unless the capture is being kept warm.
    fn drop_idle_sessions(&mut self) {
        let warm = self.warm_until.is_some_and(|at| at > Instant::now());
        let open = &self.open;
        let closed = &mut self.closed_counts;
        self.sessions.retain(|s| {
            let used = warm || open.iter().any(|o| o.monitor == s.monitor);
            if !used {
                *closed = closed.plus(s.counts());
            }
            used
        });
    }

    /// Gives each session its overlays' frames (`fresh` for `monitor`: take the next frame),
    /// and, after an open or close, everyone's windows to exclude.
    fn sync_capture(&mut self, fresh: Option<HMONITOR>) -> Result<()> {
        let excluded: Vec<HWND> = self
            .open
            .iter()
            .flat_map(|o| [o.window.hwnd(), o.client])
            .collect();
        for session in &self.sessions {
            let frames = self
                .open
                .iter()
                .filter(|o| o.monitor == session.monitor)
                .map(|o| o.frame);
            session.set_regions(frames, fresh == Some(session.monitor));
            if self.exclusion_changed {
                session.exclude(&excluded)?;
            }
        }
        self.exclusion_changed = false;
        Ok(())
    }

    /// Tells the request window where every overlay is now.
    fn sync_placements(&self) {
        *self.inbox.placements.borrow_mut() = self
            .open
            .iter()
            .map(|o| Placement {
                client: o.client,
                overlay: o.window.hwnd(),
                frame: o.frame,
                bounds: o.bounds,
                material: o.material,
                scale: o.scale,
            })
            .collect();
    }

    /// Takes one session's frame: copies it under the overlays it touches, keeps the crops
    /// that changed, and shows those overlays' glass.
    ///
    /// One GPU round trip: the compares, and the glass of overlays that will surely change (new
    /// or moved) or probably will (what is behind them changed last time too), go in one
    /// submission. A probable glass whose compare finds no change is simply not shown (it would
    /// look the same), so a still backdrop costs no glass pass; only a change after a still
    /// spell takes a second submission.
    fn update(
        &mut self,
        index: usize,
        frame: windows::Graphics::Capture::Direct3D11CaptureFrame,
    ) -> Result<()> {
        self.stats.updates += 1;
        let (monitor, bounds) = (self.sessions[index].monitor, self.sessions[index].bounds);
        let (texture, dirty) = capture::open_frame(&frame)?;
        let hits: Vec<usize> = (0..self.open.len())
            .filter(|&o| {
                let overlay = &self.open[o];
                let frame = geometry::local(overlay.frame, bounds);
                overlay.monitor == monitor
                    && (overlay.fresh || dirty.iter().any(|d| geometry::touches(*d, frame)))
            })
            .collect();
        if hits.is_empty() {
            return Ok(());
        }
        let live = self.live.as_mut().context("no overlays")?;
        if let Some(t) = &live.copy_timer {
            t.begin();
        }
        for &o in &hits {
            let overlay = &self.open[o];
            live.capture.copy(
                &texture,
                geometry::local(overlay.frame, bounds),
                overlay.targets.fresh(),
            );
        }
        live.capture.hand_to_wgpu()?;
        if let Some(t) = &live.copy_timer {
            t.end();
        }

        let compared: Vec<usize> = hits
            .iter()
            .copied()
            .filter(|&o| !self.open[o].fresh)
            .collect();
        // Drawn now, from the fresh crop: certain (fresh) and probable (still changing).
        let early: Vec<usize> = hits
            .iter()
            .copied()
            .filter(|&o| self.open[o].fresh || self.open[o].changing)
            .collect();
        let device = &self.gpu.device;
        let mut encoder = device.create_command_encoder(&Default::default());
        if !compared.is_empty() {
            for &o in &compared {
                self.open[o].targets.clear_count(&mut encoder);
            }
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("glass overlay compare"),
                    timestamp_writes: live.timer.as_mut().and_then(Timer::compute_pass),
                });
                for &o in &compared {
                    self.open[o].targets.compare(&mut pass, &live.pipes);
                }
            }
            for &o in &compared {
                self.open[o].targets.read_count(&mut encoder);
            }
        }
        render(
            &mut self.open,
            &early,
            true,
            &self.gpu,
            &mut encoder,
            live,
        );
        if let Some(t) = &live.timer {
            t.resolve(&mut encoder);
        }
        self.gpu.queue.submit([encoder.finish()]);
        for &o in &compared {
            self.open[o].targets.map_count();
        }
        // Also makes sure the copies are done before the frame goes back to the pool.
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        drop(frame);
        if let Some(t) = &mut live.timer {
            self.stats.gpu_ms += t.read(device);
        }
        if let Some(ms) = live.copy_timer.as_ref().and_then(CopyTimer::read) {
            self.stats.gpu_ms += ms;
        }

        let mut changed = Vec::with_capacity(hits.len());
        let mut late = Vec::new();
        for &o in &hits {
            let overlay = &mut self.open[o];
            let differ = if overlay.fresh {
                true
            } else {
                self.stats.compares += 1;
                overlay.targets.count() > 0
            };
            if differ && !early.contains(&o) {
                late.push(o);
            }
            if differ {
                overlay.targets.accept();
                changed.push(o);
            } else {
                self.stats.unchanged += 1;
            }
            overlay.fresh = false;
            overlay.changing = differ;
        }
        if !late.is_empty() {
            let mut encoder = device.create_command_encoder(&Default::default());
            render(
                &mut self.open,
                &late,
                false,
                &self.gpu,
                &mut encoder,
                live,
            );
            if let Some(t) = &live.timer {
                t.resolve(&mut encoder);
            }
            self.gpu.queue.submit([encoder.finish()]);
            if let Some(t) = &mut live.timer {
                self.stats.gpu_ms += t.read(device);
            }
        }
        self.show(&changed)?;
        if self.timing {
            self.stats.timed += 1;
        }
        Ok(())
    }

    /// Draws overlays `which` from their shown crops and shows them (a material change).
    fn draw(&mut self, which: &[usize]) -> Result<()> {
        let live = self.live.as_mut().context("no overlays")?;
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        render(
            &mut self.open,
            which,
            false,
            &self.gpu,
            &mut encoder,
            live,
        );
        self.gpu.queue.submit([encoder.finish()]);
        self.show(which)
    }

    /// Shows the glass wgpu has drawn for overlays `which`; glass shown for the first time
    /// starts fading in.
    fn show(&mut self, which: &[usize]) -> Result<()> {
        if which.is_empty() {
            return Ok(());
        }
        let live = self.live.as_mut().context("no overlays")?;
        live.capture.wait_for_wgpu()?;
        for &o in which {
            let overlay = &mut self.open[o];
            overlay
                .window
                .show(&live.capture, overlay.targets.glass())?;
            match overlay.shown {
                None => {
                    overlay.window.fade_in(&live.dcomp)?;
                    overlay.shown = Some(Instant::now());
                    self.stats.open_to_glass = Some(overlay.opened.elapsed());
                    log::info!(
                        "glass overlay: open to glass {:.1} ms",
                        overlay.opened.elapsed().as_secs_f64() * 1000.0
                    );
                }
                Some(at) if !overlay.faded && at.elapsed() > window::FADE => {
                    overlay.window.settle()?;
                    overlay.faded = true;
                }
                Some(_) => {}
            }
        }
        // SAFETY: a commit on a live device: the new glass shows at the next composition.
        unsafe { live.dcomp.Commit()? };
        self.stats.passes += which.len() as u64;
        Ok(())
    }

    /// Hooks window destruction in the client's process, so a client that dies without closing
    /// its overlay doesn't leave the glass behind.
    fn watch_client(&mut self, client: HWND) {
        let mut pid = 0;
        // SAFETY: a read-only query.
        unsafe { GetWindowThreadProcessId(client, Some(&mut pid)) };
        if pid == 0 || self.hooks.iter().any(|(p, _)| *p == pid) {
            return;
        }
        PRUNE_TARGET.with(|t| t.set(self.window.0.0 as isize));
        // SAFETY: an out-of-context hook; its callback runs on this thread's message loop.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_DESTROY,
                EVENT_OBJECT_DESTROY,
                None,
                Some(on_destroy),
                pid,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if !hook.is_invalid() {
            self.hooks.push((pid, Hook(hook)));
        }
    }
}

impl Live {
    fn new(gpu: &Gpu, capture: CaptureDevice, pipes: Rc<Pipelines>, timing: bool) -> Result<Self> {
        // SAFETY: a DirectComposition device drawing surfaces with the capture device.
        let dcomp: IDCompositionDesktopDevice =
            unsafe { DCompositionCreateDevice3(&capture.dxgi()?)? };
        let (timer, copy_timer) = if timing {
            (
                Timer::new(&gpu.device, &gpu.queue),
                Some(capture.copy_timer()?),
            )
        } else {
            (None, None)
        };
        Ok(Self {
            pipes,
            capture,
            dcomp,
            timer,
            copy_timer,
        })
    }
}

thread_local! {
    /// The request window the destroy hook wakes (hooks have no context pointer).
    static PRUNE_TARGET: Cell<isize> = const { Cell::new(0) };
}

unsafe extern "system" fn on_destroy(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if object != OBJID_WINDOW.0 || child != 0 {
        return;
    }
    let target = PRUNE_TARGET.with(Cell::get);
    // SAFETY: posting to our own window.
    let _ = unsafe { PostMessageW(Some(HWND(target as *mut _)), WM_PRUNE, WPARAM(0), LPARAM(0)) };
}

struct Hook(HWINEVENTHOOK);

impl Drop for Hook {
    fn drop(&mut self) {
        // SAFETY: our own hook.
        let _ = unsafe { UnhookWinEvent(self.0) };
    }
}

/// Records the glass of overlays `which` (from their fresh crops with `fresh`, else from their
/// shown ones) into each one's glass texture.
fn render(
    open: &mut [Overlay],
    which: &[usize],
    fresh: bool,
    gpu: &Gpu,
    encoder: &mut wgpu::CommandEncoder,
    live: &mut Live,
) {
    for &o in which {
        let overlay = &mut open[o];
        overlay.targets.set_plate(
            geometry::local(overlay.rect, overlay.frame),
            overlay.radius,
            &overlay.material,
            overlay.scale,
        );
        overlay.targets.draw(
            &gpu.device,
            &gpu.queue,
            encoder,
            fresh,
            live.timer.as_mut().and_then(Timer::render_pass),
        );
    }
}

/// The px per point of the monitor at `bounds` (its DPI / 96).
fn scale_of(bounds: Rect) -> f32 {
    mypaper_desktop::monitor_infos()
        .iter()
        .find(|m| m.rect == bounds)
        .map_or(1.0, |m| m.dpi as f32 / 96.0)
}

fn size(frame: Rect) -> (u32, u32) {
    (frame.width.max(1) as u32, frame.height.max(1) as u32)
}

/// The monitor most of `rect` is on, and its bounds.
fn monitor_of(rect: Rect) -> (HMONITOR, Rect) {
    let r = RECT {
        left: rect.x,
        top: rect.y,
        right: rect.x + rect.width,
        bottom: rect.y + rect.height,
    };
    // SAFETY: plain monitor queries; `info` outlives the call.
    unsafe {
        let monitor = MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let m = info.rcMonitor;
        (
            monitor,
            Rect {
                x: m.left,
                y: m.top,
                width: m.right - m.left,
                height: m.bottom - m.top,
            },
        )
    }
}

/// Tells `client` to set `WDA_EXCLUDEFROMCAPTURE` on itself ([`EXCLUDE_YOURSELF`]).
fn notify_exclude_yourself(client: HWND) {
    // SAFETY: a registered message to a window that may belong to another process.
    unsafe {
        let message = RegisterWindowMessageW(EXCLUDE_YOURSELF);
        let _ = PostMessageW(Some(client), message, WPARAM(0), LPARAM(0));
    }
}

/// The message-only window requests and wakes arrive at.
struct MessageWindow(HWND);

impl MessageWindow {
    fn new(inbox: &Rc<Inbox>) -> Result<Self> {
        // SAFETY: plain registration and creation; the window's user data points at `inbox`,
        // which `Overlays` keeps alive until after the window is destroyed (field order).
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(inbox_proc),
                hInstance: instance.into(),
                lpszClassName: OVERLAY_CLASS,
                ..Default::default()
            };
            RegisterClassExW(&class);
            let hwnd = CreateWindowExW(
                Default::default(),
                OVERLAY_CLASS,
                w!("mypaper glass overlays"),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
            .context("CreateWindowExW (overlay requests)")?;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::as_ptr(inbox) as isize);
            Ok(Self(hwnd))
        }
    }
}

impl Drop for MessageWindow {
    fn drop(&mut self) {
        // SAFETY: our own window, on its thread.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

/// Takes the requests the shell is sending now, into the queue (`tick` runs them next). A cold
/// open blocks this thread longer than the shell waits for each request (50 ms), so without
/// this a `Close` or `Place` sent meanwhile was refused and lost: the glass stayed open.
fn take_sent() {
    let mut msg = MSG::default();
    // SAFETY: only dispatches messages sent to this thread's windows; nothing is removed.
    let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE | PM_QS_SENDMESSAGE) };
}

/// Moves `request`'s overlay window now if it is a `Place` that only moves it (same size, same
/// monitor): the client sends it just before moving itself, so both land in one frame.
fn move_now(inbox: &Inbox, request: &Request) {
    let Request::Place {
        client,
        rect,
        radius: _,
    } = *request
    else {
        return;
    };
    let mut placements = inbox.placements.borrow_mut();
    let Some(p) = placements.iter_mut().find(|p| p.client == client) else {
        return;
    };
    let frame = geometry::frame(rect, geometry::reach(rect, &p.material, p.scale), p.bounds);
    let same_size = (frame.width, frame.height) == (p.frame.width, p.frame.height);
    if same_size && frame != p.frame && window::move_under(p.overlay, client, frame, true).is_ok() {
        p.frame = frame;
        inbox.moved_at_once.set(inbox.moved_at_once.get() + 1);
    }
}

unsafe extern "system" fn inbox_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the user data is the `Inbox` that outlives this window, or 0 while it is created.
    let inbox = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Inbox).as_ref() };
    match (msg, inbox) {
        (WM_COPYDATA, Some(inbox)) => {
            // SAFETY: a WM_COPYDATA's lParam.
            let reply = match unsafe { request::decode(lparam) } {
                Some(request) => {
                    move_now(inbox, &request);
                    inbox.requests.borrow_mut().push_back(request);
                    if inbox.affinity.get() {
                        Reply::TakenExcludeYourself
                    } else {
                        Reply::Taken
                    }
                }
                None => Reply::Rejected,
            };
            LRESULT(reply.to_lresult())
        }
        (WM_FRAME, Some(inbox)) => {
            inbox.frames.set(true);
            LRESULT(0)
        }
        (WM_PRUNE, Some(inbox)) => {
            inbox.prune.set(true);
            LRESULT(0)
        }
        // SAFETY: the system's own message, forwarded unchanged.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
