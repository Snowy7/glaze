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
//! **Moving.** The shell publishes its latest geometry and material in a shared mailbox and
//! posts one wake. The engine takes at most one snapshot per display frame; repeated geometry
//! costs nothing and a busy/cold engine never blocks an animating shell. Open and close stay
//! ordered in the request queue, including requests received while a cold open is being built.
//!
//! **Memory and warmth.** Nothing exists until the first overlay opens. WGC's first frame takes
//! ~100-250 ms after a session starts, so the capture device, sessions and DirectComposition
//! device stay [`WARM`] after the last close (the pipelines stay for good): the next popup (a
//! submenu, the next flyout) gets its glass in a frame. A warm session with no overlay drops
//! every frame on WGC's thread. (A warm-up on pointer-down was measured and dropped: starting the capture while
//! the button is down stalled the click's delivery to the shell by ~30 ms.)
//!
//! **Plates over web and app wallpapers (P14).** Those wallpapers draw with windows of their
//! own, so the engine has no frame to draw the shell's plates into. Their plates become overlays
//! the engine opens itself ([`Overlays::sync_plates`], keyed by plate id, apart from the shell's
//! own requests for the same window): a widget's under it in the desktop band (`Layer::Desktop`),
//! a bar's topmost (`Layer::Popup`). The capture follows the wallpaper's own frames like any
//! backdrop, capped by the theme's fps ([`Overlays::set_fps`]). On a covered or held screen they
//! are *frozen*: the last glass stays on screen and nothing is captured for them; a monitor with
//! only frozen overlays has no session.
//!
//! Driving it: create [`Overlays`] on the engine's thread, pump that thread's messages (requests
//! and frame wakes arrive as window messages), call [`Overlays::tick`] after each pump, and wake
//! by [`Overlays::deadline`] (when the warm capture goes cold).

mod abi;
mod capture;
mod geometry;
mod glass;
mod recovery;
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
    DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW, GetMonitorInfoW, HMONITOR,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EVENT_OBJECT_DESTROY, GWL_EXSTYLE,
    GWLP_USERDATA, GetWindowLongPtrW, GetWindowThreadProcessId, HWND_MESSAGE, IsWindow, MSG,
    OBJID_WINDOW, PM_NOREMOVE, PM_QS_SENDMESSAGE, PeekMessageW, PostMessageW, RegisterClassExW,
    RegisterWindowMessageW, SetWindowDisplayAffinity, SetWindowLongPtrW, WDA_EXCLUDEFROMCAPTURE,
    WINEVENT_OUTOFCONTEXT, WM_APP, WM_COPYDATA, WNDCLASSEXW, WS_EX_TOPMOST,
};
use windows::core::{PCWSTR, w};

pub use self::capture::Counts;
use self::capture::{CaptureDevice, CopyTimer, Session};
use self::glass::{Pipelines, Targets};
use self::request::{Channel, Layer, Material, OVERLAY_CLASS, Reply, Request, Style, WM_PLACEMENT};
use self::timing::Timer;
use self::window::OverlayWindow;

/// Posted to the request window by a capture thread when a frame waits.
const WM_FRAME: u32 = WM_APP + 0x4F0;
/// Posted by the window-destroy hook: a client may be gone.
const WM_PRUNE: u32 = WM_APP + 0x4F1;
/// Opt-in, input-free live regression fixture. 1 loses capture; 2 removes our shared DX12
/// device. Ignored unless MYPAPER_TEST_OVERLAY_RECOVERY was set at process start.
const WM_TEST_RECOVERY: u32 = WM_APP + 0x4F3;
/// Registered message posted to a client whose window must exclude itself from capture
/// (`WDA_EXCLUDEFROMCAPTURE`), when this Windows has no per-session exclusion list.
pub const EXCLUDE_YOURSELF: windows::core::PCWSTR = w!("mypaper.glass.exclude-yourself");
/// How long the capture stays running after the last overlay closes.
pub const WARM: Duration = Duration::from_secs(3);

/// A shell plate the engine draws as an overlay, because the wallpaper under it is a web page or
/// an app ([`Overlays::sync_plates`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlateOverlay {
    /// The plate's id: one overlay per plate.
    pub id: u64,
    /// The surface window the plate belongs to, which the overlay sits directly under.
    pub client: HWND,
    pub layer: Layer,
    /// Virtual-screen physical px.
    pub rect: Rect,
    pub radius: f32,
    pub material: Material,
    pub style: Style,
    /// Whether what is behind it can change: false on a covered or held screen, where the glass
    /// keeps its last frame and nothing is captured for it.
    pub live: bool,
}

