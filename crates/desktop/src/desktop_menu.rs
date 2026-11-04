//! mypaper's own right-click menu for the bare desktop (opt-in,
//! `desktop_menu.enabled`; docs/desktop-extras.md).
//!
//! **Two threads, both ours; nothing is injected anywhere.**
//!
//! - The **hook thread** owns a `WH_MOUSE_LL` hook and does nothing but pump
//!   messages. Its callback returns in microseconds: for everything except a
//!   right-button press or release it returns at once; for those it asks a
//!   pure decision ([`Swallower`]) using a window-class check and a cached list of
//!   the icons' rectangles ([`IconMap`]), and posts a message. A right-click that lands on the
//!   bare desktop is swallowed (press and release); everything else, in particular
//!   every click on an icon, passes untouched.
//! - The **menu thread** does the slow work: it reads the icon rectangles from the
//!   desktop's folder view, confirms each swallowed click with UI Automation
//!   (`ElementFromPoint`: a list *item* is an icon), draws the menu
//!   ([`menu_ui`](crate::menu_ui)) and runs the chosen command. If the confirmation says
//!   "icon" (the cached rectangles were stale), the click is replayed to the
//!   native desktop. "Show more options" does the same: the right-click is replayed,
//!   marked as ours so the hook lets it through once.
//!
//! The hook is not installed at all while a fullscreen app is in front, the session is
//! locked or the display is off (`Shift`+right-click is always native).

use std::collections::VecDeque;
use std::process::Command as Process;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicIsize, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;

use anyhow::{Context, Result, anyhow};
use mypaper_config::DesktopMenu as MenuConfig;
use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, HWINEVENTHOOK, IUIAutomation, SetWinEventHook, UIA_ListItemControlTypeId,
    UnhookWinEvent,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEINPUT, SendInput, VK_SHIFT,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, GetMessageW, HHOOK, HWND_MESSAGE, KillTimer,
    LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostMessageW,
    PostThreadMessageW, RegisterClassExW, SW_SHOWNORMAL, SetCursorPos, SetTimer, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, WH_MOUSE_LL, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WM_APP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE,
    WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_TIMER, WM_XBUTTONDOWN, WNDCLASSEXW,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::control::ControlWindow;
use crate::desktop_hit::{self, Click, Edge, IconMap, Swallower, Under, Verdict};
use crate::desktop_view::{self, Com, DesktopView, SortKey};
use crate::fullscreen_app_in_front;
use crate::hold::Fact;
use crate::menu_model::{self, Command, ViewState};
pub use crate::menu_ui::Look;
use crate::{menu_ui, monitors};

/// Everything the menu takes from the config and the theme. Replaced as a whole
/// by [`DesktopMenu::update`]; read each time the menu opens.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub menu: MenuConfig,
    pub look: Look,
}

/// A menu item that belongs to the rest of mypaper, not to the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    /// "Change wallpaper".
    OpenStudio,
    /// "Edit widgets".
    EditWidgets,
}

/// Marks the right-clicks we send ourselves, so our hook lets them through.
const OURS: usize = 0x6d79_7061; // "mypa"

// Messages to the menu thread.
const WM_RECHECK: u32 = WM_APP + 40;
const WM_FACT: u32 = crate::control::WM_FACT;
const WM_REQUEST: u32 = WM_APP + 41;
const WM_RELEASE: u32 = WM_APP + 42;
const WM_OUTSIDE: u32 = WM_APP + 43;
const WM_ICONS_DIRTY: u32 = WM_APP + 44;
/// A chosen item is waiting in `COMMANDS`.
const WM_RUN: u32 = WM_APP + 46;
// Message to the hook thread: wParam 1 = install the hook, 0 = remove it.
const WM_HOOK_SET: u32 = WM_APP + 45;

/// A refresh of the icon rectangles this long after the last click on the desktop
/// (long enough for Explorer to have moved the icon).
const ICONS_SETTLE_MS: u32 = 300;
const ICONS_TIMER: usize = 1;
const INBOX_CLASS: PCWSTR = w!("mypaper.desktopmenu.inbox");

// --- state shared with the hook callback (which must never block) ---------------------------