impl PlateOverlay {
    /// A published plate as an overlay. The low 32 bits of a plate's id are its surface's window
    /// (`mypaper_glass_ipc::Plate::id`); `None` unless that is a live window of the plate's
    /// publisher (`writer`, its process id), so a stale id never puts glass under another
    /// app's window. A topmost surface (a bar) gets a topmost overlay, a desktop one (a widget)
    /// one in the desktop band.
    pub fn of(
        plate: &mypaper_glass_ipc::Plate,
        style: &mypaper_glass_ipc::Style,
        writer: u32,
        live: bool,
    ) -> Option<Self> {
        let client = HWND(plate.id as u32 as usize as *mut _);
        let mut pid = 0;
        // SAFETY: read-only queries of a handle that may be stale.
        let topmost = unsafe {
            if !IsWindow(Some(client)).as_bool() {
                return None;
            }
            GetWindowThreadProcessId(client, Some(&mut pid));
            GetWindowLongPtrW(client, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0
        };
        if pid != writer {
            return None;
        }
        let [x, y, w, h] = plate.rect;
        let (left, top) = (x.round() as i32, y.round() as i32);
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let m = plate.material;
        Some(Self {
            id: plate.id,
            client,
            layer: if topmost {
                Layer::Popup
            } else {
                Layer::Desktop
            },
            rect: Rect {
                x: left,
                y: top,
                width: ((x + w).round() as i32 - left).max(1),
                height: ((y + h).round() as i32 - top).max(1),
            },
            radius: plate.radius.max(0.0),
            material: Material {
                clarity: m.clarity.clamp(0.0, 1.0),
                blur: m.blur.clamp(0.0, mypaper_glass_ipc::MAX_BLUR),
                refraction: m.refraction.clamp(0.0, 1.0),
                highlight: m.highlight.clamp(0.0, 1.0),
                tint: m.tint.map(byte),
            },
            style: Style {
                brightness: style.brightness,
                saturation: style.saturation,
                smoothing: style.smoothing,
                sheen: style.sheen.unwrap_or(-1.0),
                depth: style.depth.unwrap_or(-1.0),
                inner_shadow: style.inner_shadow,
                // A plate fades in like the shell's glass does by default.
                ..Style::DEFAULT
            },
            live,
        })
    }
}

/// The engine's GPU, which draws every overlay (plan, Rule 1).
#[derive(Clone)]
pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub fn removed_reason(&self) -> windows::core::HRESULT {
        // SAFETY: read-only diagnostic on our shared device, including after loss.
        unsafe {
            self.device
                .as_hal::<wgpu::hal::api::Dx12>()
                .and_then(|hal| hal.raw_device().GetDeviceRemovedReason().err())
                .map_or(windows::core::HRESULT(0), |e| e.code())
        }
    }
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
    diagnostics: bool,
    test_loss: Cell<usize>,
    requests: RefCell<VecDeque<Request>>,
    frames: Cell<bool>,
    prune: Cell<bool>,
    /// Whether clients must exclude themselves (no per-session exclusion list).
    affinity: Cell<bool>,
    /// Where each overlay is, so a `Place` can move its window at once (see the module docs).
    placements: RefCell<Vec<Placement>>,
    moved_at_once: Cell<u64>,
    motion: RefCell<Vec<Motion>>,
}

struct Motion {
    client: HWND,
    channel: Channel,
    last: Request,
    pending: bool,
    next: Instant,
    interval: Duration,
    bounds: Rect,
}

impl Inbox {
    fn receive(&self, request: Request) {
        // Lifecycle messages are synchronous, so a Close received during Open still queues
        // after it. Remove its mailbox now: an older posted wake cannot resurrect the glass.
        match request {
            Request::Open { client, rect, .. } => {
                let mut motion = self.motion.borrow_mut();
                motion.retain(|m| m.client != client);
                if let Ok(mut channel) = Channel::open(client, false) {
                    let _ = channel.take();
                    motion.push(Motion {
                        client,
                        channel,
                        last: request,
                        pending: false,
                        next: Instant::now(),
                        interval: display_interval(rect),
                        bounds: monitor_of(rect).1,
                    });
                }
            }
            Request::Close { client } => self.motion.borrow_mut().retain(|m| m.client != client),
            _ => {}
        }
        move_now(self, &request);
        self.requests.borrow_mut().push_back(request);
    }