/// Where the icons are; replaced whole by the menu thread, read by the hook.
static ICONS: RwLock<IconMap> = RwLock::new(IconMap::empty());
/// The menu thread's inbox window (a raw handle; 0 while the menu is not running).
static INBOX_WINDOW: AtomicIsize = AtomicIsize::new(0);
/// The menu is on screen: a click anywhere outside it closes it.
static MENU_OPEN: AtomicBool = AtomicBool::new(false);
static ICONS_DIRTY_QUEUED: AtomicBool = AtomicBool::new(false);
static RECHECK_QUEUED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Only the hook thread runs the callback.
    static SWALLOWER: std::cell::RefCell<Swallower> = std::cell::RefCell::new(Swallower::default());
}

// --- measuring the hook ---------------------------------------------------------------------

static STATS_ON: AtomicBool = AtomicBool::new(false);
static STATS: [Stat; 3] = [Stat::new(), Stat::new(), Stat::new()];

/// Clock ticks spent in one phase of the callback: a sum and a worst case.
struct Phase {
    total: AtomicI64,
    max: AtomicI64,
}

impl Phase {
    const fn new() -> Self {
        Self {
            total: AtomicI64::new(0),
            max: AtomicI64::new(0),
        }
    }

    fn add(&self, ticks: i64) {
        self.total.fetch_add(ticks, Ordering::Relaxed);
        self.max.fetch_max(ticks, Ordering::Relaxed);
    }

    fn reset(&self) {
        self.total.store(0, Ordering::Relaxed);
        self.max.store(0, Ordering::Relaxed);
    }
}

/// What the callback cost for one kind of event, split into our own work and the time
/// `CallNextHookEx` spent in whatever hooks other programs have installed after ours.
struct Stat {
    count: AtomicU64,
    own: Phase,
    chain: Phase,
}

impl Stat {
    const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            own: Phase::new(),
            chain: Phase::new(),
        }
    }

    fn record(&self, own: i64, chain: i64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.own.add(own);
        self.chain.add(chain);
    }
}

/// How long the hook callback took for one kind of event.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HookCost {
    pub events: u64,
    /// Our own work in the callback: what this hook adds to every mouse event.
    pub mean_us: f64,
    pub max_us: f64,
    /// Time spent passing the event on to other programs' hooks (not ours to control).
    pub chain_mean_us: f64,
    pub chain_max_us: f64,
}

/// What the callback cost, by kind of event (see [`measure_hook`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HookStats {
    /// Mouse moves: the overwhelming majority of events.
    pub moves: HookCost,
    /// Other mouse events (left and middle buttons, wheel).
    pub other: HookCost,
    /// Right-button presses and releases: the only ones that run the decision.
    pub right_button: HookCost,
}

/// Starts (and resets) or stops timing the hook callback with
/// `QueryPerformanceCounter`. Off by default: two clock reads per event.
pub fn measure_hook(on: bool) {
    for stat in &STATS {
        stat.count.store(0, Ordering::Relaxed);
        stat.own.reset();
        stat.chain.reset();
    }
    STATS_ON.store(on, Ordering::Relaxed);
}

pub fn hook_stats() -> HookStats {
    let mut frequency = 0i64;
    // SAFETY: the out pointer is a valid local.
    let _ = unsafe { QueryPerformanceFrequency(&mut frequency) };
    let us = |ticks: f64| ticks * 1e6 / frequency.max(1) as f64;
    let cost = |stat: &Stat| {
        let events = stat.count.load(Ordering::Relaxed);
        let mean = |phase: &Phase| {
            if events == 0 {
                0.0
            } else {
                us(phase.total.load(Ordering::Relaxed) as f64 / events as f64)
            }
        };
        HookCost {
            events,
            mean_us: mean(&stat.own),
            max_us: us(stat.own.max.load(Ordering::Relaxed) as f64),
            chain_mean_us: mean(&stat.chain),
            chain_max_us: us(stat.chain.max.load(Ordering::Relaxed) as f64),
        }
    };
    HookStats {
        moves: cost(&STATS[0]),
        other: cost(&STATS[1]),
        right_button: cost(&STATS[2]),
    }
}

// --- the public handle ----------------------------------------------------------------------

/// The running menu: its threads stop when this is dropped.
pub struct DesktopMenu {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
    settings: Arc<Mutex<Settings>>,
}

impl DesktopMenu {
    /// Starts the menu thread (and, through it, the hook thread). `on_app` runs
    /// on the menu thread for the items that belong to the rest of mypaper; keep it quick.
    pub fn start(settings: Settings, on_app: impl Fn(AppAction) + Send + 'static) -> Result<Self> {
        let settings = Arc::new(Mutex::new(settings));
        let (ready_tx, ready_rx) = mpsc::channel();
        let shared = settings.clone();
        let thread = std::thread::Builder::new()
            .name("desktop-menu".into())
            .spawn(move || menu_thread(shared, Box::new(on_app), ready_tx))
            .context("spawning the desktop menu thread")?;
        let thread_id = ready_rx
            .recv()
            .map_err(|_| anyhow!("the desktop menu thread died while starting"))??;
        Ok(Self {
            thread_id,
            thread: Some(thread),
            settings,
        })
    }

    /// New items or look, used from the next time the menu opens.
    pub fn update(&self, settings: Settings) {
        if let Ok(mut current) = self.settings.lock() {
            *current = settings;
        }
    }
}

impl Drop for DesktopMenu {
    fn drop(&mut self) {
        // SAFETY: posting to a thread id is always safe; failure means it is gone.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// --- the hook thread ------------------------------------------------------------------------

/// The thread that owns the mouse hook.
struct HookThread {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl HookThread {
    fn spawn() -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("desktop-menu-hook".into())
            .spawn(move || hook_main(tx))
            .context("spawning the mouse hook thread")?;
        let thread_id = rx
            .recv()
            .context("the mouse hook thread died while starting")?;
        Ok(Self {
            thread_id,
            thread: Some(thread),
        })
    }

    /// Installs or removes the hook (asynchronously).
    fn set(&self, installed: bool) {
        // SAFETY: posting to a thread id is always safe.
        let _ = unsafe {
            PostThreadMessageW(
                self.thread_id,
                WM_HOOK_SET,
                WPARAM(usize::from(installed)),
                LPARAM(0),
            )
        };
    }
}

impl Drop for HookThread {
    fn drop(&mut self) {
        // SAFETY: as above.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn hook_main(ready: mpsc::Sender<u32>) {
    // Physical pixels in the hook's coordinates, whatever the process default is.
    // SAFETY: affects this thread only.
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let mut msg = MSG::default();
    // SAFETY: creates this thread's message queue before its id is published.
    let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
    // SAFETY: no preconditions.
    let _ = ready.send(unsafe { GetCurrentThreadId() });

    let mut hook: Option<HHOOK> = None;
    // SAFETY: standard message loop; the hook procedure runs inside `GetMessageW`.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOOK_SET {
                match (msg.wParam.0 != 0, hook) {
                    (true, None) => hook = install_hook(),
                    (false, Some(installed)) => {
                        let _ = UnhookWindowsHookEx(installed);
                        hook = None;
                        // A swallowed press must not wait for a release that can no longer come.
                        SWALLOWER.with(|s| *s.borrow_mut() = Swallower::default());
                    }
                    _ => {}
                }
            } else {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if let Some(installed) = hook {
            let _ = UnhookWindowsHookEx(installed);
        }
    }
}

fn install_hook() -> Option<HHOOK> {
    // SAFETY: a low-level hook with a 'static procedure, installed by the thread that pumps messages.
    let installed = unsafe {
        let module = GetModuleHandleW(None).ok()?;
        SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), Some(HINSTANCE(module.0)), 0)
    };
    installed
        .inspect_err(|e| log::warn!("desktop menu: mouse hook not installed: {e}"))
        .ok()
}