    fn take_motion(&self, now: Instant) {
        for motion in self.motion.borrow_mut().iter_mut() {
            if !motion.pending || now < motion.next {
                continue;
            }
            motion.pending = false;
            let Some(latest @ Request::Open { client, rect, .. }) = motion.channel.take() else {
                continue;
            };
            if client != motion.client {
                continue;
            }
            let (x, y) = rect.center();
            if !motion.bounds.contains_point(x, y) {
                let bounds = monitor_of(rect).1;
                if bounds != motion.bounds {
                    motion.bounds = bounds;
                    motion.interval = display_interval(rect);
                }
            }
            if latest != motion.last {
                // Open is also the full-state update of an existing overlay: geometry and
                // material change together, with one placement/commit for this frame.
                self.requests.borrow_mut().push_back(latest);
            }
            motion.last = latest;
            motion.next = now + motion.interval;
        }
    }

    fn motion_deadline(&self) -> Option<Instant> {
        self.motion
            .borrow()
            .iter()
            .filter(|m| m.pending)
            .map(|m| m.next)
            .min()
    }
}

/// Read refresh rate at Open or a display crossing, outside normal frame updates.
fn display_interval(rect: Rect) -> Duration {
    let (monitor, _) = monitor_of(rect);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    // SAFETY: correctly sized, live output structs; the device name is zero terminated.
    let hz = unsafe {
        if GetMonitorInfoW(monitor, (&raw mut info).cast()).as_bool()
            && EnumDisplaySettingsW(
                PCWSTR(info.szDevice.as_ptr()),
                ENUM_CURRENT_SETTINGS,
                &mut mode,
            )
            .as_bool()
        {
            mode.dmDisplayFrequency
        } else {
            60
        }
    };
    Duration::from_secs_f64(1.0 / f64::from(if hz > 1 { hz } else { 60 }))
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
    gpu: crate::gpu::DeviceSlot<Gpu>,
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
    /// The shortest time between two captured frames ([`set_fps`](Self::set_fps)).
    interval: Duration,
    /// Last requests are independent of GPU objects, including an Open that failed midway.
    desired: recovery::Requests,
    plates: Vec<PlateOverlay>,
    retry_at: Option<Instant>,
    retry_delay: Duration,
    /// Shared DX12 loss must rebuild the wallpaper's device too, before replaying overlays.
    gpu_lost: bool,
    failure: Option<String>,
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
    layer: Layer,
    /// The plate it draws ([`Overlays::sync_plates`]); `None` for the shell's own requests.
    plate: Option<u64>,
    /// Shows its last glass and takes no frames (a plate on a covered or held screen).
    frozen: bool,
    /// The client surface, virtual px.
    rect: Rect,
    radius: f32,
    material: Material,
    /// The finer knobs and the fade, from the shell's `Style` request.
    style: Style,
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
    /// Whether recovery also needs a new engine device (capture-only loss does not).
    pub fn needs_gpu(&self) -> bool {
        self.gpu_lost
    }

    /// The engine replaced its lost DX12 device; rebuild capture and replay on the next tick.
    pub fn replace_gpu(&mut self, gpu: Gpu) {
        self.discard_device();
        self.gpu.replace(gpu);
        self.gpu_lost = false;
        self.retry_at = Some(Instant::now());
    }

    /// Release every reference to the shared device before the engine reopens its adapter.
    pub fn release_gpu(&mut self) {
        self.discard_device();
        self.gpu.clear();
        self.gpu_lost = true;
        self.retry_at.get_or_insert(Instant::now());
    }

    fn discard_device(&mut self) {
        for session in &self.sessions {
            self.closed_counts = self.closed_counts.plus(session.counts());
        }
        self.sessions.clear();
        self.open.clear();
        self.live = None;
        self.pipes = None;
        self.hooks.clear();
        self.inbox.placements.borrow_mut().clear();
        self.inbox.frames.set(false);
        self.warm_until = None;
        self.exclusion_changed = true;
    }

    /// Enter recovery once, rather than emitting a warning for every animation request.
    fn failed(&mut self, operation: &str, error: anyhow::Error) {
        if recovery::device_lost(&error) {
            let dx12 = self.gpu.removed_reason();
            self.gpu_lost |= dx12.is_err();
            if self.retry_at.is_none() {
                let dx11 = self.live.as_ref().map(|live| live.capture.removed_reason());
                log::warn!(
                    "glass overlays: device lost during {operation}: {error:#}; GetDeviceRemovedReason D3D11={dx11:?}, DX12={dx12:?}; recovering"
                );
                self.discard_device();
                self.retry_at = Some(Instant::now() + self.retry_delay);
            }
        } else {
            let message = format!("{operation}: {error:#}");
            if self.failure.as_ref() != Some(&message) {
                log::warn!("glass overlays: {message}");
                self.failure = Some(message);
            }
        }
    }

    fn reopen(&mut self) -> Result<()> {
        // SAFETY: read-only stale-window check. A Close received during a cold open stays
        // queued and is processed in order on the next tick.
        let desired = self
            .desired
            .live(|hwnd| unsafe { IsWindow(Some(hwnd)) }.as_bool());
        for overlay in desired {
            self.apply(overlay.open())?;
            self.apply(Request::Style {
                client: overlay.client,
                style: overlay.style,
            })?;
        }
        for plate in self.plates.clone() {
            // Frozen plates need a fresh crop on the new device before they can freeze again.
            self.sync_plate(&PlateOverlay {
                live: true,
                ..plate
            })?;
        }
        self.sync_placements();
        Ok(())
    }

    /// Creates the request window (`OVERLAY_CLASS`) on this thread. Nothing else exists until
    /// an overlay opens.
    pub fn new(gpu: Gpu) -> Result<Self> {
        capture::ask_borderless();
        let inbox = Rc::new(Inbox {
            diagnostics: std::env::var_os("MYPAPER_TEST_OVERLAY_RECOVERY").is_some(),
            ..Default::default()
        });
        let window = MessageWindow::new(&inbox)?;
        Ok(Self {
            window,
            inbox,
            gpu: gpu.into(),
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
            interval: capture::MIN_INTERVAL,
            desired: recovery::Requests::default(),
            plates: Vec::new(),
            retry_at: None,
            retry_delay: Duration::from_secs(1),
            gpu_lost: false,
            failure: None,
        })
    }

    /// Caps captured frames at the theme's frame rate (`engine.fps`) when that is below the
    /// capture's own 30 a second. Plates over a web or app wallpaper redraw at most this often.
    pub fn set_fps(&mut self, fps: Option<u32>) {
        let interval = capture::interval(fps);
        if interval == self.interval {
            return;
        }
        self.interval = interval;
        for session in &self.sessions {
            session.set_interval(interval);
        }
    }

    /// Next coalesced placement or capture going cold: call [`tick`](Self::tick) by then.
    pub fn deadline(&self) -> Option<Instant> {
        self.warm_until
            .into_iter()
            .chain(self.inbox.motion_deadline())
            .chain(self.retry_at)
            .min()
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
        if self.retry_at.is_none() && self.gpu.get().is_some() {
            let reason = self.gpu.removed_reason();
            if reason.is_err() {
                self.failed(
                    "shared device health",
                    windows::core::Error::from_hresult(reason).into(),
                );
            }
        }
        match self.inbox.test_loss.take() {
            1 => self.failed(
                "diagnostic capture loss",
                windows::core::Error::from_hresult(
                    windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED,
                )
                .into(),
            ),
            2 => {
                use windows::core::Interface;
                // SAFETY: opt-in fixture removes only our process's logical device, never
                // another application's device. Production does not accept this message.
                unsafe {
                    if let Some(hal) = self.gpu.device.as_hal::<wgpu::hal::api::Dx12>()
                        && let Ok(device) =
                            hal.raw_device()
                                .cast::<windows::Win32::Graphics::Direct3D12::ID3D12Device5>()
                    {
                        device.RemoveDevice();
                    }
                }
                self.failed(
                    "diagnostic DX12 loss",
                    windows::core::Error::from_hresult(
                        windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED,
                    )
                    .into(),
                );
            }
            _ => {}
        }
        if self.retry_at.is_none()
            && let Some(live) = &self.live
        {
            let reason = live.capture.removed_reason();
            if reason.is_err() {
                self.failed(
                    "capture health",
                    windows::core::Error::from_hresult(reason).into(),
                );
            }
        }
        self.inbox.take_motion(Instant::now());
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
                    self.failed("request", e);
                }
            }
        }
        if self.inbox.prune.take() {
            let mut gone: Vec<HWND> = self
                .open
                .iter()
                // SAFETY: a read-only query.
                .filter(|o| !unsafe { IsWindow(Some(o.client)) }.as_bool())
                .map(|o| o.client)
                .collect();
            gone.dedup();
            for client in gone {
                log::info!("glass overlay: client {client:?} is gone");
                self.inbox
                    .motion
                    .borrow_mut()
                    .retain(|m| m.client != client);
                // Its own overlay and any of its plates'.
                while let Some(o) = self.open.iter().position(|o| o.client == client) {
                    self.close_at(o);
                }
                self.desired.remember(Request::Close { client });
            }
        }
        if handled > 0 {
            self.sync_placements();
        }
        if self.inbox.frames.take() {
            for index in 0..self.sessions.len() {
                if let Some(error) = self.sessions[index].take_error() {
                    self.failed("frame pool", error.into());
                    break;
                }
                if let Some(frame) = self.sessions[index].take()
                    && let Err(e) = self.update(index, frame)
                {
                    self.failed("update", e);
                    // Recovery drops sessions, so no indices from this generation remain valid.
                    break;
                }
            }
        }
        if !self.gpu_lost && self.retry_at.is_some_and(|at| at <= Instant::now()) {
            match self.reopen() {
                Ok(()) => {
                    self.retry_at = None;
                    self.retry_delay = Duration::from_secs(1);
                    self.failure = None;
                    log::info!(
                        "glass overlays: device recovered; reopened {} overlays",
                        self.open.len()
                    );
                }
                Err(e) => {
                    self.failed("reopen", e);
                    self.discard_device();
                    self.retry_delay = (self.retry_delay * 2).min(Duration::from_secs(30));
                    self.retry_at = Some(Instant::now() + self.retry_delay);
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
        self.desired.remember(request);
        if self.retry_at.is_some() {
            return Ok(());
        }
        self.apply(request)
    }

    fn apply(&mut self, request: Request) -> Result<()> {
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
                self.open(client, self.open[o].layer, rect, radius, material)
            }
            Request::Material { client, material } => {
                let Some(o) = self.find(client) else {
                    return Ok(());
                };
                let (rect, radius) = (self.open[o].rect, self.open[o].radius);
                self.open(client, self.open[o].layer, rect, radius, material)
            }
            Request::Close { client } => {
                self.close(client);
                Ok(())
            }
            Request::Style { client, style } => {
                for o in 0..self.open.len() {
                    if self.open[o].client == client && self.open[o].plate.is_none() {
                        self.restyle(o, style)?;
                    }
                }
                Ok(())
            }
        }
    }

    /// Makes the engine's own overlays match `plates`, the plates over web and app wallpapers
    /// (see the module docs): opens the new live ones, moves, restyles, freezes or thaws the
    /// rest, and closes those gone. Unchanged plates cost a comparison each.
    pub fn sync_plates(&mut self, plates: &[PlateOverlay]) {
        self.plates = plates.to_vec();
        if self.retry_at.is_some() {
            return;
        }
        while let Some(o) = self.open.iter().position(|o| {
            o.plate
                .is_some_and(|id| !plates.iter().any(|p| p.id == id && p.client == o.client))
        }) {
            self.close_at(o);
        }
        if self.retry_at.is_some() {
            return;
        }
        for plate in plates {
            if let Err(e) = self.sync_plate(plate) {
                self.failed("plate", e);
                break;
            }
        }
    }

    fn sync_plate(&mut self, plate: &PlateOverlay) -> Result<()> {
        if !plate.live && !self.open.iter().any(|o| o.plate == Some(plate.id)) {
            return Ok(());
        }
        self.upsert(
            plate.client,
            Some(plate.id),
            plate.layer,
            plate.rect,
            plate.radius,
            plate.material,
        )?;
        for o in 0..self.open.len() {
            if self.open[o].plate == Some(plate.id) {
                self.restyle(o, plate.style)?;
                self.set_frozen(o, !plate.live)?;
            }
        }
        Ok(())
    }

    /// Takes overlay `o`'s finer knobs: shown glass redraws with them at once; the fade only
    /// applies to glass not shown yet.
    fn restyle(&mut self, o: usize, style: Style) -> Result<()> {
        if std::mem::replace(&mut self.open[o].style, style) == style {
            return Ok(());
        }
        let overlay = &self.open[o];
        overlay.window.set_clip(
            geometry::local(overlay.rect, overlay.frame),
            overlay.radius,
            style.smoothing,
        )?;
        if overlay.shown.is_some() && !overlay.fresh {
            self.draw(&[o])?;
        }
        Ok(())
    }

    /// Freezes overlay `o` (its glass stays as drawn, nothing is captured for it, and a monitor
    /// left with only frozen overlays loses its session) or thaws it (it takes the next frame).
    fn set_frozen(&mut self, o: usize, frozen: bool) -> Result<()> {
        // A rebuilt frozen plate needs one fresh crop before retaining its last frame again.
        if frozen && self.open[o].shown.is_none() {
            return Ok(());
        }
        if std::mem::replace(&mut self.open[o].frozen, frozen) == frozen {
            return Ok(());
        }
        let (monitor, bounds) = (self.open[o].monitor, self.open[o].bounds);
        if frozen {
            self.sync_capture(None)?;
            self.drop_idle_sessions();
            return Ok(());
        }
        self.open[o].fresh = true;
        if !self.sessions.iter().any(|s| s.monitor == monitor) {
            let live = self.live.take().context("no overlays")?;
            let started = self.start_session(&live.capture, monitor, bounds);
            self.live = Some(live);
            started?;
        }
        self.sync_capture(Some(monitor))
    }

    /// The shell's own overlay for `client` (not one of the engine's plates).
    fn find(&self, client: HWND) -> Option<usize> {
        self.open
            .iter()
            .position(|o| o.client == client && o.plate.is_none())
    }

    fn open(
        &mut self,
        client: HWND,
        layer: Layer,
        rect: Rect,
        radius: f32,
        material: Material,
    ) -> Result<()> {
        self.upsert(client, None, layer, rect, radius, material)
    }

    /// One piece per intersected monitor, each using the full shape in local coordinates.
    /// Internal monitor edges therefore do not become rounded corners of the client.
    fn upsert(
        &mut self,
        client: HWND,
        plate: Option<u64>,
        layer: Layer,
        rect: Rect,
        radius: f32,
        material: Material,
    ) -> Result<()> {
        let monitors = pieces(rect);
        while let Some(o) = self.open.iter().position(|o| {
            o.client == client && o.plate == plate && !monitors.iter().any(|(m, _)| *m == o.monitor)
        }) {
            self.close_at(o);
        }
        for (monitor, bounds) in monitors {
            if let Some(o) = self
                .open
                .iter()
                .position(|o| o.client == client && o.plate == plate && o.monitor == monitor)
            {
                let material_changed = self.open[o].material != material;
                self.place(o, rect, radius, material)?;
                if material_changed && self.open[o].shown.is_some() && !self.open[o].fresh {
                    self.draw(&[o])?;
                }
            } else {
                self.create(
                    client, plate, layer, rect, radius, material, monitor, bounds,
                )?;
            }
        }
        Ok(())
    }

    /// A new overlay under `client`: the shell's (`plate` `None`) or one of the engine's plates.
    fn create(
        &mut self,
        client: HWND,
        plate: Option<u64>,
        layer: Layer,
        rect: Rect,
        radius: f32,
        material: Material,
        monitor: HMONITOR,
        bounds: Rect,
    ) -> Result<()> {
        let opened = Instant::now();
        if self.live.is_none() {
            let capture = CaptureDevice::new(&self.gpu.adapter, &self.gpu.device, &self.gpu.queue)?;
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
            0.0,
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
            layer,
            plate,
            frozen: false,
            rect,
            radius,
            material,
            style: Style::DEFAULT,
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
        let session = Session::start(
            capture,
            monitor,
            bounds,
            self.interval,
            self.window.0,
            WM_FRAME,
        )?;
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
        let (monitor, bounds) = (self.open[o].monitor, self.open[o].bounds);
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
            overlay.style.smoothing,
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
        let frozen = overlay.frozen;
        if previous != monitor {
            if !frozen && !self.sessions.iter().any(|s| s.monitor == monitor) {
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
        while let Some(o) = self.find(client) {
            self.close_at(o);
        }
    }

    fn close_at(&mut self, o: usize) {
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
            self.failed("close", e);
        }
    }

    /// Ends the sessions no overlay uses, unless the capture is being kept warm.
    fn drop_idle_sessions(&mut self) {
        let warm = self.warm_until.is_some_and(|at| at > Instant::now());
        let open = &self.open;
        let closed = &mut self.closed_counts;
        self.sessions.retain(|s| {
            let used = warm || open.iter().any(|o| o.monitor == s.monitor && !o.frozen);
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
                .filter(|o| o.monitor == session.monitor && !o.frozen)
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
        // The shell's overlays only: a `Place` names its client's own.
        *self.inbox.placements.borrow_mut() = self
            .open
            .iter()
            .filter(|o| o.plate.is_none())
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
                    && !overlay.frozen
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
            )?;
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
        render(&mut self.open, &early, true, &self.gpu, &mut encoder, live);
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
            render(&mut self.open, &late, false, &self.gpu, &mut encoder, live);
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
        render(&mut self.open, which, false, &self.gpu, &mut encoder, live);
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
                .show(&live.dcomp, &live.capture, overlay.targets.glass())?;
            let fade = Duration::from_secs_f32(overlay.style.fade_ms / 1000.0);
            match overlay.shown {
                None => {
                    overlay
                        .window
                        .fade_in(&live.dcomp, fade, overlay.style.fade_curve)?;
                    overlay.shown = Some(Instant::now());
                    overlay.faded = fade.is_zero();
                    self.stats.open_to_glass = Some(overlay.opened.elapsed());
                    log::info!(
                        "glass overlay: open to glass {:.1} ms",
                        overlay.opened.elapsed().as_secs_f64() * 1000.0
                    );
                }
                Some(at) if !overlay.faded && at.elapsed() > fade => {
                    overlay.window.settle()?;
                    overlay.faded = true;
                }
                Some(_) => {}
            }
        }
        // SAFETY: a commit on a live device: the new glass shows at the next composition.
        unsafe { live.dcomp.Commit()? };
        self.stats.passes += which.len() as u64;
        self.failure = None;
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
            &overlay.style,
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

fn pieces(rect: Rect) -> Vec<(HMONITOR, Rect)> {
    let mut monitors: Vec<_> = mypaper_desktop::monitor_infos()
        .into_iter()
        .filter(|m| geometry::touches(rect, m.rect))
        .map(|m| monitor_of(m.rect))
        .collect();
    if monitors.is_empty() {
        monitors.push(monitor_of(rect));
    }
    monitors
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
    for p in placements.iter_mut().filter(|p| p.client == client) {
        let frame = geometry::frame(rect, geometry::reach(rect, &p.material, p.scale), p.bounds);
        let same_size = (frame.width, frame.height) == (p.frame.width, p.frame.height);
        if same_size
            && frame != p.frame
            && window::move_under(p.overlay, client, frame, true).is_ok()
        {
            p.frame = frame;
            inbox.moved_at_once.set(inbox.moved_at_once.get() + 1);
        }
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
                    inbox.receive(request);
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
        (WM_PLACEMENT, Some(inbox)) => {
            let client = HWND(wparam.0 as *mut _);
            if let Some(motion) = inbox
                .motion
                .borrow_mut()
                .iter_mut()
                .find(|m| m.client == client)
            {
                motion.pending = true;
            }
            LRESULT(0)
        }
        (WM_FRAME, Some(inbox)) => {
            inbox.frames.set(true);
            LRESULT(0)
        }
        (WM_PRUNE, Some(inbox)) => {
            inbox.prune.set(true);
            LRESULT(0)
        }
        (WM_TEST_RECOVERY, Some(inbox)) if inbox.diagnostics => {
            inbox.test_loss.set(wparam.0);
            LRESULT(0)
        }
        // SAFETY: the system's own message, forwarded unchanged.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod plate_tests {
    use super::*;

    #[test]
    fn a_plate_becomes_an_overlay_only_under_a_live_window_of_its_publisher() {
        // SAFETY: a plain child-less STATIC window of this test process, destroyed below.
        let window = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("plate"),
                Default::default(),
                0,
                0,
                10,
                10,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        let plate = mypaper_glass_ipc::Plate {
            // A bar's second shape on the same window: the window is in the low 32 bits.
            id: (window.0 as usize as u64 & 0xFFFF_FFFF) | (1 << 32),
            rect: [-2559.6, 10.4, 300.2, 99.5],
            radius: 12.0,
            material: mypaper_glass_ipc::Material {
                tint: [0.1, 0.1, 0.12, 0.4],
                ..Default::default()
            },
        };
        let style = mypaper_glass_ipc::Style {
            smoothing: 0.6,
            sheen: Some(0.5),
            ..Default::default()
        };
        let me = std::process::id();
        let p = PlateOverlay::of(&plate, &style, me, true).expect("our own window");
        assert_eq!(p.client, window);
        assert_eq!(p.layer, Layer::Desktop, "not topmost: the desktop band");
        assert_eq!(
            p.rect,
            Rect {
                x: -2560,
                y: 10,
                width: 301,
                height: 100
            }
        );
        assert_eq!(
            p.material.tint,
            [26, 26, 31, 102],
            "the stained tint survives"
        );
        assert_eq!(
            (p.style.smoothing, p.style.sheen, p.style.depth),
            (0.6, 0.5, -1.0)
        );
        assert_eq!(p.style.fade_ms, Style::DEFAULT.fade_ms);
        assert!(
            PlateOverlay::of(&plate, &style, me + 1, true).is_none(),
            "another publisher"
        );
        // SAFETY: our own window.
        unsafe { DestroyWindow(window).unwrap() };
        assert!(
            PlateOverlay::of(&plate, &style, me, true).is_none(),
            "a stale id"
        );
        let auto = mypaper_glass_ipc::Plate { id: 3, ..plate };
        assert!(
            PlateOverlay::of(&auto, &style, me, true).is_none(),
            "an id without a window"
        );
    }

    #[test]
    fn the_capture_rate_follows_a_lower_fps_cap_only() {
        assert_eq!(capture::interval(None), capture::MIN_INTERVAL);
        assert_eq!(capture::interval(Some(60)), capture::MIN_INTERVAL);
        assert_eq!(capture::interval(Some(10)), Duration::from_millis(100));
    }

    #[test]
    fn overlays_keep_a_plates_stained_tint() {
        let m = Material {
            tint: [255, 255, 255, 102],
            ..Material::CLEAR
        };
        let [r, _, _, a] = glass::controls(&m).tint;
        assert!(r == 1.0 && (a - 0.4).abs() < 1e-6);
    }
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    #[test]
    fn motion_coalesces_per_frame_and_close_during_open_cannot_be_undone_by_a_wake() {
        let client = HWND((0x7100_0000usize + std::process::id() as usize) as *mut _);
        let initial = Request::Open {
            client,
            layer: Layer::Popup,
            rect: Rect {
                x: 10,
                y: 10,
                width: 300,
                height: 100,
            },
            radius: 12.0,
            material: Material::CLEAR,
        };
        let mut writer = Channel::open(client, true).unwrap();
        writer.write(&initial);
        let inbox = Inbox::default();
        inbox.receive(initial);
        // Open has been dequeued but its expensive capture setup has not completed yet.
        assert_eq!(inbox.requests.borrow_mut().pop_front(), Some(initial));
        let update = |x| Request::Open {
            client,
            layer: Layer::Popup,
            rect: Rect {
                x,
                y: 10,
                width: 300,
                height: 100,
            },
            radius: 14.0,
            material: Material {
                blur: 8.0,
                ..Material::CLEAR
            },
        };
        let now = Instant::now();
        for x in 20..30 {
            writer.write(&update(x));
        }
        inbox.motion.borrow_mut()[0].pending = true;
        inbox.take_motion(now);
        assert_eq!(inbox.requests.borrow_mut().pop_front(), Some(update(29)));
        writer.write(&update(40));
        inbox.motion.borrow_mut()[0].pending = true;
        inbox.take_motion(now);
        assert!(inbox.requests.borrow().is_empty(), "one update per frame");
        let next = inbox.motion_deadline().unwrap();
        writer.write(&update(50));
        inbox.take_motion(next);
        assert_eq!(inbox.requests.borrow_mut().pop_front(), Some(update(50)));
        writer.write(&update(60));
        inbox.motion.borrow_mut()[0].pending = true;
        inbox.receive(Request::Close { client });
        inbox.take_motion(next + Duration::from_secs(1));
        assert_eq!(
            inbox.requests.borrow_mut().pop_front(),
            Some(Request::Close { client })
        );
        assert!(
            inbox.requests.borrow().is_empty(),
            "no stale motion after close"
        );
        assert!(inbox.motion_deadline().is_none());
        // Reopening the same HWND gets a new lifecycle and still accepts motion.
        writer.write(&initial);
        inbox.receive(initial);
        writer.write(&update(70));
        inbox.motion.borrow_mut()[0].pending = true;
        inbox.take_motion(Instant::now());
        assert_eq!(
            inbox.requests.borrow_mut().drain(..).collect::<Vec<_>>(),
            vec![initial, update(70)]
        );
    }
}