/// The `WH_MOUSE_LL` callback. Microseconds: mouse moves return at once; only the right
/// button runs the (pure) decision, and the rest is a thread message.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let started = STATS_ON.load(Ordering::Relaxed).then(now_ticks);
    let message = wparam.0 as u32;
    let mut swallow = false;
    let mut kind = 1; // stats: other events
    if code >= 0 {
        match message {
            WM_MOUSEMOVE => kind = 0,
            WM_RBUTTONDOWN | WM_RBUTTONUP => {
                kind = 2;
                swallow = on_right_button(message == WM_RBUTTONDOWN, lparam);
            }
            WM_LBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
                if MENU_OPEN.load(Ordering::Relaxed) =>
            {
                // SAFETY: for a low-level mouse hook, lparam points at an MSLLHOOKSTRUCT.
                let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
                post(WM_OUTSIDE, pack(info.pt.x, info.pt.y));
            }
            WM_LBUTTONUP => {
                // A drop on the desktop may have moved an icon: refresh the rectangles soon.
                // SAFETY: as above.
                let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
                if desktop_hit::under(info.pt) == Under::Desktop
                    && !ICONS_DIRTY_QUEUED.swap(true, Ordering::Relaxed)
                {
                    post(WM_ICONS_DIRTY, 0);
                }
            }
            _ => {}
        }
    }
    // Our own work ends here; what follows is other programs' hooks.
    let decided = started.map(|_| now_ticks());
    let result = if swallow {
        LRESULT(1)
    } else {
        // SAFETY: passing the event on, with the arguments we were given.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    };
    if let (Some(started), Some(decided)) = (started, decided) {
        STATS[kind].record(decided - started, now_ticks() - decided);
    }
    result
}

/// The decision for a right-button event; posts the message that follows from it.
fn on_right_button(down: bool, lparam: LPARAM) -> bool {
    // SAFETY: for a low-level mouse hook, lparam points at an MSLLHOOKSTRUCT.
    let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
    let click = Click {
        edge: if down { Edge::Down } else { Edge::Up },
        at: (info.pt.x, info.pt.y),
        // SAFETY: no preconditions.
        now_ms: unsafe { GetTickCount64() },
        ours: info.flags & LLMHF_INJECTED != 0 && info.dwExtraInfo == OURS,
        // SAFETY: no preconditions.
        shift: down && unsafe { GetAsyncKeyState(i32::from(VK_SHIFT.0)) } < 0,
    };
    let verdict = SWALLOWER.with(|swallower| {
        // A poisoned lock still holds a usable map: the writer only ever swaps it whole.
        let icons = ICONS.read().unwrap_or_else(|e| e.into_inner());
        swallower
            .borrow_mut()
            .decide(click, &icons, || desktop_hit::under(info.pt))
    });
    match verdict {
        Verdict::Pass => false,
        Verdict::SwallowDown => {
            post(WM_REQUEST, pack(info.pt.x, info.pt.y));
            true
        }
        Verdict::SwallowUp => {
            post(WM_RELEASE, 0);
            true
        }
    }
}

fn now_ticks() -> i64 {
    let mut ticks = 0i64;
    // SAFETY: the out pointer is a valid local.
    let _ = unsafe { QueryPerformanceCounter(&mut ticks) };
    ticks
}

/// Posts to the menu thread's inbox window (microseconds; never blocks).
///
/// A window, not the thread: the menu thread makes COM calls to Explorer, and COM's wait loop
/// dispatches (and so discards) thread messages that have no window, which would lose a
/// swallowed right-click's release. A window message always reaches [`inbox_proc`], which only
/// queues it for the main loop.
fn post(message: u32, lparam: isize) {
    let inbox = INBOX_WINDOW.load(Ordering::Relaxed);
    if inbox == 0 {
        return;
    }
    // SAFETY: posting to a window handle is always safe; failure means it is gone.
    let _ = unsafe {
        PostMessageW(
            Some(HWND(inbox as *mut _)),
            message,
            WPARAM(0),
            LPARAM(lparam),
        )
    };
}

/// A message from the hook, a timer or the menu, waiting for the menu thread's main loop.
#[derive(Debug, Clone, Copy)]
struct Msg {
    message: u32,
    wparam: usize,
    lparam: isize,
}

thread_local! {
    static INBOX: std::cell::RefCell<VecDeque<Msg>> = const { std::cell::RefCell::new(VecDeque::new()) };
}

/// Queues our messages (and the icon timer's ticks) and nothing else, whatever the thread is
/// doing when they arrive, so they are handled in order and never re-entrantly.
unsafe extern "system" fn inbox_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if (WM_RECHECK..=WM_RUN).contains(&message) || message == WM_TIMER {
        INBOX.with(|queue| {
            queue.borrow_mut().push_back(Msg {
                message,
                wparam: wparam.0,
                lparam: lparam.0,
            });
        });
        return LRESULT(0);
    }
    // SAFETY: default handling of a message we were called with.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// The message-only window that receives [`post`]s.
fn create_inbox() -> Result<HWND> {
    // SAFETY: plain Win32 registration and message-only window creation with a 'static procedure.
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(inbox_proc),
            hInstance: GetModuleHandleW(None)?.into(),
            lpszClassName: INBOX_CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
        }
        CreateWindowExW(
            Default::default(),
            INBOX_CLASS,
            w!("mypaper desktop menu inbox"),
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
        .context("creating the desktop menu inbox window")
    }
}

/// Screen coordinates as an `lParam` (signed 16-bit halves hold any virtual screen).
fn pack(x: i32, y: i32) -> isize {
    (i32::from(y as i16) << 16 | i32::from(x as i16) & 0xFFFF) as isize
}

fn unpack(lparam: isize) -> (i32, i32) {
    (i32::from(lparam as i16), i32::from((lparam >> 16) as i16))
}

// --- the menu thread ------------------------------------------------------------------------

/// A swallowed press waiting for its release.
struct Pending {
    at: (i32, i32),
    /// The UI Automation verdict: nothing but desktop at that point.
    bare: bool,
    /// The desktop's state for the check marks, read while the button is still down so that
    /// opening the menu on release does not wait for Explorer.
    state: ViewState,
}

struct Host {
    settings: Arc<Mutex<Settings>>,
    on_app: Box<dyn Fn(AppAction)>,
    uia: Option<IUIAutomation>,
    hook: HookThread,
    pending: Option<Pending>,
    locked: bool,
    display_off: bool,
    hook_on: bool,
    /// Where the menu was opened: "Show more options" replays the click there.
    opened_at: (i32, i32),
    /// An icon refresh is scheduled (a timer on the inbox window).
    icons_timer: bool,
    /// The window [`post`]s go to; also owns the icon timer.
    inbox: HWND,
}

fn menu_thread(
    settings: Arc<Mutex<Settings>>,
    on_app: Box<dyn Fn(AppAction) + Send>,
    ready: mpsc::Sender<Result<u32>>,
) {
    // SAFETY: affects this thread only.
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let _com = Com::init();
    let mut msg = MSG::default();
    // SAFETY: creates this thread's message queue before its id is published.
    let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
    // SAFETY: no preconditions.
    let thread_id = unsafe { GetCurrentThreadId() };

    let started = create_inbox().and_then(|inbox| Host::new(settings, on_app, inbox));
    let mut host = match started {
        Ok(host) => host,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    INBOX_WINDOW.store(host.inbox.0 as isize, Ordering::Relaxed);
    // Dropped when the loop ends: stops watching and closes the menu.
    let _control = ControlWindow::new(w!("mypaper.desktopmenu.control"))
        .inspect_err(|e| log::warn!("desktop menu: no lock/display notifications: {e:#}"))
        .ok();
    let events = watch_foreground();
    let _ = ready.send(Ok(thread_id));

    host.refresh_icons("start");
    host.sync_hook();
    // SAFETY: standard message loop on the thread that owns the menu windows.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.hwnd.is_invalid() && msg.message == WM_FACT {
                // Posted to the thread by the control window (power and session changes).
                host.handle(Msg {
                    message: msg.message,
                    wparam: msg.wParam.0,
                    lparam: msg.lParam.0,
                });
            } else {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            host.drain();
        }
    }
    INBOX_WINDOW.store(0, Ordering::Relaxed);
    menu_ui::uninstall();
    for hook in events {
        // SAFETY: hooks we installed.
        let _ = unsafe { UnhookWinEvent(hook) };
    }
    // SAFETY: our own window, on its creating thread.
    let _ = unsafe { DestroyWindow(host.inbox) };
}

fn watch_foreground() -> Vec<HWINEVENTHOOK> {
    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    // SAFETY: out-of-context hook with a 'static callback; events arrive through this thread's loop.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_MINIMIZEEND,
            None,
            Some(on_foreground),
            0,
            0,
            flags,
        )
    };
    if hook.is_invalid() {
        Vec::new()
    } else {
        vec![hook]
    }
}

unsafe extern "system" fn on_foreground(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    // Only whole windows count; carets and cursors fire constantly.
    if object == 0 && child == 0 && !RECHECK_QUEUED.swap(true, Ordering::Relaxed) {
        post(WM_RECHECK, 0);
    }
}

impl Host {
    fn new(
        settings: Arc<Mutex<Settings>>,
        on_app: Box<dyn Fn(AppAction)>,
        inbox: HWND,
    ) -> Result<Self> {
        let uia = create_uia()
            .inspect_err(|e| {
                log::warn!("desktop menu: no UI Automation, trusting the icon cache: {e:#}")
            })
            .ok();
        menu_ui::install(command_sink())?;
        Ok(Self {
            settings,
            on_app,
            uia,
            hook: HookThread::spawn()?,
            pending: None,
            locked: false,
            display_off: false,
            hook_on: false,
            opened_at: (0, 0),
            icons_timer: false,
            inbox,
        })
    }

    /// Handles every queued message, in order. Handling one may call into Explorer and so let
    /// more messages arrive; they are queued and handled here too.
    fn drain(&mut self) {
        while let Some(msg) = INBOX.with(|queue| queue.borrow_mut().pop_front()) {
            self.handle(msg);
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg.message {
            WM_REQUEST => {
                let at = unpack(msg.lparam);
                let bare = self.confirm_bare(at);
                self.pending = Some(Pending {
                    at,
                    bare,
                    state: if bare {
                        view_state()
                    } else {
                        ViewState::default()
                    },
                });
            }
            WM_RELEASE => {
                if let Some(pending) = self.pending.take() {
                    if pending.bare {
                        self.show(pending.at, pending.state);
                    } else {
                        log::debug!(
                            "desktop menu: stale icon cache; replaying the click to the desktop"
                        );
                        replay_right_click(pending.at);
                        self.refresh_icons("a stale cache");
                    }
                }
            }
            WM_RUN => {
                let commands = COMMANDS.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
                for command in commands {
                    self.run(command);
                }
            }
            WM_OUTSIDE => {
                let (x, y) = unpack(msg.lparam);
                if !menu_ui::is_open() {
                    MENU_OPEN.store(false, Ordering::Relaxed);
                } else if !menu_ui::contains(x, y) {
                    menu_ui::close();
                    MENU_OPEN.store(false, Ordering::Relaxed);
                }
            }
            WM_RECHECK => {
                RECHECK_QUEUED.store(false, Ordering::Relaxed);
                self.sync_hook();
            }
            WM_FACT => match Fact::from_wparam(msg.wparam) {
                Some(Fact::SessionLocked) => {
                    self.locked = msg.lparam != 0;
                    self.sync_hook();
                }
                Some(Fact::DisplayOff) => {
                    self.display_off = msg.lparam != 0;
                    self.sync_hook();
                }
                _ => {}
            },
            WM_ICONS_DIRTY => {
                // Debounce: the refresh runs once the clicking has stopped.
                // SAFETY: a timer on our own window; replacing it restarts the wait.
                self.icons_timer =
                    unsafe { SetTimer(Some(self.inbox), ICONS_TIMER, ICONS_SETTLE_MS, None) } != 0;
            }
            WM_TIMER if msg.wparam == ICONS_TIMER && self.icons_timer => {
                // SAFETY: kills the timer set above.
                let _ = unsafe { KillTimer(Some(self.inbox), ICONS_TIMER) };
                self.icons_timer = false;
                ICONS_DIRTY_QUEUED.store(false, Ordering::Relaxed);
                self.refresh_icons("a click on the desktop");
            }
            _ => {}
        }
    }

    /// Installs the hook unless a fullscreen app is in front, the session is locked or
    /// the display is off.
    fn sync_hook(&mut self) {
        let wanted = !self.locked && !self.display_off && !fullscreen_app_in_front();
        if wanted != self.hook_on {
            self.hook_on = wanted;
            self.hook.set(wanted);
            log::debug!("desktop menu: hook {}", if wanted { "on" } else { "off" });
            if wanted {
                self.refresh_icons("the hook coming on");
            }
        }
    }

    /// Re-reads the icons' rectangles for the hook's fast check.
    fn refresh_icons(&mut self, why: &str) {
        let rects = DesktopView::open().and_then(|view| view.icon_rects());
        match rects {
            Ok(rects) => {
                log::debug!("desktop menu: {} icons (after {why})", rects.len());
                if let Ok(mut icons) = ICONS.write() {
                    *icons = IconMap::new(rects);
                }
            }
            Err(e) => {
                log::debug!("desktop menu: icon rectangles unavailable: {e:#}");
                if let Ok(mut icons) = ICONS.write() {
                    *icons = IconMap::empty();
                }
            }
        }
    }

    /// Is there nothing but desktop at `at`? An icon is a UI Automation list *item*.
    /// Without an answer, the cached rectangles (which already said "bare") stand.
    fn confirm_bare(&self, at: (i32, i32)) -> bool {
        let Some(uia) = &self.uia else { return true };
        let started = std::time::Instant::now();
        // SAFETY: documented UI Automation calls on an interface we own.
        let verdict = unsafe {
            uia.ElementFromPoint(POINT { x: at.0, y: at.1 })
                .and_then(|element| element.CurrentControlType())
        };
        log::debug!(
            "desktop menu: UI Automation answered in {:?}",
            started.elapsed()
        );
        match verdict {
            Ok(kind) => kind != UIA_ListItemControlTypeId,
            Err(e) => {
                log::debug!("desktop menu: ElementFromPoint failed: {e}");
                true
            }
        }
    }

    fn show(&mut self, at: (i32, i32), state: ViewState) {
        self.opened_at = at;
        let settings = self.settings.lock().map(|s| s.clone());
        let Ok(settings) = settings else { return };
        let items = menu_model::build(&settings.menu, state);
        if items.is_empty() {
            replay_right_click(at);
            return;
        }
        let (work, dpi) = monitor_at(at);
        MENU_OPEN.store(true, Ordering::Relaxed);
        menu_ui::show(items, at, work, dpi, &settings.look);
    }

    /// Runs a chosen command (the menu is already closed).
    fn run(&mut self, command: Command) {
        let outcome = self.try_run(command);
        if let Err(e) = outcome {
            log::warn!("desktop menu: {e:#}");
        }
        MENU_OPEN.store(false, Ordering::Relaxed);
    }

    fn try_run(&mut self, command: Command) -> Result<()> {
        match command {
            Command::IconSize(size) => DesktopView::open()?.set_icon_size(size)?,
            Command::ToggleIcons => {
                let view = DesktopView::open()?;
                view.set_icons_visible(!view.icons_visible()?)?;
            }
            Command::SortBy(key) => {
                let view = DesktopView::open()?;
                let ascending = view.sort()?.is_none_or(|(_, up)| up);
                view.set_sort(key, ascending)?;
            }
            Command::SortOrder { ascending } => {
                let view = DesktopView::open()?;
                let key = view.sort()?.map_or(SortKey::Name, |(key, _)| key);
                view.set_sort(key, ascending)?;
            }
            Command::Refresh => DesktopView::open()?.refresh()?,
            Command::New(item) => DesktopView::open()?.create(item)?,
            Command::Paste => DesktopView::open()?.paste()?,
            Command::OpenStudio => (self.on_app)(AppAction::OpenStudio),
            Command::EditWidgets => (self.on_app)(AppAction::EditWidgets),
            Command::Display => shell_open("ms-settings:display", None)?,
            Command::Personalize => shell_open("ms-settings:personalization", None)?,
            Command::Terminal => open_terminal()?,
            Command::MoreOptions => replay_right_click(self.opened_at),
            Command::Custom { command, args } => run_custom(&command, &args)?,
        }
        // The view changed (or may have): the icons may have moved.
        self.refresh_icons("a menu command");
        Ok(())
    }
}

/// The callback `menu_ui` calls for a chosen item. The command is queued and a thread message
/// runs it, so it starts after the menu's own window procedure has finished.
fn command_sink() -> menu_ui::OnCommand {
    std::rc::Rc::new(|command| {
        COMMANDS.with(|queue| queue.borrow_mut().push(command));
        post(WM_RUN, 0);
    })
}

thread_local! {
    static COMMANDS: std::cell::RefCell<Vec<Command>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn view_state() -> ViewState {
    let clipboard = clipboard_has_files();
    match DesktopView::open() {
        Ok(view) => ViewState {
            icon_size: view.icon_size().ok(),
            icons_visible: view.icons_visible().unwrap_or(true),
            sort: view.sort().ok().flatten(),
            can_paste: clipboard,
        },
        Err(e) => {
            log::debug!("desktop menu: view state unavailable: {e:#}");
            ViewState {
                icons_visible: true,
                can_paste: clipboard,
                ..ViewState::default()
            }
        }
    }
}

fn clipboard_has_files() -> bool {
    use windows::Win32::System::DataExchange::IsClipboardFormatAvailable;
    // CF_HDROP: files copied or cut in Explorer.
    // SAFETY: no preconditions.
    unsafe { IsClipboardFormatAvailable(15) }.is_ok()
}

/// The work area and DPI of the monitor at `at`.
fn monitor_at(at: (i32, i32)) -> (menu_model::Edges, u32) {
    let work = monitors::monitor_infos()
        .into_iter()
        .find(|m| m.rect.contains_point(at.0, at.1))
        .map_or(
            (i32::MIN / 2, i32::MIN / 2, i32::MAX / 2, i32::MAX / 2),
            |m| {
                (
                    m.work.x,
                    m.work.y,
                    m.work.x + m.work.width,
                    m.work.y + m.work.height,
                )
            },
        );
    // SAFETY: plain queries.
    let dpi = unsafe {
        let monitor = MonitorFromPoint(POINT { x: at.0, y: at.1 }, MONITOR_DEFAULTTONEAREST);
        let (mut x, mut y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        x
    };
    (work, dpi)
}

fn create_uia() -> Result<IUIAutomation> {
    // SAFETY: documented COM activation, in the apartment this thread set up.
    Ok(unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?)
}

/// Sends a right click (press and release) at `at`, marked as ours so the hook passes it. The
/// cursor goes back to where the user clicked, so the native menu opens there.
fn replay_right_click(at: (i32, i32)) {
    let mouse = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                dwExtraInfo: OURS,
                ..Default::default()
            },
        },
    };
    // SAFETY: plain input injection with a fully initialized array.
    unsafe {
        let _ = SetCursorPos(at.0, at.1);
        SendInput(
            &[mouse(MOUSEEVENTF_RIGHTDOWN), mouse(MOUSEEVENTF_RIGHTUP)],
            size_of::<INPUT>() as i32,
        );
    }
}

/// `ShellExecuteW("open", file, params)`: documents, URIs like `ms-settings:`, programs.
fn shell_open(file: &str, params: Option<&str>) -> Result<()> {
    let file = HSTRING::from(file);
    let params = params.map(HSTRING::from);
    // SAFETY: the strings outlive the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &file,
            params
                .as_ref()
                .map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success as a value above 32.
    if result.0 as isize <= 32 {
        anyhow::bail!("could not open {file}");
    }
    Ok(())
}

/// Windows Terminal in the Desktop folder, or a console where it is not installed.
fn open_terminal() -> Result<()> {
    let desktop = desktop_view::desktop_folder()?;
    let quoted = format!("\"{}\"", desktop.display());
    shell_open("wt.exe", Some(&format!("-d {quoted}")))
        .or_else(|_| shell_open("cmd.exe", Some(&format!("/K cd /d {quoted}"))))
}

/// Runs a custom command. A program that cannot be started directly (a document, a URI)
/// goes to the shell.
fn run_custom(command: &str, args: &[String]) -> Result<()> {
    match Process::new(command).args(args).spawn() {
        Ok(_) => Ok(()),
        Err(direct) => {
            let joined = args
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(" ");
            shell_open(command, (!joined.is_empty()).then_some(joined.as_str()))
                .with_context(|| format!("starting {command}: {direct}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_points_survive_the_message_round_trip_including_negative_monitors() {
        for point in [
            (0, 0),
            (1280, 700),
            (-1280, 700),
            (-2560, -10),
            (4479, 1439),
            (-1, -1),
        ] {
            let packed = pack(point.0, point.1);
            assert_eq!(unpack(packed), point);
        }
    }
}
